# Generated mobile JSON-RPC clients

由 `gen_mobile_clients` 从 OpenRPC JSON 生成（业务 crate 无改动）。目录已 gitignore。

## 重新生成

```bash
just gen-clients                 # curl openrpc → 生成（api 需在听，或用缓存）
cargo run -p gen_mobile_clients  # 仅生成（需已有 .clients/openrpc.json）
```

## 产物

| 路径 | 说明 |
|------|------|
| `openrpc.json` | OpenRPC 文档 |
| `swift/JsonRpcClient.swift` | `URLSession` + 会话 |
| `kotlin/JsonRpcClient.kt` | `HttpURLConnection` + 会话 |
| `dart/json_rpc_client.dart` | `dart:io` `HttpClient` + 会话（Flutter 非 Web） |
| `typescript/jsonRpcClient.ts` | `fetch` + 会话 |
| `swift/` `dart/` `typescript/` `kotlin/` | 各语言 README + 冒烟 |

默认 endpoint：`__SERVER__`  
Seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`（见各子目录 `README.md`）。

## 会话（各语言一致）

- `accessToken` + `refreshToken`：login / refresh 成功自动写入；logout 后清空
- 私有 RPC 遇 `-32020/-32021/-32022`：单飞 refresh 后重试一次；若 access 已被他人轮换则直接重试、不再 rotate
- **仅** `-32004` 清空会话并触发 `onSessionExpired`；限流 / 网络错误保留 token
- refresh 请求本身**不做**传输层自动重试（非幂等）
- 传输层失败：有限次短退避重试（业务 RPC；超时/断连类）。非幂等写接口仍可能重放，生产可再收紧
- `onTokensChanged` / `onSessionExpired`：可选回调（本客户端不负责持久化）
- 公开 `identity.account.refresh` / `refreshSession` 走同一单飞（已有 in-flight 则加入，**不**覆盖进行中的 refreshToken）
- `sessionEpoch`：`clearTokens` / logout 递增；在途 refresh 成功后若 epoch 已变则**不**写回本地 token（防假登录）

### 孤儿票（可接受）

在途 refresh 已在服务端 commit 新票，但客户端因本地 logout/`clearTokens` 丢弃结果时，服务端新 refresh 会成为孤儿，直到 TTL / 下次 login 覆盖。客户端**不会**为此自动再打 logout；接入方可在 `onSessionExpired` 后清本地持久化即可。

## 注意

- `x-auth: required` 且无 refresh 时本地抛 auth required。
- update avatar 三态（省略 / null / 字符串）未建模 omit。
- 非幂等写 RPC 仍可能被传输层短重试重放；生产可按 method 收紧。
- 冒烟脚手架在 `bin/gen_mobile_clients/templates/`；清 `.clients` 后重新生成即可补回。
- 会话不变量另有 `cargo test -p gen_mobile_clients`（模板字符串断言）；行为回归靠各语言冒烟。
