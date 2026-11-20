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
    private struct RefreshParams: Encodable {
        let refresh_token: String
    }

    private static let maxNetworkRetries = 2
    private let refreshCoordinator = RefreshCoordinator()
    /// 仅存 sessionEpoch；access/refresh 仍用公开属性（便于调用方直接赋值），读写均经 withLock。
    private let epochLock = OSAllocatedUnfairLock(initialState: 0)

    public var onTokensChanged: (@Sendable (String?, String?) -> Void)?
    public var onSessionExpired: (@Sendable () -> Void)?

    private actor RefreshCoordinator {
        private var inFlight: Task<TokenResponse, Error>?

        func run(_ operation: @escaping @Sendable () async throws -> TokenResponse) async throws -> TokenResponse {
            if let inFlight {
                return try await inFlight.value
            }
            let task = Task { try await operation() }
            inFlight = task
            defer { inFlight = nil }
            return try await task.value
        }
    }

    public func applyTokens(_ tokens: TokenResponse) {
        let pair = epochLock.withLock { _ -> (String?, String?) in
            accessToken = tokens.access_token
            refreshToken = tokens.refresh_token
            return (accessToken, refreshToken)
        }
        onTokensChanged?(pair.0, pair.1)
    }

    public func clearTokens(sessionExpired: Bool = false) {
        epochLock.withLock { epoch in
            epoch += 1
            accessToken = nil
            refreshToken = nil
        }
        onTokensChanged?(nil, nil)
        if sessionExpired { onSessionExpired?() }
    }

    /// 公开 refresh：若已有单飞则加入；否则用 refresh 发起一轮。
    public func refreshSession(refresh: String) async throws -> TokenResponse {
        try await refreshCoordinator.run { [weak self] in
            guard let self else { throw JsonRpcClientError.sessionExpired }
            self.epochLock.withLock { _ in self.refreshToken = refresh }
            return try await self.rotateRefresh()
        }
    }

    public func call<P: Encodable, R: Decodable>(
        method: String,
        params: P?,
        requiresAuth: Bool,
        as _: R.Type
    ) async throws -> R {
        if requiresAuth {
            try await ensureAccessToken(method: method)
        }
        let usedAccess = epochLock.withLock { _ in accessToken }
        do {
            return try await callWithNetworkRetry(
                method: method,
                params: params,
                requiresAuth: requiresAuth,
                bearer: usedAccess,
                as: R.self
            )
        } catch let JsonRpcClientError.rpc(err) {
            if !requiresAuth || !err.isAccessAuthnFailure {
                throw JsonRpcClientError.rpc(err)
            }
            let current = epochLock.withLock { _ in accessToken }
            if let current, !current.isEmpty, current != usedAccess {
                return try await callWithNetworkRetry(
                    method: method,
                    params: params,
                    requiresAuth: requiresAuth,
                    bearer: current,
                    as: R.self
                )
            }
            _ = try await refreshSingleFlight()
            let bearer = epochLock.withLock { _ in accessToken }
            return try await callWithNetworkRetry(
                method: method,
                params: params,
                requiresAuth: requiresAuth,
                bearer: bearer,
                as: R.self
            )
        }
    }

    private func ensureAccessToken(method: String) async throws {
        let (access, refresh) = epochLock.withLock { _ in (accessToken, refreshToken) }
        if let access, !access.isEmpty { return }
        if let refresh, !refresh.isEmpty {
            _ = try await refreshSingleFlight()
            return
        }
        throw JsonRpcClientError.authRequired(method: method)
    }

    private func callWithNetworkRetry<P: Encodable, R: Decodable>(
        method: String,
        params: P?,
        requiresAuth: Bool,
        bearer: String?,
        as type: R.Type
    ) async throws -> R {
        var attempt = 0
        while true {
            do {
                return try await callOnce(
                    method: method,
                    params: params,
                    requiresAuth: requiresAuth,
                    bearer: bearer,
                    as: type
                )
            } catch let error as URLError {
                guard Self.isRetryable(error), attempt < Self.maxNetworkRetries else { throw error }
            } catch {
                throw error
            }
            let delay = UInt64(200 * (1 << attempt)) * 1_000_000
            try await Task.sleep(nanoseconds: delay)
            attempt += 1
        }
    }

    private static func isRetryable(_ error: URLError) -> Bool {
        switch error.code {
        case .timedOut, .cannotConnectToHost, .networkConnectionLost,
             .notConnectedToInternet, .dnsLookupFailed, .cannotFindHost:
            return true
        default:
            return false
        }
    }

    private func callOnce<P: Encodable, R: Decodable>(
        method: String,
        params: P?,
        requiresAuth: Bool,
        bearer: String?,
        as _: R.Type
    ) async throws -> R {
        var req = URLRequest(url: endpoint)
        req.httpMethod = "POST"
        req.timeoutInterval = 30
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let acceptLanguage { req.setValue(acceptLanguage, forHTTPHeaderField: "Accept-Language") }
        if requiresAuth, let bearer, !bearer.isEmpty {
            req.setValue("Bearer \(bearer)", forHTTPHeaderField: "Authorization")
        }
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

    private func refreshSingleFlight() async throws -> TokenResponse {
        try await refreshCoordinator.run { [weak self] in
            guard let self else { throw JsonRpcClientError.sessionExpired }
            return try await self.rotateRefresh()
        }
    }

    private func rotateRefresh() async throws -> TokenResponse {
        let (current, epoch) = epochLock.withLock { epoch in (refreshToken, epoch) }
        guard let current, !current.isEmpty else {
            clearTokens(sessionExpired: true)
            throw JsonRpcClientError.sessionExpired
        }
        do {
            // refresh 非幂等：不做传输层自动重试
            let tokens: TokenResponse = try await callOnce(
                method: "identity.account.refresh",
                params: Optional(RefreshParams(refresh_token: current)),
                requiresAuth: false,
                bearer: nil,
                as: TokenResponse.self
            )
            let pair: (String?, String?) = try epochLock.withLock { still in
                guard epoch == still else {
                    throw JsonRpcClientError.sessionExpired
                }
                accessToken = tokens.access_token
                refreshToken = tokens.refresh_token
                return (accessToken, refreshToken)
            }
            onTokensChanged?(pair.0, pair.1)
            return tokens
        } catch let JsonRpcClientError.rpc(err) {
            if err.isRefreshInvalid {
                clearTokens(sessionExpired: true)
                throw JsonRpcClientError.sessionExpired
            }
            throw JsonRpcClientError.rpc(err)
        }
    }
