# Blob 后端 Feature 对齐表

`biz` 的 `blob-fs` / `blob-cos` 是组合根 meta-feature；下游 crate 必须同步打开对应 feature，否则会出现「blob provide 了 fs、file 路由却按 cos 编」之类的错配。

| 打开（二选一） | `biz` | `infra/blob` | `biz/file` | `infra/app_state` | `infra/file_attachment` | `infra/config` |
|----------------|-------|--------------|------------|-------------------|-------------------------|----------------|
| 本地 FS（默认） | `blob-fs` | `fs` | `blob-fs` | `blob-fs` | `fs` | `fs` |
| 腾讯 COS | `blob-cos` | `cos` | `blob-cos` | `blob-cos` | `cos` | `cos` |

转发链（已在 `Cargo.toml` 写好，一般只改 `biz`）：

```text
biz/blob-fs  → blob/fs + file/blob-fs
               blob/fs → config/fs + blob_kit/fs
               file/blob-fs → app_state/blob-fs + file_attachment/fs + config/fs + blob_kit/fs

biz/blob-cos → blob/cos + file/blob-cos
               blob/cos → config/cos + blob_kit/cos
               file/blob-cos → app_state/blob-cos + file_attachment/cos + config/cos + blob_kit/cos
```

**编译期互斥**：workspace 对 `app_state` / `file_attachment` / `identity` / `http_server` / `http_auth` / `http_routing` / `probe` 使用 `default-features = false`。因此 `cargo check -p biz --no-default-features --features blob-cos` **不会**再经这些 crate 的默认 feature 拉进 `blob_kit/fs`。叶子单独 check（如 `just check-fast identity`）仍走各包自己的 `default = ["blob-fs"]`。

`--all-features`（clippy CI）会同时打开 fs 与 cos，属有意并集，不代表生产二进制。

规则：

- **日常只切 `biz` feature**，勿单独 `cargo check -p file --features blob-cos` 又立刻 `cargo check -p biz --features blob-fs` 来回切（会触发 feature 统一化补建）。
- COS 优先：`infra/blob` 在同时启用时以 `cos` 为准（`--all-features` / 误同时打开时）。
- e2e / 本地快速启动：`cargo run -p biz --no-default-features --features blob-fs`（关 otlp）。
- 校验 cos 不拉 fs：`cargo tree -p biz --no-default-features --features blob-cos -e features -i blob_kit`（不应出现 `blob_kit/fs`）。

## 威胁模型（公开可读）

fs 下 `GET /files/{*path}`（及可选 `?s=`）对符合 dated 白名单的对象**匿名可读**，含 `staging/…`：无 claim ACL，安全依赖高熵 path（能力型 URL）。转正只是去掉 `staging/` 前缀——知 staging URL 即可推断正式 URL。

生产若不能接受「持 staging URL ≈ 持正式 URL」：

- 用 COS + 私有读 + 短时签名；或
- 给正式对象另发能力型 token / 独立 ACL；或
- 接受能力型 URL 模型并保证 path 熵与 TLS。

`?s=` 仅正式对象；`staging/` 禁止派生。缩略图落盘旁路缓存（`*_s{n}.webp`）；purge/GC 会清派生文件。

## 多实例（fs）

`blob-fs` 把对象与 `?s=` 派生缓存写在**本机目录**（`BLOB_FS_ROOT`）。多实例必须共享同一卷（NFS / PVC 等），且仍只开一个 `MASTER=true` 跑 staging/attachment GC；否则各节点各清各的盘、URL 也可能指到没有对象的实例。COS 无此约束。
