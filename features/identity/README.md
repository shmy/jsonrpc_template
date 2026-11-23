# identity

完整会话 / 账号 / 头像附件示范 feature。

**不要照抄本 crate 的 `Cargo.toml`。** 这里额外拉了会话栈（`jwt_kit` / `authn_kit` / `cache_kit` / `config` / `argon2`）和附件（`file_attachment` / `blob_kit`）。叶子业务通常不需要这些。

新建 feature 请抄写 [`docs/feature-template/`](../../docs/feature-template/)，并对照仓库根 `AGENTS.md`「扩展业务 Feature」。
