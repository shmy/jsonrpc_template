# Dart JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

HTTP：`dart:io` `HttpClient`（零 pub 依赖；面向 Flutter iOS / Android / desktop，**不含 Web**）。

## 前置

- Dart SDK ≥ 3.3（或 Flutter SDK）
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/dart
dart run main.dart
```

成功输出：`login ok, expires_in=…`

拷进 Flutter 工程时直接引用 `json_rpc_client.dart` 即可，无需再加 `http` 包。

## 文件

| 文件 | 说明 |
|------|------|
| `json_rpc_client.dart` | 生成客户端（勿手改） |
| `main.dart` | login 冒烟入口 |
| `pubspec.yaml` | 仅 smoke 用；无第三方依赖 |
