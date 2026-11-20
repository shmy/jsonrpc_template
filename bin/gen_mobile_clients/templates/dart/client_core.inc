  static const int _maxNetworkRetries = 2;
  static const Duration _timeout = Duration(seconds: 30);

  Future<TokenResponse>? _refreshInFlight;
  int _sessionEpoch = 0;

  void Function(String? accessToken, String? refreshToken)? onTokensChanged;
  void Function()? onSessionExpired;

  void applyTokens(TokenResponse tokens) {
    accessToken = tokens.access_token;
    refreshToken = tokens.refresh_token;
    onTokensChanged?.call(accessToken, refreshToken);
  }

  void clearTokens({bool sessionExpired = false}) {
    _sessionEpoch += 1;
    accessToken = null;
    refreshToken = null;
    onTokensChanged?.call(null, null);
    if (sessionExpired) {
      onSessionExpired?.call();
    }
  }

  /// 公开 refresh：若已有单飞则加入；否则用 [refresh] 发起一轮。
  Future<TokenResponse> refreshSession(String refresh) {
    final inflight = _refreshInFlight;
    if (inflight != null) {
      return inflight;
    }
    refreshToken = refresh;
    return _refreshSingleFlight();
  }

  Future<dynamic> _post({
    required String method,
    Map<String, dynamic>? params,
    required bool requiresAuth,
  }) async {
    if (requiresAuth) {
      await _ensureAccessToken(method);
    }
    final usedAccess = accessToken;
    try {
      return await _postWithNetworkRetry(
        method: method,
        params: params,
        requiresAuth: requiresAuth,
        bearer: usedAccess,
      );
    } on JsonRpcError catch (err) {
      if (!requiresAuth || !err.isAccessAuthnFailure) {
        rethrow;
      }
      // 他人已轮换：直接用新 access 重试，勿二次 rotate
      if (accessToken != null &&
          accessToken!.isNotEmpty &&
          accessToken != usedAccess) {
        return await _postWithNetworkRetry(
          method: method,
          params: params,
          requiresAuth: requiresAuth,
          bearer: accessToken,
        );
      }
      await _refreshSingleFlight();
      return await _postWithNetworkRetry(
        method: method,
        params: params,
        requiresAuth: requiresAuth,
        bearer: accessToken,
      );
    }
  }

  Future<void> _ensureAccessToken(String method) async {
    if (accessToken != null && accessToken!.isNotEmpty) {
      return;
    }
    if (refreshToken != null && refreshToken!.isNotEmpty) {
      await _refreshSingleFlight();
      return;
    }
    throw JsonRpcAuthRequiredException(method);
  }

  Future<dynamic> _postWithNetworkRetry({
    required String method,
    Map<String, dynamic>? params,
    required bool requiresAuth,
    String? bearer,
  }) async {
    var attempt = 0;
    while (true) {
      try {
        return await _postOnce(
          method: method,
          params: params,
          requiresAuth: requiresAuth,
          bearer: bearer,
        );
      } on SocketException {
        if (attempt >= _maxNetworkRetries) rethrow;
      } on HttpException {
        if (attempt >= _maxNetworkRetries) rethrow;
      } on TimeoutException {
        if (attempt >= _maxNetworkRetries) rethrow;
      }
      await Future<void>.delayed(Duration(milliseconds: 200 * (1 << attempt)));
      attempt += 1;
    }
  }

  Future<dynamic> _postOnce({
    required String method,
    Map<String, dynamic>? params,
    required bool requiresAuth,
    String? bearer,
  }) async {
    final payload = <String, dynamic>{
      'jsonrpc': '2.0',
      'id': DateTime.now().microsecondsSinceEpoch.toString(),
      'method': method,
      if (params != null) 'params': params,
    };
    _http.connectionTimeout = _timeout;
    final request = await _http.postUrl(Uri.parse(endpoint));
    request.headers.contentType = ContentType.json;
    if (acceptLanguage != null) {
      request.headers.set('Accept-Language', acceptLanguage!);
    }
    if (requiresAuth && bearer != null && bearer.isNotEmpty) {
      request.headers.set('Authorization', 'Bearer $bearer');
    }
    request.write(jsonEncode(payload));
    try {
      final response = await request.close().timeout(_timeout);
      final body = await response.transform(utf8.decoder).join();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw JsonRpcHttpException(response.statusCode, body);
      }
      final root = jsonDecode(body) as Map<String, dynamic>;
      final error = root['error'];
      if (error != null) {
        throw JsonRpcError.fromJson(error as Map<String, dynamic>);
      }
      return root['result'];
    } on TimeoutException {
      request.abort();
      rethrow;
    } catch (e) {
      if (e is! JsonRpcError && e is! JsonRpcHttpException) {
        request.abort();
      }
      rethrow;
    }
  }

  /// 同步占坑后再启动 I/O，避免并发双发 refresh。
  Future<TokenResponse> _refreshSingleFlight() {
    final existing = _refreshInFlight;
    if (existing != null) {
      return existing;
    }
    final gate = Completer<TokenResponse>();
    _refreshInFlight = gate.future;
    () async {
      try {
        gate.complete(await _rotateRefresh());
      } catch (e, st) {
        gate.completeError(e, st);
      } finally {
        if (identical(_refreshInFlight, gate.future)) {
          _refreshInFlight = null;
        }
      }
    }();
    return gate.future;
  }

  Future<TokenResponse> _rotateRefresh() async {
    final current = refreshToken;
    if (current == null || current.isEmpty) {
      clearTokens(sessionExpired: true);
      throw JsonRpcSessionExpiredException();
    }
    final epoch = _sessionEpoch;
    try {
      // refresh 非幂等：不做传输层自动重试
      final result = await _postOnce(
        method: 'identity.account.refresh',
        params: <String, dynamic>{'refresh_token': current},
        requiresAuth: false,
        bearer: null,
      );
      final tokens = TokenResponse.fromJson(result as Map<String, dynamic>);
      // 与 clearTokens 同序、无 await：epoch 未变才写入（防假登录；服务端新票成孤儿见 README）
      if (epoch != _sessionEpoch) {
        throw JsonRpcSessionExpiredException();
      }
      accessToken = tokens.access_token;
      refreshToken = tokens.refresh_token;
      onTokensChanged?.call(accessToken, refreshToken);
      return tokens;
    } on JsonRpcError catch (err) {
      if (err.isRefreshInvalid) {
        clearTokens(sessionExpired: true);
        throw JsonRpcSessionExpiredException();
      }
      rethrow;
    }
  }
