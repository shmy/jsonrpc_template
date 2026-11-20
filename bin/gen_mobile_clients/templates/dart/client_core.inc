  Future<dynamic> _post({
    required String method,
    Map<String, dynamic>? params,
    required bool requiresAuth,
  }) async {
    if (requiresAuth && (accessToken == null || accessToken!.isEmpty)) {
      throw JsonRpcAuthRequiredException(method);
    }
    final payload = <String, dynamic>{
      'jsonrpc': '2.0',
      'id': DateTime.now().microsecondsSinceEpoch.toString(),
      'method': method,
      if (params != null) 'params': params,
    };
    final request = await _http.postUrl(Uri.parse(endpoint));
    request.headers.contentType = ContentType.json;
    if (acceptLanguage != null) {
      request.headers.set('Accept-Language', acceptLanguage!);
    }
    if (accessToken != null) {
      request.headers.set('Authorization', 'Bearer $accessToken');
    }
    request.write(jsonEncode(payload));
    final response = await request.close();
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
  }
