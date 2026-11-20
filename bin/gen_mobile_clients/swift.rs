use crate::emit::{DocStyle, SourceBuf, banner, doc_comment, each_method, each_model, w};
use crate::model::{Ir, swift_ident};

pub fn generate(ir: &Ir) -> String {
    let mut o = SourceBuf::default();

    banner(&mut o, ir);
    w!(o, "import Foundation");
    o.blank();
    o.line(RUNTIME.trim_end());
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
    w!(o, "    public var acceptLanguage: String?");
    w!(o, "    public var session: URLSession");
    o.blank();
    w!(
        o,
        "    public init(endpoint: URL = URL(string: \"{}\")!, accessToken: String? = nil, acceptLanguage: String? = nil, session: URLSession = .shared) {{",
        ir.server
    );
    w!(o, "        self.endpoint = endpoint");
    w!(o, "        self.accessToken = accessToken");
    w!(o, "        self.acceptLanguage = acceptLanguage");
    w!(o, "        self.session = session");
    w!(o, "    }}");
    o.blank();
    o.line(CLIENT_CORE.trim_end());
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
            match &m.result_type {
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

const RUNTIME: &str = r#"
public enum JsonRpcNull: Codable, Sendable {}

public enum JsonRpcValue: Codable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JsonRpcValue])
    case object([String: JsonRpcValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null; return }
        if let v = try? c.decode(Bool.self) { self = .bool(v); return }
        if let v = try? c.decode(Double.self) { self = .number(v); return }
        if let v = try? c.decode(String.self) { self = .string(v); return }
        if let v = try? c.decode([JsonRpcValue].self) { self = .array(v); return }
        if let v = try? c.decode([String: JsonRpcValue].self) { self = .object(v); return }
        throw DecodingError.dataCorruptedError(in: c, debugDescription: "unsupported JSON value")
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }
}

public struct JsonRpcError: Error, Codable, Sendable {
    public let code: Int
    public let message: String
    public let data: JsonRpcValue?
}

public enum JsonRpcClientError: Error, Sendable {
    case http(status: Int, body: String)
    case rpc(JsonRpcError)
    case missingResult
    case authRequired(method: String)
}
"#;

const CLIENT_CORE: &str = r#"
    private struct RequestEnvelope<P: Encodable>: Encodable {
        let jsonrpc = "2.0"
        let id: String
        let method: String
        let params: P?
    }

    private struct ResponseEnvelope<R: Decodable>: Decodable {
        let jsonrpc: String?
        let id: String?
        let result: R?
        let error: JsonRpcError?
    }

    private struct EmptyParams: Encodable {}
    private struct NullResult: Decodable {}

    public func call<P: Encodable, R: Decodable>(
        method: String,
        params: P?,
        requiresAuth: Bool,
        as _: R.Type
    ) async throws -> R {
        if requiresAuth, accessToken == nil || accessToken?.isEmpty == true {
            throw JsonRpcClientError.authRequired(method: method)
        }
        var req = URLRequest(url: endpoint)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let acceptLanguage { req.setValue(acceptLanguage, forHTTPHeaderField: "Accept-Language") }
        if let accessToken { req.setValue("Bearer \(accessToken)", forHTTPHeaderField: "Authorization") }
        let envelope = RequestEnvelope(id: UUID().uuidString, method: method, params: params)
        req.httpBody = try JSONEncoder().encode(envelope)
        let (data, response) = try await session.data(for: req)
        let status = (response as? HTTPURLResponse)?.statusCode ?? -1
        guard (200..<300).contains(status) else {
            throw JsonRpcClientError.http(status: status, body: String(data: data, encoding: .utf8) ?? "")
        }
        let decoded = try JSONDecoder().decode(ResponseEnvelope<R>.self, from: data)
        if let error = decoded.error { throw JsonRpcClientError.rpc(error) }
        if R.self == NullResult.self {
            return NullResult() as! R
        }
        guard let result = decoded.result else { throw JsonRpcClientError.missingResult }
        return result
    }
"#;
