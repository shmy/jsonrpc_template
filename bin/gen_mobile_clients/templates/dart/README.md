# Dart JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

HTTP：`dart:io` `HttpClient`（零 pub 依赖；Flutter iOS / Android / desktop，**不含 Web**）。

## 会话

- `accessToken` + `refreshToken`：login / refresh 成功后自动写入；logout 后清空
- 私有 RPC 遇 `-32020/-32021/-32022`：单飞 refresh 后重试一次；access 已轮换则直接重试
- **仅** `-32004` → `clearTokens(sessionExpired: true)` + `JsonRpcSessionExpiredException`
- refresh 本身不做网络重试；业务传输错误最多再试 2 次
- `onTokensChanged` / `onSessionExpired`：可选；不负责持久化

## 前置

- Dart SDK ≥ 3.3（或 Flutter SDK）
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/dart
dart run main.dart
```

成功输出含 `login ok` 与 `concurrent ok`；失败 exit code ≠ 0。

## 文件

| 文件 | 说明 |
|------|------|
| `json_rpc_client.dart` | 生成客户端（勿手改） |
| `main.dart` | login + 无效 access 并发 refresh 冒烟 |
| `pubspec.yaml` | 仅 smoke 用；无第三方依赖 |
