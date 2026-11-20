use std::time::Duration;

use authn_kit::{
    AuthnError, access_jti_key, refresh_digest, refresh_grace_key, refresh_key,
    refresh_key_from_digest, subject_refresh_key, used_refresh_key,
};
use cache_kit::Cache;
use identity_contract::{Id, IdentityError};
use jwt_kit::TokenHelper;
use rootcause::{Report, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

/// [`consume_refresh_token`] 结果：首次消费或宽限期内幂等重放。
#[derive(Debug)]
pub enum ConsumeOutcome {
    Fresh(Id),
    GraceReplay(TokenPair),
}

pub(crate) fn map_cache_err(err: Report) -> Report {
    tracing::error!(error = %err, "cache unavailable");
    // 会话基础设施错误：归 authn 号段（-32023），由 http_auth 注册的 mapper 渲染
    Report::from(AuthnError::CacheUnavailable)
}

async fn del_refresh_by_digest(cache: &Cache, realm: &str, digest: Option<String>) -> Result<()> {
    match digest {
        Some(digest) => cache
            .del(&refresh_key_from_digest(realm, &digest))
            .await
            .map_err(map_cache_err),
        None => Ok(()),
    }
}

pub async fn issue_tokens(
    cache: &Cache,
    token_helper: &TokenHelper,
    user_id: &Id,
) -> Result<TokenPair> {
    let jti = Uuid::new_v4().to_string();
    let refresh_token = Uuid::new_v4().to_string();
    let (access_token, expires_in) = token_helper.encode_access_token(user_id, &jti)?;

    let realm = token_helper.realm();
    let user_refresh_key = subject_refresh_key(realm, user_id);
    let access_ttl = Duration::from_secs(token_helper.access_ttl_secs());
    let refresh_ttl = Duration::from_secs(token_helper.refresh_ttl_secs());

    // 旧 digest 需要先查（串行一轮），其余 3 个键互不依赖，并行写入。
    let old_digest = cache
        .get::<String>(&user_refresh_key)
        .await
        .map_err(map_cache_err)?;
    let user_id_str = user_id.to_string();
    let digest = refresh_digest(&refresh_token);
    let clear_old = del_refresh_by_digest(cache, realm, old_digest);
    let refresh_key = refresh_key_from_digest(realm, &digest);
    let jti_key = access_jti_key(realm, user_id);
    let set_refresh = cache.set_ex(&refresh_key, &user_id_str, refresh_ttl);
    let set_subject = cache.set_ex(&user_refresh_key, &digest, refresh_ttl);
    let set_jti = cache.set_ex(&jti_key, &jti, access_ttl);
    let (r0, r1, r2, r3) = tokio::join!(clear_old, set_refresh, set_subject, set_jti);
    [r0, r1, r2, r3]
        .into_iter()
        .try_for_each(|r| r.map_err(map_cache_err))?;

    Ok(TokenPair {
        access_token,
        refresh_token,
        expires_in,
    })
}

/// 原子消费一次 refresh。
///
/// - 首次：`Fresh(account_id)`；调用方须在签发新令牌后 [`commit_refresh_rotation`]。
/// - 命中 `used_*` 且宽限缓存仍在：`GraceReplay(pair)`（不吊销）。
/// - 命中 `used_*` 且宽限已过 / 关闭：吊销该账号全部会话并返回 `-32004`。
pub async fn consume_refresh_token(
    cache: &Cache,
    token_helper: &TokenHelper,
    refresh_token: &str,
) -> Result<ConsumeOutcome> {
    let realm = token_helper.realm();
    let digest = refresh_digest(refresh_token);
    let key = refresh_key_from_digest(realm, &digest);

    match cache.take::<Id>(&key).await.map_err(map_cache_err)? {
        Some(user_id) => Ok(ConsumeOutcome::Fresh(user_id)),
        None => {
            let Some(user_id) = cache
                .get::<Id>(&used_refresh_key(realm, &digest))
                .await
                .map_err(map_cache_err)?
            else {
                return Err(IdentityError::RefreshTokenInvalid.into());
            };
            if token_helper.grace_ttl_secs() > 0
                && let Some(pair) = cache
                    .get::<TokenPair>(&refresh_grace_key(realm, &digest))
                    .await
                    .map_err(map_cache_err)?
            {
                tracing::debug!(%user_id, "refresh replay within grace; returning cached pair");
                return Ok(ConsumeOutcome::GraceReplay(pair));
            }
            tracing::warn!(%user_id, "refresh token reuse detected, revoking all sessions");
            revoke_tokens(cache, token_helper, user_id).await?;
            Err(IdentityError::RefreshTokenInvalid.into())
        }
    }
}

pub async fn commit_refresh_rotation(
    cache: &Cache,
    token_helper: &TokenHelper,
    user_id: Id,
    consumed_refresh_token: &str,
    new_pair: &TokenPair,
) -> Result<()> {
    let realm = token_helper.realm();
    let digest = refresh_digest(consumed_refresh_token);
    let used_key = used_refresh_key(realm, &digest);
    cache
        .set_ex(
            &used_key,
            &user_id,
            Duration::from_secs(token_helper.refresh_ttl_secs()),
        )
        .await
        .map_err(map_cache_err)?;
    let grace = token_helper.grace_ttl_secs();
    if grace > 0 {
        cache
            .set_ex(
                &refresh_grace_key(realm, &digest),
                new_pair,
                Duration::from_secs(grace),
            )
            .await
            .map_err(map_cache_err)?;
    }
    Ok(())
}

pub async fn restore_refresh_token(
    cache: &Cache,
    token_helper: &TokenHelper,
    refresh_token: &str,
    user_id: &Id,
) -> Result<()> {
    let realm = token_helper.realm();
    cache
        .set_ex(
            &refresh_key(realm, refresh_token),
            &user_id.to_string(),
            Duration::from_secs(token_helper.refresh_ttl_secs()),
        )
        .await
        .map_err(map_cache_err)?;
    Ok(())
}

pub async fn abort_refresh_rotation(
    cache: &Cache,
    token_helper: &TokenHelper,
    user_id: Id,
    consumed_refresh_token: &str,
) -> Result<()> {
    let revoke_result = revoke_tokens(cache, token_helper, user_id).await;
    let restore_result =
        restore_refresh_token(cache, token_helper, consumed_refresh_token, &user_id).await;
    match (revoke_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(revoke_err), Ok(())) => Err(revoke_err),
        (Ok(()), Err(restore_err)) => Err(restore_err),
        (Err(revoke_err), Err(restore_err)) => {
            tracing::error!(
                revoke_error = %revoke_err,
                restore_error = %restore_err,
                %user_id,
                "both revoke and restore failed during abort_refresh_rotation"
            );
            Err(revoke_err)
        }
    }
}

