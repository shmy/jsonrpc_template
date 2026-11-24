//! 对象级鉴权（在 `PrivateRpc`「已登录」门槛之上）。
//!
//! - [`load_for_mutate`]：本人或调用者 `privileged` → update
//! - [`load_for_delete`]：同上，且目标不得为 `privileged` → delete
//!
//! 新 RPC 若按 id 改删资源，应复用本模块；勿只依赖漏写 `AuthedAccount`。

use account_id::Id;
use identity_contract::{Account, IdentityError};
use rootcause::Result;
use toasty::Executor;

/// 改删策略：本人，或调用者 `privileged`。
#[must_use]
pub(crate) fn can_mutate(caller: Id, target: Id, caller_privileged: bool) -> bool {
    caller == target || caller_privileged
}

/// 校验调用者可对 `target` 做改删；不加载目标行。
pub(crate) async fn require_self_or_privileged(
    executor: &mut dyn Executor,
    caller: Id,
    target: Id,
) -> Result<()> {
    if caller == target {
        return Ok(());
    }
    let caller_account = Account::require_by_id(executor, caller).await?;
    if can_mutate(caller, target, caller_account.privileged) {
        Ok(())
    } else {
        Err(IdentityError::AccountForbidden.into())
    }
}

/// 鉴权通过后加载目标账号（供 update）。
pub(crate) async fn load_for_mutate(
    executor: &mut dyn Executor,
    caller: Id,
    target: Id,
) -> Result<Account> {
    require_self_or_privileged(executor, caller, target).await?;
    Account::require_by_id(executor, target).await
}

/// 鉴权通过且目标非 `privileged`（供 delete；特权账号禁删）。
pub(crate) async fn load_for_delete(
    executor: &mut dyn Executor,
    caller: Id,
    target: Id,
) -> Result<Account> {
    let account = load_for_mutate(executor, caller, target).await?;
    if account.privileged {
        return Err(IdentityError::AccountForbidden.into());
    }
    Ok(account)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn can_mutate_self() {
        let id = Id::new_unchecked(7);
        assert!(can_mutate(id, id, false));
    }

    #[test]
    fn can_mutate_other_requires_privileged() {
        let caller = Id::new_unchecked(1);
        let target = Id::new_unchecked(2);
        assert!(!can_mutate(caller, target, false));
        assert!(can_mutate(caller, target, true));
    }
}
