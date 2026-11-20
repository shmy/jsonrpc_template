# TypeScript JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

零依赖（原生 `fetch`）。

## 前置

- bun，或 Node ≥ 18（建议 22+，可用 `--experimental-strip-types`）
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/typescript
bun main.ts
# 或
node --experimental-strip-types main.ts
```

成功输出：`login ok, expires_in=…`

## 文件

| 文件 | 说明 |
|------|------|
| `jsonRpcClient.ts` | 生成客户端（勿手改） |
| `main.ts` | login 冒烟入口 |