pub async fn revoke_tokens(cache: &Cache, token_helper: &TokenHelper, user_id: Id) -> Result<()> {
    let realm = token_helper.realm();
    let user_refresh_key = subject_refresh_key(realm, user_id);

    let old_digest = cache
        .take::<String>(&user_refresh_key)
        .await
        .map_err(map_cache_err)?;
    let clear_old = del_refresh_by_digest(cache, realm, old_digest);
    let jti_key = access_jti_key(realm, user_id);
    let clear_jti = cache.del(&jti_key);
    let (r0, r1) = tokio::join!(clear_old, clear_jti);
    [r0, r1]
        .into_iter()
        .try_for_each(|r| r.map_err(map_cache_err))?;
    Ok(())
}

/// 完整 refresh 轮换：consume → issue → commit（含全部补偿）。
///
/// - 宽限期内重放旧 token：返回缓存的新 pair，不吊销。
/// - `issue` 失败：restore 旧 refresh，调用者可用旧 token 重试。
/// - `commit` 失败：abort（吊销新会话 + 还原旧 refresh）。
///
/// 方法层勿再手工编排这三段——补偿规则只在本函数内演进。
pub async fn rotate_refresh(
    cache: &Cache,
    token_helper: &TokenHelper,
    refresh_token: &str,
) -> Result<TokenPair> {
    let account_id = match consume_refresh_token(cache, token_helper, refresh_token).await? {
        ConsumeOutcome::GraceReplay(pair) => return Ok(pair),
        ConsumeOutcome::Fresh(id) => id,
    };

    let tokens = match issue_tokens(cache, token_helper, &account_id).await {
        Ok(tokens) => tokens,
        Err(err) => {
            if let Err(restore_err) =
                restore_refresh_token(cache, token_helper, refresh_token, &account_id).await
            {
                tracing::error!(
                    error = %restore_err,
                    "failed to restore refresh token after issue failure"
                );
            }
            return Err(err);
        }
    };

    if let Err(err) =
        commit_refresh_rotation(cache, token_helper, account_id, refresh_token, &tokens).await
    {
        tracing::error!(error = %err, "failed to commit refresh rotation");
        if let Err(abort_err) =
            abort_refresh_rotation(cache, token_helper, account_id, refresh_token).await
        {
            tracing::error!(
                error = %abort_err,
                "failed to abort refresh rotation after commit failure"
            );
        }
        return Err(err);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use authn_kit::access_jti_key;
    use jwt_kit::{TokenHelper, TokenRealm};
    use secrecy::SecretString;

    fn test_helper() -> TokenHelper {
        // grace=0：保留「重放即吊销」既有断言语义
        TokenHelper::new(
            TokenRealm::Account,
            SecretString::from("test-jwt-secret-at-least-32-bytes!!"),
            60,
            3600,
            0,
            "jsonrpc-template",
            "account",
        )
    }

    fn test_helper_with_grace(grace_secs: u64) -> TokenHelper {
        TokenHelper::new(
            TokenRealm::Account,
            SecretString::from("test-jwt-secret-at-least-32-bytes!!"),
            60,
            3600,
            grace_secs,
            "jsonrpc-template",
            "account",
        )
    }

    fn test_cache() -> Cache {
        Cache::new_memory()
    }

    async fn consume_fresh(cache: &Cache, helper: &TokenHelper, token: &str) -> Id {
        match consume_refresh_token(cache, helper, token).await.unwrap() {
            ConsumeOutcome::Fresh(id) => id,
            ConsumeOutcome::GraceReplay(_) => panic!("expected Fresh consume"),
        }
    }

    #[tokio::test]
    async fn revoke_drops_refresh_and_jti() {
        let cache = test_cache();
        let helper = test_helper();
        let user_id = Id::default();
        let tokens = issue_tokens(&cache, &helper, &user_id).await.unwrap();
        revoke_tokens(&cache, &helper, user_id).await.unwrap();
        let err = consume_refresh_token(&cache, &helper, &tokens.refresh_token)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("refresh_token_invalid"));
        assert!(
            cache
                .get::<String>(&access_jti_key(helper.realm(), user_id))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn restore_refresh_allows_retry_after_consume() {
        let cache = test_cache();
        let helper = test_helper();
        let user_id = Id::default();
        let tokens = issue_tokens(&cache, &helper, &user_id).await.unwrap();
        let consumed = consume_fresh(&cache, &helper, &tokens.refresh_token).await;
        restore_refresh_token(&cache, &helper, &tokens.refresh_token, &consumed)
            .await
            .unwrap();
        let again = consume_fresh(&cache, &helper, &tokens.refresh_token).await;
        assert_eq!(consumed, again);
    }

    #[tokio::test]
    async fn concurrent_refresh_only_one_succeeds() {
        let cache = test_cache();
        let helper = test_helper();
        let tokens = issue_tokens(&cache, &helper, &Id::default()).await.unwrap();
        let token = tokens.refresh_token.clone();
        let (r1, r2) = tokio::join!(
            consume_refresh_token(&cache, &helper, &token),
            consume_refresh_token(&cache, &helper, &token),
        );
        let ok_count = [r1.is_ok(), r2.is_ok()]
            .into_iter()
            .filter(|ok| *ok)
            .count();
        assert_eq!(ok_count, 1);
    }

    #[tokio::test]
    async fn refresh_reuse_revokes_rotated_session() {
        let cache = test_cache();
        let helper = test_helper();
        let user_id = Id::default();
        let tokens_a = issue_tokens(&cache, &helper, &user_id).await.unwrap();

        let consumed_id = consume_fresh(&cache, &helper, &tokens_a.refresh_token).await;
        let tokens_b = issue_tokens(&cache, &helper, &consumed_id).await.unwrap();
        commit_refresh_rotation(
            &cache,
            &helper,
            consumed_id,
            &tokens_a.refresh_token,
            &tokens_b,
        )
        .await
        .unwrap();

        let err = consume_refresh_token(&cache, &helper, &tokens_a.refresh_token)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("refresh_token_invalid"));

        let refresh_err = consume_refresh_token(&cache, &helper, &tokens_b.refresh_token)
            .await
            .unwrap_err();
        assert!(
            refresh_err.to_string().contains("refresh_token_invalid"),
            "replay should revoke the rotated session"
        );
    }

    #[tokio::test]
    async fn abort_after_commit_failure_restores_old_refresh() {
        let cache = test_cache();
        let helper = test_helper();
        let user_id = Id::default();
        let tokens_a = issue_tokens(&cache, &helper, &user_id).await.unwrap();

        let consumed = consume_fresh(&cache, &helper, &tokens_a.refresh_token).await;
        let tokens_b = issue_tokens(&cache, &helper, &consumed).await.unwrap();

        abort_refresh_rotation(&cache, &helper, consumed, &tokens_a.refresh_token)
            .await
            .unwrap();

        let revived = consume_fresh(&cache, &helper, &tokens_a.refresh_token).await;
        assert_eq!(revived, consumed);

        let new_dead = consume_refresh_token(&cache, &helper, &tokens_b.refresh_token)
            .await
            .unwrap_err();
        assert!(
            new_dead.to_string().contains("refresh_token_invalid"),
            "aborted new session must be revoked"
        );
    }

    #[tokio::test]
    async fn concurrent_refresh_does_not_revoke_winner() {
        let cache = test_cache();
        let helper = test_helper();
        let user_id = Id::default();
        let tokens = issue_tokens(&cache, &helper, &user_id).await.unwrap();
        let token = tokens.refresh_token.clone();

        let (r1, r2) = tokio::join!(
            consume_refresh_token(&cache, &helper, &token),
            consume_refresh_token(&cache, &helper, &token),
        );
        let winner = match (r1, r2) {
            (Ok(ConsumeOutcome::Fresh(id)), _) | (_, Ok(ConsumeOutcome::Fresh(id))) => id,
            (Ok(ConsumeOutcome::GraceReplay(_)), _) | (_, Ok(ConsumeOutcome::GraceReplay(_))) => {
                panic!("unexpected grace replay before commit")
            }
            (Err(e), Err(_)) => panic!("both concurrent consumes failed: {e}"),
        };

        let new_tokens = issue_tokens(&cache, &helper, &winner).await.unwrap();
        commit_refresh_rotation(&cache, &helper, winner, &token, &new_tokens)
            .await
            .unwrap();

        assert!(
            matches!(
                consume_refresh_token(&cache, &helper, &new_tokens.refresh_token).await,
                Ok(ConsumeOutcome::Fresh(_))
            ),
            "winner's freshly issued refresh must survive a concurrent duplicate"
        );
    }

    #[tokio::test]
    async fn rotate_refresh_marks_old_used_and_replay_revokes_rotated_session() {
        let cache = test_cache();
        let helper = test_helper();
        let user_id = Id::default();
        let tokens_a = issue_tokens(&cache, &helper, &user_id).await.unwrap();

        let tokens_b = rotate_refresh(&cache, &helper, tokens_a.refresh_token.as_str())
            .await
            .unwrap();
        assert!(
            matches!(
                consume_refresh_token(&cache, &helper, tokens_b.refresh_token.as_str()).await,
                Ok(ConsumeOutcome::Fresh(id)) if id == user_id
            ),
            "rotated pair must be usable"
        );

        // 旧 token 重放：命中 used_* → 吊销该账号全部会话（含刚轮换出的新会话）。
        let replay = consume_refresh_token(&cache, &helper, tokens_a.refresh_token.as_str())
            .await
            .unwrap_err();
        assert!(replay.to_string().contains("refresh_token_invalid"));

        let rotated_dead = consume_refresh_token(&cache, &helper, tokens_b.refresh_token.as_str())
            .await
            .unwrap_err();
        assert!(
            rotated_dead.to_string().contains("refresh_token_invalid"),
            "replay of the old token must revoke the rotated session"
        );
    }

    #[tokio::test]
    async fn rotate_refresh_replay_within_grace_returns_same_pair() {
        let cache = test_cache();
        let helper = test_helper_with_grace(60);
        let user_id = Id::default();
        let tokens_a = issue_tokens(&cache, &helper, &user_id).await.unwrap();

        let tokens_b = rotate_refresh(&cache, &helper, tokens_a.refresh_token.as_str())
            .await
            .unwrap();
        let replay = rotate_refresh(&cache, &helper, tokens_a.refresh_token.as_str())
            .await
            .unwrap();
        assert_eq!(replay, tokens_b);

        // 宽限过后（删 grace key）再重放 → 吊销
        cache
            .del(&refresh_grace_key(
                helper.realm(),
                &refresh_digest(&tokens_a.refresh_token),
            ))
            .await
            .unwrap();
        let after = consume_refresh_token(&cache, &helper, tokens_a.refresh_token.as_str())
            .await
            .unwrap_err();
        assert!(after.to_string().contains("refresh_token_invalid"));
        let rotated_dead = consume_refresh_token(&cache, &helper, tokens_b.refresh_token.as_str())
            .await
            .unwrap_err();
        assert!(rotated_dead.to_string().contains("refresh_token_invalid"));
    }
}
