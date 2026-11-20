use crate::emit::{DocStyle, SourceBuf, banner, doc_comment, each_method, each_model, w};
use crate::model::{Ir, kotlin_ident};

pub fn generate(ir: &Ir) -> String {
    let mut o = SourceBuf::default();

    banner(&mut o, ir);
    w!(o, "package com.example.jsonrpc.generated");
    o.blank();
    o.line(IMPORTS.trim_end());
    o.blank();
    o.line(RUNTIME.trim_end());
    o.blank();

    each_model(&mut o, &ir.models, DocStyle::Block, |o, model| {
        w!(o, "@Serializable");
        w!(o, "data class {}(", model.name);
        for (i, f) in model.fields.iter().enumerate() {
            let comma = if i + 1 < model.fields.len() { "," } else { "" };
            let default = if f.optional { " = null" } else { "" };
            let ident = kotlin_ident(&f.name);
            doc_comment(o, &f.description, DocStyle::Block, "    ");
            if ident != f.name {
                w!(o, "    @SerialName(\"{}\")", f.name);
            }
            w!(o, "    val {ident}: {}{default}{comma}", f.kotlin_type);
        }
        w!(o, ")");
    });

    w!(o, "class JsonRpcClient(");
    w!(o, "    var endpoint: String = \"{}\",", ir.server);
    w!(o, "    var accessToken: String? = null,");
    w!(o, "    var acceptLanguage: String? = null,");
    w!(
        o,
        "    private val json: Json = Json {{ ignoreUnknownKeys = true; explicitNulls = true }},"
    );
    w!(o, ") {{");
    o.line(CLIENT_CORE.trim_end());
    o.blank();

    each_method(&mut o, &ir.methods, DocStyle::Block, "    ", |o, m| {
        let auth = m.auth_lit();
        match (&m.params_type, &m.result_type) {
            (Some(pt), Some(rt)) => {
                w!(o, "    fun {}(params: {pt}): {rt} {{", m.fn_name);
                w!(
                    o,
                    "        return call(method = \"{}\", params = params, requiresAuth = {auth})",
                    m.name
                );
            }
            (Some(pt), None) => {
                w!(o, "    fun {}(params: {pt}) {{", m.fn_name);
                w!(
                    o,
                    "        callNull(method = \"{}\", params = json.encodeToJsonElement(params), requiresAuth = {auth})",
                    m.name
                );
            }
            (None, Some(rt)) => {
                w!(o, "    fun {}(): {rt} {{", m.fn_name);
                w!(
                    o,
                    "        val result = post(method = \"{}\", params = null, requiresAuth = {auth}) ?: throw JsonRpcClientException.MissingResult",
                    m.name
                );
                w!(o, "        return json.decodeFromJsonElement<{rt}>(result)");
            }
            (None, None) => {
                w!(o, "    fun {}() {{", m.fn_name);
                w!(
                    o,
                    "        callNull(method = \"{}\", params = null, requiresAuth = {auth})",
                    m.name
                );
            }
        }
        w!(o, "    }}");
    });

    w!(o, "}}");
    o.blank();
    o.into_string()
}

const IMPORTS: &str = r#"
import java.net.HttpURLConnection
import java.net.URI
import java.nio.charset.StandardCharsets
import java.util.UUID
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
"#;

const RUNTIME: &str = r#"
typealias JsonRpcValue = JsonElement

@Serializable
data class JsonRpcErrorBody(
    val code: Int,
    val message: String,
    val data: JsonElement? = null,
)

sealed class JsonRpcClientException : RuntimeException() {
    data class Http(val status: Int, val body: String) : JsonRpcClientException()
    data class Rpc(val error: JsonRpcErrorBody) : JsonRpcClientException()
    data object MissingResult : JsonRpcClientException()
    data class AuthRequired(val method: String) : JsonRpcClientException()
}
"#;

const CLIENT_CORE: &str = r#"
    private fun post(method: String, params: JsonElement?, requiresAuth: Boolean): JsonElement? {
        if (requiresAuth && accessToken.isNullOrEmpty()) {
            throw JsonRpcClientException.AuthRequired(method)
        }
        val payload = buildJsonObject {
            put("jsonrpc", "2.0")
            put("id", UUID.randomUUID().toString())
            put("method", method)
            if (params != null) put("params", params)
        }
        val conn = (URI(endpoint).toURL().openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            doOutput = true
            setRequestProperty("Content-Type", "application/json")
            acceptLanguage?.let { setRequestProperty("Accept-Language", it) }
            accessToken?.let { setRequestProperty("Authorization", "Bearer $it") }
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

    private inline fun <reified P, reified R> call(method: String, params: P, requiresAuth: Boolean): R {
        val result = post(method, json.encodeToJsonElement(params), requiresAuth)
            ?: throw JsonRpcClientException.MissingResult
        return json.decodeFromJsonElement(result)
    }

    private fun callNull(method: String, params: JsonElement?, requiresAuth: Boolean) {
        post(method, params, requiresAuth)
    }
"#;
