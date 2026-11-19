/// Application-defined JSON-RPC codes for identity (`-32001..-32009`，跳过 `-32007`)。
///
/// 会话基础设施错误（token 缺失/无效/吊销、缓存不可用）不在此处，
/// 归 `http_auth` 的 `-32020..-32029` 号段（`authn_kit` 不感知 RPC）。
/// `-32007` / `-32010` 为协议核（jsonrpsee 对齐）保留，勿占用。
pub mod codes {
    pub const ACCOUNT_DUPLICATED: i32 = -32001;
    pub const NOT_FOUND: i32 = -32002;
    pub const INVALID_CREDENTIALS: i32 = -32003;
    pub const REFRESH_TOKEN_INVALID: i32 = -32004;
    pub const LOGIN_RATE_LIMITED: i32 = -32005;
    /// 与 `-32005` 同码：客户端按同一条「auth 限流」分支处理，仅文案区分 login/refresh。
    pub const AUTH_RATE_LIMITED: i32 = -32005;
    pub const PASSWORD_UNCHANGED: i32 = -32006;
    /// 原密码不正确（改密）；勿占用协议 `-32007` / authn `-32020..`。
    pub const INCORRECT_ORIGINAL_PASSWORD: i32 = -32008;
    pub const ACCOUNT_FORBIDDEN: i32 = -32009;
}

/// Domain-facing identity errors.
///
/// [`Display`](std::fmt::Display) / [`l10n_key`](Self::l10n_key) is the Fluent key.
/// Feature crates register FTL + an RPC domain mapper; do not wrap infra errors here.
///
/// Prefer `Err(IdentityError::….into())` so the feature domain mapper can localize.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("account_not_found")]
    AccountNotFound,
    #[error("account_duplicated")]
    AccountDuplicated,
    #[error("account_invalid_credentials")]
    AccountInvalidCredentials,
    #[error("refresh_token_invalid")]
    RefreshTokenInvalid,
    #[error("login_rate_limited")]
    LoginRateLimited,
    #[error("auth_rate_limited")]
    AuthRateLimited,
    #[error("account_password_unchanged")]
    AccountPasswordUnchanged,
    #[error("account_forbidden")]
    AccountForbidden,
    #[error("incorrect_original_password")]
    IncorrectOriginalPassword,
}

impl IdentityError {
    #[must_use]
    pub const fn rpc_code(&self) -> i32 {
        match self {
            Self::AccountNotFound => codes::NOT_FOUND,
            Self::AccountDuplicated => codes::ACCOUNT_DUPLICATED,
            Self::AccountInvalidCredentials => codes::INVALID_CREDENTIALS,
            Self::RefreshTokenInvalid => codes::REFRESH_TOKEN_INVALID,
            Self::LoginRateLimited => codes::LOGIN_RATE_LIMITED,
            Self::AuthRateLimited => codes::AUTH_RATE_LIMITED,
            Self::AccountPasswordUnchanged => codes::PASSWORD_UNCHANGED,
            Self::AccountForbidden => codes::ACCOUNT_FORBIDDEN,
            Self::IncorrectOriginalPassword => codes::INCORRECT_ORIGINAL_PASSWORD,
        }
    }

    /// Fluent key; kept in sync with `#[error(...)]` (see unit test).
    #[must_use]
    pub const fn l10n_key(&self) -> &'static str {
        match self {
            Self::AccountNotFound => "account_not_found",
            Self::AccountDuplicated => "account_duplicated",
            Self::AccountInvalidCredentials => "account_invalid_credentials",
            Self::RefreshTokenInvalid => "refresh_token_invalid",
            Self::LoginRateLimited => "login_rate_limited",
            Self::AuthRateLimited => "auth_rate_limited",
            Self::AccountPasswordUnchanged => "account_password_unchanged",
            Self::AccountForbidden => "account_forbidden",
            Self::IncorrectOriginalPassword => "incorrect_original_password",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l10n_key_matches_display() {
        let cases = [
            IdentityError::AccountNotFound,
            IdentityError::AccountDuplicated,
            IdentityError::AccountInvalidCredentials,
            IdentityError::RefreshTokenInvalid,
            IdentityError::LoginRateLimited,
            IdentityError::AuthRateLimited,
            IdentityError::AccountPasswordUnchanged,
            IdentityError::AccountForbidden,
            IdentityError::IncorrectOriginalPassword,
        ];
        for err in cases {
            assert_eq!(err.l10n_key(), err.to_string());
        }
    }

    #[test]
    fn incorrect_original_password_stays_in_identity_band() {
        let code = IdentityError::IncorrectOriginalPassword.rpc_code();
        assert_eq!(code, codes::INCORRECT_ORIGINAL_PASSWORD);
        assert!((-32009..=-32001).contains(&code));
        // 协议核保留 -32007（body 过大），业务码不得占用
        assert_ne!(code, -32007);
    }

    #[test]
    fn identity_codes_skip_protocol_reserved() {
        for code in [
            codes::ACCOUNT_DUPLICATED,
            codes::NOT_FOUND,
            codes::INVALID_CREDENTIALS,
            codes::REFRESH_TOKEN_INVALID,
            codes::LOGIN_RATE_LIMITED,
            codes::AUTH_RATE_LIMITED,
            codes::PASSWORD_UNCHANGED,
            codes::INCORRECT_ORIGINAL_PASSWORD,
            codes::ACCOUNT_FORBIDDEN,
        ] {
            assert_ne!(code, -32007, "{code} must not reuse OVERSIZED_REQUEST");
            assert_ne!(code, -32010, "{code} must not reuse TOO_BIG_BATCH_REQUEST");
            assert!(
                !(-32029..=-32020).contains(&code),
                "{code} must not land in authn band"
            );
        }
    }

    #[test]
    fn auth_rate_limited_shares_code_with_login() {
        // Same wire code, distinct message: clients branch once on -32005.
        assert_eq!(
            IdentityError::AuthRateLimited.rpc_code(),
            IdentityError::LoginRateLimited.rpc_code()
        );
    }
}
