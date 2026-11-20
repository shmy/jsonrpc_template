# Dart JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

## 前置

- Dart SDK ≥ 3.3
- api 在听（seed：`13888888888` / `admin123!`）

## 冒烟

```bash
cd .clients/dart
dart pub get
dart run main.dart
```

成功输出：`login ok, expires_in=…`

## 文件

| 文件 | 说明 |
|------|------|
| `json_rpc_client.dart` | 生成客户端（勿手改） |
| `main.dart` | login 冒烟入口 |
| `pubspec.yaml` | 依赖 `http` |
