# TypeScript JSON-RPC client

由 `gen_clients` 生成。默认 endpoint：`__SERVER__`。

零依赖（原生 `fetch`）。会话语义见根 `README.md`。

## 前置

- bun，或 Node ≥ 18（`AbortSignal.timeout`）
- biz 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟 / 单测

```bash
cd .clients/typescript
bun main.ts            # 需 biz
bun session.test.ts    # mock fetch，无需 biz
```

## 文件

| 文件 | 说明 |
|------|------|
| `jsonRpcClient.ts` | 生成客户端（勿手改） |
| `main.ts` | login + 并发 refresh 冒烟 |
| `session.test.ts` | 会话行为单测 |
