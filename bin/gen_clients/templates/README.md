# Generated JSON-RPC clients

由 `gen_clients` 从 OpenRPC JSON 生成；目录入库，CI / `just clients-check` 锁漂移。

```bash
just gen-clients                 # curl openrpc → 生成（api 需在听，或用缓存）
just clients-check               # 再生成 + git diff + bun session.test.ts
cargo run -p gen_clients         # 仅生成（需已有 .clients/openrpc.json）
```

| 路径 | 说明 |
|------|------|
| `openrpc.json` | OpenRPC 文档 |
| `swift/` `kotlin/` `dart/` `typescript/` | 客户端 + README + 冒烟 |

默认 endpoint：`__SERVER__`；seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`。

## 会话

- login / refresh 成功写入 `accessToken` + `refreshToken`；logout / `-32004` 清空
- 私有 RPC 遇 `-32020/-32021/-32022`：单飞 refresh 后重试；access 已轮换则直接重试
- refresh **不做**传输层重试；业务 RPC 传输失败有限次短退避（非幂等写可能重放）
- `refreshSession` 与自动 refresh 共用单飞；已有 in-flight 则加入
- `sessionEpoch`：`clearTokens` 递增；在途 refresh 若 epoch 已变则不写回（防假登录；服务端新票可能成孤儿直至 TTL）
- 不负责持久化；接入方自行读写 token 字段

## 其它

- 无 refresh 时本地抛 auth required（`x-auth: required`）
- avatar 三态 omit 未建模
- 冒烟在 `templates/`；会话不变量：`cargo test -p gen_clients`；TS 行为：`bun session.test.ts`
