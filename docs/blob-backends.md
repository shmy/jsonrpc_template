# Blob 后端 Feature 对齐表

`api` 的 `blob-fs` / `blob-cos` 是组合根 meta-feature；下游 crate 必须同步打开对应 feature，否则会出现「blob provide 了 fs、file 路由却按 cos 编」之类的错配。

| 打开（二选一） | `api` | `infra/blob` | `features/file` | `infra/app_state` | `infra/config` |
|----------------|-------|--------------|-----------------|-------------------|----------------|
| 本地 FS（默认） | `blob-fs` | `fs` | `blob-fs` | `blob-fs` | `fs` |
| 腾讯 COS | `blob-cos` | `cos` | `blob-cos` | `blob-cos` | `cos` |

转发链（已在 `Cargo.toml` 写好，一般只改 `api`）：

```text
api/blob-fs  → blob/fs + file/blob-fs
               blob/fs → config/fs + blob_kit/fs
               file/blob-fs → app_state/blob-fs + config/fs + blob_kit/fs

api/blob-cos → blob/cos + file/blob-cos
               （对称）
```

规则：

- **日常只切 `api` feature**，勿单独 `cargo check -p file --features blob-cos` 又立刻 `cargo check -p api --features blob-fs` 来回切（会触发 feature 统一化补建）。
- COS 优先：`infra/blob` 在同时启用时以 `cos` 为准。
- e2e / 本地快速启动：`cargo run -p api --no-default-features --features blob-fs`（关 otlp）。

## 威胁模型（公开可读）

fs 下 `GET /files/{*path}`（及可选 `?s=`）对符合 dated 白名单的对象**匿名可读**，含 `staging/…`：无 claim ACL，安全依赖高熵 path（能力型 URL）。转正只是去掉 `staging/` 前缀——知 staging URL 即可推断正式 URL。

适合头像等「泄露 URL ≈ 可读」的场景。**私密附件勿默认走同一管道**；需要机密性时另做鉴权 GET（或短 TTL 签名读），勿把「世界可读 + 高熵」当成私有存储。

COS 不挂同进程回源守卫；公网可读性取决于桶 ACL 与 `BLOB_S3_DOMAIN`，运维配错会比 fs 守卫更宽松。

### 脏绝对 URL / GC 归一

Attachment in-use 与删号 detach 会经 `Blob::strip_public_url_prefix` 归一。本站匹配规则：

- 前缀须与 `BLOB_FS_DOMAIN` / `BLOB_S3_DOMAIN` 一致（含 path，如 `/files`）；仅 **http↔https** 可互换
- host 大小写、端口、URL-encode 差异**不会**自动对齐——脏数据应与配置 domain 同形
- 本站 URL / 相对 key 会去掉 `?`/`#`；**真外链保留 query**（签名 CDN）
