use crate::emit::{DocStyle, SourceBuf, banner, doc_comment, each_method, each_model, w};
use crate::model::{Ir, swift_ident};

pub fn generate(ir: &Ir) -> String {
    let mut o = SourceBuf::default();

    banner(&mut o, ir);
    w!(o, "import Foundation");
    w!(o, "import os");
    o.blank();
    o.line(include_str!("templates/swift/runtime.inc").trim_end());
    o.blank();

    each_model(&mut o, &ir.models, DocStyle::TripleSlash, |o, model| {
        w!(o, "public struct {}: Codable, Sendable {{", model.name);
        for f in &model.fields {
            doc_comment(o, &f.description, DocStyle::TripleSlash, "    ");
            w!(
                o,
                "    public var {}: {}",
                swift_ident(&f.name),
                f.swift_type
            );
        }
        o.blank();
        let args = model
            .fields
            .iter()
            .map(|f| format!("{}: {}", swift_ident(&f.name), f.swift_type))
            .collect::<Vec<_>>()
            .join(", ");
        w!(o, "    public init({}) {{", args);
        for f in &model.fields {
            let id = swift_ident(&f.name);
            w!(o, "        self.{id} = {id}");
        }
        w!(o, "    }}");
        w!(o, "}}");
    });

    w!(
        o,
        "public final class JsonRpcClient: @unchecked Sendable {{"
    );
    w!(o, "    public var endpoint: URL");
    w!(o, "    public var accessToken: String?");
    w!(o, "    public var refreshToken: String?");
    w!(o, "    public var acceptLanguage: String?");
    w!(o, "    public var session: URLSession");
    o.blank();
    w!(
        o,
        "    public init(endpoint: URL = URL(string: \"{}\")!, accessToken: String? = nil, refreshToken: String? = nil, acceptLanguage: String? = nil, session: URLSession = .shared) {{",
        ir.server
    );
    w!(o, "        self.endpoint = endpoint");
    w!(o, "        self.accessToken = accessToken");
    w!(o, "        self.refreshToken = refreshToken");
    w!(o, "        self.acceptLanguage = acceptLanguage");
    w!(o, "        self.session = session");
    w!(o, "    }}");
    o.blank();
    o.line(include_str!("templates/swift/client_core.inc").trim_end());
    o.blank();

    each_method(
        &mut o,
        &ir.methods,
        DocStyle::TripleSlash,
        "    ",
        |o, m| {
            let auth = m.auth_lit();
            let params_sig = m
                .params_type
                .as_ref()
                .map(|pt| format!("_ params: {pt}"))
                .unwrap_or_default();
            let params_arg = if m.params_type.is_some() {
                "params"
            } else {
                "Optional<EmptyParams>.none"
            };
            let is_logout = m.name == "identity.account.logout";
            let is_refresh = m.name == "identity.account.refresh";
            match &m.result_type {
                Some(rt) if is_refresh => {
                    w!(
                        o,
                        "    public func {}({params_sig}) async throws -> {rt} {{",
                        m.fn_name
                    );
                    w!(o, "        return try await refreshSession(refresh: params.refresh_token)");
                }
                Some(rt) if rt == "TokenResponse" => {
                    w!(
                        o,
                        "    public func {}({params_sig}) async throws -> {rt} {{",
                        m.fn_name
                    );
                    w!(
                        o,
                        "        let tokens = try await call(method: \"{}\", params: {params_arg}, requiresAuth: {auth}, as: TokenResponse.self)",
                        m.name
                    );
                    w!(o, "        applyTokens(tokens)");
                    w!(o, "        return tokens");
                }
                Some(rt) => {
                    w!(
                        o,
                        "    public func {}({params_sig}) async throws -> {rt} {{",
                        m.fn_name
                    );
                    w!(
                        o,
                        "        return try await call(method: \"{}\", params: {params_arg}, requiresAuth: {auth}, as: {rt}.self)",
                        m.name
                    );
                }
                None if is_logout => {
                    w!(
                        o,
                        "    public func {}({params_sig}) async throws {{",
                        m.fn_name
                    );
                    w!(o, "        defer {{ clearTokens() }}");
                    w!(
                        o,
                        "        let _: NullResult = try await call(method: \"{}\", params: {params_arg}, requiresAuth: {auth}, as: NullResult.self)",
                        m.name
                    );
                }
                None => {
                    w!(
                        o,
                        "    public func {}({params_sig}) async throws {{",
                        m.fn_name
                    );
                    w!(
                        o,
                        "        let _: NullResult = try await call(method: \"{}\", params: {params_arg}, requiresAuth: {auth}, as: NullResult.self)",
                        m.name
                    );
                }
            }
            w!(o, "    }}");
        },
    );

    w!(o, "}}");
    o.blank();
    o.into_string()
}
