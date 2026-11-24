# Blob 后端 Feature 对齐表

`app` 的 `blob-fs` / `blob-cos` 是组合根 meta-feature；下游 crate 必须同步打开对应 feature，否则会出现「blob provide 了 fs、file 路由却按 cos 编」之类的错配。

| 打开（二选一） | `app` | `infra/blob` | `app/file` | `infra/app_state` | `infra/config` |
|----------------|-------|--------------|------------|-------------------|----------------|
| 本地 FS（默认） | `blob-fs` | `fs` | `blob-fs` | `blob-fs` | `fs` |
| 腾讯 COS | `blob-cos` | `cos` | `blob-cos` | `blob-cos` | `cos` |

转发链（已在 `Cargo.toml` 写好，一般只改 `app`）：

```text
app/blob-fs  → blob/fs + file/blob-fs
               blob/fs → config/fs + blob_kit/fs
               file/blob-fs → app_state/blob-fs + config/fs + blob_kit/fs

app/blob-cos → blob/cos + file/blob-cos
               （对称）
```

规则：

- **日常只切 `app` feature**，勿单独 `cargo check -p file --features blob-cos` 又立刻 `cargo check -p app --features blob-fs` 来回切（会触发 feature 统一化补建）。
- COS 优先：`infra/blob` 在同时启用时以 `cos` 为准。
- e2e / 本地快速启动：`cargo run -p app --no-default-features --features blob-fs`（关 otlp）。

## 威胁模型（公开可读）

fs 下 `GET /files/{*path}`（及可选 `?s=`）对符合 dated 白名单的对象**匿名可读**，含 `staging/…`：无 claim ACL，安全依赖高熵 path（能力型 URL）。转正只是去掉 `staging/` 前缀——知 staging URL 即可推断正式 URL。

生产若不能接受「持 staging URL ≈ 持正式 URL」：

- 用 COS + 私有读 + 短时签名；或
- 给正式对象另发能力型 token / 独立 ACL；或
- 接受能力型 URL 模型并保证 path 熵与 TLS。

`?s=` 仅正式对象；`staging/` 禁止派生。缩略图落盘旁路缓存（`*_s{n}.webp`）；purge/GC 会清派生文件。
