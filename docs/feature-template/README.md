# 最小 Feature 骨架

**不要照抄 `features/identity` 的 `Cargo.toml`。** identity 额外拉了会话栈（`jwt_kit` / `authn_kit` / `cache_kit` / `config` / `argon2`）和附件（`file_attachment` / `blob_kit`），叶子业务通常不需要。

本目录是**非 workspace 成员**的抄写模板；复制到 `features/<name>/` 后按清单接线。

## 默认依赖（叶子 feature）

```toml
[dependencies]
app_state = { workspace = true }
http_routing = { workspace = true }
axum_jsonrpc = { workspace = true }
locale = { workspace = true }
# 有落库 / 跨 feature 共享类型时再加：
# <name>_contract = { workspace = true }
# 需要 caller id：
# http_auth = { workspace = true }
# 需要 ClientIp：
# http_extractors = { workspace = true }
```

## 接线清单

1. 建 `features/<name>/`（可复制本目录文件）与可选 `features/<name>_contract/`
2. `*_contract` 有模型 → `infra/persistence` 的 `toasty::models!(…)` 注册 → `just migration-generate`
3. `bin/api/main.rs`：`.module(<name>::module())`，且排在 `http_server` **之前**
4. 有 FTL / domain mapper → 在 `module()` **同步段**注册（seal 后会 panic 或静默 `-32603`）。`just layering` 会扫描 `register_ftl` / `register_domain_mapper` 并校验顺序（见 `scripts/check_module_order.py`）
5. RPC：默认进 `PrivateRpc`；仅匿名方法进 `PublicRpc`
6. 有附件列 → `file_attachment::bind` + `register_attachment_in_use`（module 排在 `file` 前）
7. `just layering` 应通过；`cargo check -p <name>` 或 `just check-fast`

## 目录约定

见仓库根 `AGENTS.md`「扩展业务 Feature」。对照实现：`features/probe`（仅 HTTP）、`features/file`（RPC + 可选 HTTP）、`features/identity`（完整示范，勿抄依赖图）。

本模板已含 `shared/l10n.rs` + `locales/{en-US,zh-CN}/example.ftl` 最小样例：有领域错误时填 key 并取消 `l10n.rs` 内注释；校验文案跟 feature 走，勿塞进 `infra/locale` 的 `shared.ftl`。
