    companion object {
        private const val MAX_NETWORK_RETRIES = 2
        private const val TIMEOUT_MS = 30_000
    }

    private val refreshMonitor = Any()
    @Volatile private var sessionEpoch = 0
    @Volatile private var refreshInFlight: java.util.concurrent.CompletableFuture<TokenResponse>? = null

    @Volatile var onTokensChanged: ((String?, String?) -> Unit)? = null
    @Volatile var onSessionExpired: (() -> Unit)? = null

    fun applyTokens(tokens: TokenResponse) {
        synchronized(refreshMonitor) {
            accessToken = tokens.access_token
            refreshToken = tokens.refresh_token
        }
        onTokensChanged?.invoke(tokens.access_token, tokens.refresh_token)
    }

    fun clearTokens(sessionExpired: Boolean = false) {
        synchronized(refreshMonitor) {
            sessionEpoch += 1
            accessToken = null
            refreshToken = null
        }
        onTokensChanged?.invoke(null, null)
        if (sessionExpired) {
            onSessionExpired?.invoke()
        }
    }

    /** 公开 refresh：与自动 refresh 共用单飞，避免竞态。 */
    fun refreshSession(refresh: String): TokenResponse {
        var created: java.util.concurrent.CompletableFuture<TokenResponse>? = null
        val future = synchronized(refreshMonitor) {
            refreshInFlight?.let { return@synchronized it }
            refreshToken = refresh
            val gate = java.util.concurrent.CompletableFuture<TokenResponse>()
            refreshInFlight = gate
            created = gate
            gate
        }
        runRefreshGate(created, future)
        return awaitFuture(future)
    }

    private fun post(method: String, params: JsonElement?, requiresAuth: Boolean): JsonElement? {
        if (requiresAuth) {
            ensureAccessToken(method)
        }
        val usedAccess = accessToken
        return try {
            postWithNetworkRetry(method, params, requiresAuth, usedAccess)
        } catch (err: JsonRpcClientException.Rpc) {
            if (!requiresAuth || !err.error.isAccessAuthnFailure) {
                throw err
            }
            val current = accessToken
            if (!current.isNullOrEmpty() && current != usedAccess) {
                return postWithNetworkRetry(method, params, requiresAuth, current)
            }
            refreshAccess(staleAccess = usedAccess)
            postWithNetworkRetry(method, params, requiresAuth, accessToken)
        }
    }

    private fun ensureAccessToken(method: String) {
        if (!accessToken.isNullOrEmpty()) return
        if (!refreshToken.isNullOrEmpty()) {
            refreshAccess(staleAccess = null)
            return
        }
        throw JsonRpcClientException.AuthRequired(method)
    }

    private fun postWithNetworkRetry(
        method: String,
        params: JsonElement?,
        requiresAuth: Boolean,
        bearer: String?,
    ): JsonElement? {
        var attempt = 0
        while (true) {
            try {
                return postOnce(method, params, requiresAuth, bearer)
            } catch (err: java.io.IOException) {
                if (attempt >= MAX_NETWORK_RETRIES) throw err
                Thread.sleep(200L * (1L shl attempt))
                attempt += 1
            }
        }
    }

    private fun postOnce(
        method: String,
        params: JsonElement?,
        requiresAuth: Boolean,
        bearer: String?,
    ): JsonElement? {
        val payload = buildJsonObject {
            put("jsonrpc", "2.0")
            put("id", UUID.randomUUID().toString())
            put("method", method)
            if (params != null) put("params", params)
        }
        val conn = (URI(endpoint).toURL().openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            doOutput = true
            connectTimeout = TIMEOUT_MS
            readTimeout = TIMEOUT_MS
            setRequestProperty("Content-Type", "application/json")
            acceptLanguage?.let { setRequestProperty("Accept-Language", it) }
            if (requiresAuth && !bearer.isNullOrEmpty()) {
                setRequestProperty("Authorization", "Bearer $bearer")
            }
        }
        conn.outputStream.use { it.write(json.encodeToString(JsonElement.serializer(), payload).toByteArray(StandardCharsets.UTF_8)) }
        val status = conn.responseCode
        val raw = (if (status in 200..299) conn.inputStream else conn.errorStream)
            ?.bufferedReader(StandardCharsets.UTF_8)?.use { it.readText() }
            .orEmpty()
        if (status !in 200..299) throw JsonRpcClientException.Http(status, raw)
        val root = json.parseToJsonElement(raw).jsonObject
        root["error"]?.let {
            throw JsonRpcClientException.Rpc(json.decodeFromJsonElement(JsonRpcErrorBody.serializer(), it))
        }
        return root["result"]
    }

    /**
     * 自动 refresh：锁内复用 in-flight / 判断是否可跳过 / 登记 Future；网络在锁外。
     * @param staleAccess 鉴权失败时的旧 access；ensure 路径传 null（只要有 refresh 就 rotate）。
     */
    private fun refreshAccess(staleAccess: String?) {
        var created: java.util.concurrent.CompletableFuture<TokenResponse>? = null
        val future = synchronized(refreshMonitor) {
            refreshInFlight?.let { return@synchronized it }
            val current = accessToken
            if (!current.isNullOrEmpty() && (staleAccess == null || current != staleAccess)) {
                return@synchronized null
            }
            val gate = java.util.concurrent.CompletableFuture<TokenResponse>()
            refreshInFlight = gate
            created = gate
            gate
        } ?: return
        runRefreshGate(created, future)
        awaitFuture(future)
    }

    private fun runRefreshGate(
        created: java.util.concurrent.CompletableFuture<TokenResponse>?,
        future: java.util.concurrent.CompletableFuture<TokenResponse>,
    ) {
        if (created != null && created === future) {
            try {
                created.complete(rotateRefresh())
            } catch (e: Throwable) {
                created.completeExceptionally(e)
            } finally {
                synchronized(refreshMonitor) {
                    if (refreshInFlight === created) {
                        refreshInFlight = null
                    }
                }
            }
        }
    }

    private fun <T> awaitFuture(future: java.util.concurrent.CompletableFuture<T>): T {
        try {
            return future.get()
        } catch (e: java.util.concurrent.ExecutionException) {
            when (val cause = e.cause) {
                is RuntimeException -> throw cause
                is Error -> throw cause
                null -> throw e
                else -> throw RuntimeException(cause)
            }
        } catch (e: InterruptedException) {
            Thread.currentThread().interrupt()
            throw JsonRpcClientException.SessionExpired()
        }
    }

    private fun rotateRefresh(): TokenResponse {
        val (current, epoch) = synchronized(refreshMonitor) {
            Pair(refreshToken, sessionEpoch)
        }
        if (current.isNullOrEmpty()) {
            clearTokens(sessionExpired = true)
            throw JsonRpcClientException.SessionExpired()
        }
        try {
            // refresh 非幂等：不做传输层自动重试
            val result = postOnce(
                method = "identity.account.refresh",
                params = buildJsonObject { put("refresh_token", current) },
                requiresAuth = false,
                bearer = null,
            ) ?: throw JsonRpcClientException.MissingResult()
            val tokens = json.decodeFromJsonElement(TokenResponse.serializer(), result)
            synchronized(refreshMonitor) {
                if (epoch != sessionEpoch) {
                    throw JsonRpcClientException.SessionExpired()
                }
                accessToken = tokens.access_token
                refreshToken = tokens.refresh_token
            }
            onTokensChanged?.invoke(tokens.access_token, tokens.refresh_token)
            return tokens
        } catch (err: JsonRpcClientException.Rpc) {
            if (err.error.isRefreshInvalid) {
                clearTokens(sessionExpired = true)
                throw JsonRpcClientException.SessionExpired()
            }
            throw err
        }
    }

    private inline fun <reified P, reified R> call(method: String, params: P, requiresAuth: Boolean): R {
        val result = post(method, json.encodeToJsonElement(params), requiresAuth)
            ?: throw JsonRpcClientException.MissingResult()
        return json.decodeFromJsonElement(result)
    }

    private fun callNull(method: String, params: JsonElement?, requiresAuth: Boolean) {
        post(method, params, requiresAuth)
    }
