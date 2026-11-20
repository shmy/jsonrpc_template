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
| `swift/JsonRpcClient.swift` | `URLSession` + `Codable` |
| `kotlin/JsonRpcClient.kt` | `HttpURLConnection` + kotlinx.serialization |
| `dart/json_rpc_client.dart` | `dart:io` `HttpClient`（Flutter 非 Web） |
| `typescript/jsonRpcClient.ts` | 原生 `fetch` |
| `swift/` `dart/` `typescript/` `kotlin/` | 各语言 README + login 冒烟 |

默认 endpoint：`__SERVER__`  
Seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`（见各子目录 `README.md`）。

## 注意

- `x-auth: required` 缺 `accessToken` 时本地抛错。
- update avatar 三态（省略 / null / 字符串）未建模 omit。
- 冒烟脚手架在 `bin/gen_mobile_clients/templates/`；清 `.clients` 后重新生成即可补回。
