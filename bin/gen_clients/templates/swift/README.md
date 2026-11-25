# Swift JSON-RPC client

由 `gen_clients` 生成。默认 endpoint：`__SERVER__`。会话语义见根 `README.md`。

## 前置

- macOS + Xcode / Swift toolchain
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/swift
swiftc main.swift JsonRpcClient.swift -o ./smoke && ./smoke
```

## 文件

| 文件 | 说明 |
|------|------|
| `JsonRpcClient.swift` | 生成客户端（勿手改） |
| `main.swift` | login + 并发 refresh 冒烟 |
