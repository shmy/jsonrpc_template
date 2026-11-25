package com.example.jsonrpc.generated

import java.util.concurrent.Callable
import java.util.concurrent.Executors

fun main() {
    val client = JsonRpcClient(acceptLanguage = "zh")
    try {
        client.identityAccountLogin(
            IdentityAccountLoginParams(password = "__SEED_PASSWORD__", phone = "__SEED_PHONE__"),
        )
        println("login ok, access=${client.accessToken != null} refresh=${client.refreshToken != null}")

        // 无效 access → -32021 → 单飞 refresh → 重试
        client.accessToken = "invalid-access-token"
        val pool = Executors.newFixedThreadPool(2)
        try {
            val tasks = listOf(
                Callable {
                    client.identityAccountSearch(
                        IdentityAccountSearchParams(cursor = null, limit = 1, name = null, phone = "__SEED_PHONE__"),
                    )
                },
                Callable {
                    client.identityAccountSearch(
                        IdentityAccountSearchParams(cursor = null, limit = 1, name = null, phone = "__SEED_PHONE__"),
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
