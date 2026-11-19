use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use argon2::{Algorithm, Argon2, Params, Version};
use rootcause::Result;
use std::fmt::{self, Debug};
use std::sync::LazyLock;
use tokio::task::spawn_blocking;

/// 显式固定 argon2id 成本（m=19MiB, t=2, p=1，OWASP 推荐基线）。
/// 不用 `Params::default()`，避免 argon2 crate 升级时静默改变哈希成本。
#[allow(clippy::expect_used)] // 字面量在合法范围；失败只可能是 crate API 破坏
static ARGON2: LazyLock<Argon2> = LazyLock::new(|| {
    let params = Params::new(19 * 1024, 2, 1, None)
        .expect("OWASP argon2id baseline params (m=19MiB,t=2,p=1) must be accepted");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
});

/// 哈希后的密码（PHC 字符串）。实现细节不进 `*_contract`（契约层保持薄模型）。
#[derive(Clone)]
pub struct HashedPassword(String);

impl HashedPassword {
    pub fn new_unchecked(hash: String) -> Self {
        Self(hash)
    }

    /// 同步哈希。仅用于测试或已在 `spawn_blocking` 内的路径；请求处理请用 [`Self::try_new`]。
    pub fn try_new_sync(password: &str) -> Result<Self> {
        let hash = (*ARGON2).hash_password(password.as_bytes())?;
        Ok(Self(hash.to_string()))
    }

    /// argon2 哈希，在 blocking 线程池执行，避免阻塞 tokio worker。
    pub async fn try_new(password: &str) -> Result<Self> {
        let password = password.to_string();
        spawn_blocking(move || Self::try_new_sync(&password)).await?
    }

    pub fn verify_sync(&self, password: &str) -> Result<()> {
        let parsed_hash = PasswordHash::new(&self.0)?;
        (*ARGON2).verify_password(password.as_bytes(), &parsed_hash)?;
        Ok(())
    }

    pub async fn verify(&self, password: &str) -> Result<()> {
        let hashed = self.clone();
        let password = password.to_string();
        spawn_blocking(move || hashed.verify_sync(&password)).await?
    }

    /// 取出底层 PHC 字符串（存库用），避免 Deref 意外泄露到字符串上下文。
    pub fn into_string(self) -> String {
        self.0
    }

    /// 登录时手机号不存在时跑一次等价 argon2，抹平与「密码错误」路径的耗时差异。
    pub async fn equalize_timing() {
        const DUMMY: &[u8] = b"timing-equalizer-dummy-password";
        let result = spawn_blocking(move || (*ARGON2).hash_password(DUMMY)).await;
        if let Err(err) = result {
            tracing::error!(error = %err, "login timing equalizer hash failed");
        }
    }
}

impl Debug for HashedPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HashedPassword")
            .field(&"<Redacted>")
            .finish()
    }
}
