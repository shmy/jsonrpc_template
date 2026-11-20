package com.example.jsonrpc.generated

fun main() {
    val client = JsonRpcClient(acceptLanguage = "zh")
    try {
        val tokens = client.identityAccountLogin(
            IdentityAccountLoginParams(password = "__SEED_PASSWORD__", phone = "__SEED_PHONE__"),
        )
        client.accessToken = tokens.access_token
        println("login ok, expires_in=${tokens.expires_in}")
    } catch (e: Exception) {
        println("login failed: $e")
    }
}
