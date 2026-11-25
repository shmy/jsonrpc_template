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

    public var isAccessAuthnFailure: Bool {
        code == -32020 || code == -32021 || code == -32022
    }

    public var isRefreshInvalid: Bool { code == -32004 }
}

public enum JsonRpcClientError: Error, Sendable {
    case http(status: Int, body: String)
    case rpc(JsonRpcError)
    case missingResult
    case authRequired(method: String)
    case sessionExpired
}
