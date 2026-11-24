# 最小 Feature 骨架

**不要照抄 `features/identity` 的 `Cargo.toml`。** identity 额外拉了会话栈与附件；叶子业务通常不需要。

本目录是**非 workspace 成员**的抄写模板。优先：`just new-feature <name>`（自动复制并替换占位符）；或手动复制到 `features/<name>/`。

## Checklist（接线顺序）

- [ ] `just new-feature <name>`（或复制本目录）
- [ ] 需要落库 / 跨 feature 共享类型 → 建 `features/<name>_contract/`，并在 `infra/persistence` `toasty::models!(…)` 注册 → `just migration-generate`
- [ ] 根 `Cargo.toml`：`workspace.members` + `[workspace.dependencies]`；`bin/api` 依赖该 crate
- [ ] `bin/api/main.rs`：`.module(<name>::module())`，**必须在 `http_server` 之前**（顺序由 `just layering` → `check_module_order.py` 拦，不必死记）
- [ ] 有领域错误 → `module()` 同步段用 `locale::register_locale_bundle`（FTL + mapper **成对**；漏 mapper → `-32603`；seal 后再注册会 panic）
- [ ] RPC：**默认 `PrivateRpc`**；仅 login/refresh 等匿名方法进 `PublicRpc`
- [ ] 附件列 → `file_attachment::bind` + `register_attachment_in_use`（module 排在 `file` 前）
- [ ] `just layering` + `just check-fast <name>`

authn FTL/mapper 由 `http_server` 统一注册，feature **不要**再注册。

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

## 目录约定

见仓库根 [`AGENTS.md`](../../AGENTS.md)「扩展业务 Feature」。对照：`features/probe`（仅 HTTP）、`features/file`（RPC + 可选 HTTP）、`features/identity`（完整示范，勿抄依赖图）。

本模板已含 `shared/l10n.rs` + `locales/{en-US,zh-CN}/example.ftl`：有领域错误时填 key 并取消 `l10n.rs` 内注释；校验文案跟 feature 走，勿塞进 `infra/locale` 的 `shared.ftl`。
