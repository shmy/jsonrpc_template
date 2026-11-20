package com.example.jsonrpc.generated

import java.util.concurrent.Callable
import java.util.concurrent.Executors

fun main() {
    val client = JsonRpcClient(acceptLanguage = "zh")
    try {
        client.identityAccountLogin(
            IdentityAccountLoginParams(password = "admin123!", phone = "13888888888"),
        )
        println("login ok, access=${client.accessToken != null} refresh=${client.refreshToken != null}")

        // 无效 access → -32021 → 单飞 refresh → 重试
        client.accessToken = "invalid-access-token"
        val pool = Executors.newFixedThreadPool(2)
        try {
            val tasks = listOf(
                Callable {
                    client.identityAccountSearch(
                        IdentityAccountSearchParams(cursor = null, limit = 1, name = null, phone = "13888888888"),
                    )
                },
                Callable {
                    client.identityAccountSearch(
                        IdentityAccountSearchParams(cursor = null, limit = 1, name = null, phone = "13888888888"),
                    )
                },
            )
            val results = pool.invokeAll(tasks).map { it.get() }
            println("concurrent ok, items=${results.first().items.size}")
        } finally {
            pool.shutdownNow()
        }
    } catch (e: Exception) {
        System.err.println("smoke failed: $e")
        kotlin.system.exitProcess(1)
    }
}
