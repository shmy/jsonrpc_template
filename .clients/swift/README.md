# Swift JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`http://127.0.0.1:19001/jsonrpc`。会话语义见根 `README.md`。

## 前置

- macOS + Xcode / Swift toolchain
- api 在听（seed：`13888888888` / `admin123!`）

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
