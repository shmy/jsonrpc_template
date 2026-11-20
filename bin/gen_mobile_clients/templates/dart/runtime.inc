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
