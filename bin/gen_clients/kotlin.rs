use crate::emit::{DocStyle, SourceBuf, banner, doc_comment, each_method, each_model, w};
use crate::model::{Ir, kotlin_ident};

pub fn generate(ir: &Ir) -> String {
    let mut o = SourceBuf::default();

    banner(&mut o, ir);
    w!(o, "package com.example.jsonrpc.generated");
    o.blank();
    o.line(IMPORTS.trim_end());
    o.blank();
    o.line(include_str!("templates/kotlin/runtime.inc").trim_end());
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
    w!(o, "    @Volatile var accessToken: String? = null,");
    w!(o, "    @Volatile var refreshToken: String? = null,");
    w!(o, "    var acceptLanguage: String? = null,");
    w!(
        o,
        "    private val json: Json = Json {{ ignoreUnknownKeys = true; explicitNulls = true }},"
    );
    w!(o, ") {{");
    o.line(include_str!("templates/kotlin/client_core.inc").trim_end());
    o.blank();

    each_method(&mut o, &ir.methods, DocStyle::Block, "    ", |o, m| {
        let auth = m.auth_lit();
        let is_logout = m.name == "identity.account.logout";
        let is_refresh = m.name == "identity.account.refresh";
        match (&m.params_type, &m.result_type) {
            (Some(pt), Some(_)) if is_refresh => {
                w!(o, "    fun {}(params: {pt}): TokenResponse {{", m.fn_name);
                w!(o, "        return refreshSession(params.refresh_token)");
            }
            (Some(pt), Some(rt)) if rt == "TokenResponse" => {
                w!(o, "    fun {}(params: {pt}): TokenResponse {{", m.fn_name);
                w!(
                    o,
                    "        val tokens = call<{pt}, TokenResponse>(method = \"{}\", params = params, requiresAuth = {auth})",
                    m.name
                );
                w!(o, "        applyTokens(tokens)");
                w!(o, "        return tokens");
            }
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
            (None, Some(rt)) if rt == "TokenResponse" => {
                w!(o, "    fun {}(): TokenResponse {{", m.fn_name);
                w!(
                    o,
                    "        val result = post(method = \"{}\", params = null, requiresAuth = {auth}) ?: throw JsonRpcClientException.MissingResult",
                    m.name
                );
                w!(
                    o,
                    "        val tokens = json.decodeFromJsonElement<TokenResponse>(result)"
                );
                w!(o, "        applyTokens(tokens)");
                w!(o, "        return tokens");
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
            (None, None) if is_logout => {
                w!(o, "    fun {}() {{", m.fn_name);
                w!(o, "        try {{");
                w!(
                    o,
                    "            callNull(method = \"{}\", params = null, requiresAuth = {auth})",
                    m.name
                );
                w!(o, "        }} finally {{");
                w!(o, "            clearTokens()");
                w!(o, "        }}");
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
