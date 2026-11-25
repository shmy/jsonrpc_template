# Kotlin JSON-RPC client

由 `gen_mobile_clients` 生成。默认 endpoint：`__SERVER__`。

依赖：`kotlinx-serialization-json`。会话语义见根 `README.md`。阻塞 HTTP：勿在 Android 主线程调用。

## 前置

- JDK 17+（工程钉 JVM 17 字节码，避免 JDK 25 与 Kotlin target 冲突）
- Gradle
- api 在听（seed：`__SEED_PHONE__` / `__SEED_PASSWORD__`）

## 冒烟

```bash
cd .clients/kotlin
gradle -q run
```

## 文件

| 文件 | 说明 |
|------|------|
| `JsonRpcClient.kt` | 生成客户端（勿手改） |
| `Main.kt` | login + 并发 refresh 冒烟 |
| `build.gradle.kts` / `settings.gradle.kts` | 最小 Gradle 工程 |
