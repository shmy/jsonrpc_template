typealias JsonRpcValue = JsonElement

@Serializable
data class JsonRpcErrorBody(
    val code: Int,
    val message: String,
    val data: JsonElement? = null,
) {
    val isAccessAuthnFailure: Boolean
        get() = code == -32020 || code == -32021 || code == -32022

    val isRefreshInvalid: Boolean
        get() = code == -32004
}

sealed class JsonRpcClientException : RuntimeException() {
    class Http(val status: Int, val body: String) : JsonRpcClientException()
    class Rpc(val error: JsonRpcErrorBody) : JsonRpcClientException()
    class MissingResult : JsonRpcClientException()
    class AuthRequired(val method: String) : JsonRpcClientException()
    class SessionExpired : JsonRpcClientException()
}
