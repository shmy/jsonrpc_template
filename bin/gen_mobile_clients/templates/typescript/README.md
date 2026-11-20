# TypeScript JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

零依赖（原生 `fetch`）。会话语义见根 `README.md`。

## 前置

- bun，或 Node ≥ 18（`AbortSignal.timeout`；可用 `--experimental-strip-types`）
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/typescript
bun main.ts
# 或
node --experimental-strip-types main.ts
```

成功输出含 `login ok` 与 `concurrent ok`；失败 exit code ≠ 0。

## 会话单测（mock fetch，无需 api）

```bash
cd .clients/typescript
bun session.test.ts
```

覆盖：单飞 refresh、仅 `-32004` 清会话、epoch 丢弃孤儿票、轮换后不再二次 refresh。

## 文件

| 文件 | 说明 |
|------|------|
| `jsonRpcClient.ts` | 生成客户端（勿手改） |
| `main.ts` | login + 无效 access 并发 refresh 冒烟 |
| `session.test.ts` | mock fetch 会话行为单测 |
