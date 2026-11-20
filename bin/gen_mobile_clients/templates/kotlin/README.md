# Kotlin JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

依赖：`kotlinx-serialization-json`（见 `build.gradle.kts`）。

## 前置

- JDK（17+；工程把字节码钉在 17，避免 JDK 25 与 Kotlin target 冲突）
- Gradle（`brew install gradle` 或本机已有）
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/kotlin
gradle -q run
```

成功输出：`login ok, expires_in=…`

## 文件

| 文件 | 说明 |
|------|------|
| `JsonRpcClient.kt` | 生成客户端（勿手改） |
| `Main.kt` | login 冒烟入口 |
| `build.gradle.kts` / `settings.gradle.kts` | 最小 Gradle 工程 |
