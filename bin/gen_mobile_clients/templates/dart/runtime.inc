class JsonRpcError implements Exception {
  JsonRpcError({required this.code, required this.message, this.data});
  final int code;
  final String message;
  final dynamic data;

  factory JsonRpcError.fromJson(Map<String, dynamic> json) => JsonRpcError(
        code: json['code'] as int,
        message: json['message'] as String,
        data: json['data'],
      );

  /// access 缺失 / 无效 / 已吊销（`http_auth` `-32020..-32022`）
  bool get isAccessAuthnFailure => code == -32020 || code == -32021 || code == -32022;

  /// refresh 无效或 replay（identity `-32004`）→ 需重新 login
  bool get isRefreshInvalid => code == -32004;

  @override
  String toString() => 'JsonRpcError($code): $message';
}

class JsonRpcHttpException implements Exception {
  JsonRpcHttpException(this.status, this.body);
  final int status;
  final String body;
  @override
  String toString() => 'JsonRpcHttpException($status): $body';
}

class JsonRpcAuthRequiredException implements Exception {
  JsonRpcAuthRequiredException(this.method);
  final String method;
  @override
  String toString() => 'JsonRpcAuthRequiredException: $method';
}

/// refresh 不可用或轮换失败，本地会话已清空。
class JsonRpcSessionExpiredException implements Exception {
  @override
  String toString() => 'JsonRpcSessionExpiredException';
}
