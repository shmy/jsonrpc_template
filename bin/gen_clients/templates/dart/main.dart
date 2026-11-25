import 'dart:io';

import 'json_rpc_client.dart';

Future<void> main() async {
  final client = JsonRpcClient(acceptLanguage: 'zh');
  try {
    await client.identityAccountLogin(
      IdentityAccountLoginParams(password: '__SEED_PASSWORD__', phone: '__SEED_PHONE__'),
    );
    stdout.writeln(
      'login ok, access=${client.accessToken != null} refresh=${client.refreshToken != null}',
    );

    // 无效 access → -32021 → 单飞 refresh → 重试
    client.accessToken = 'invalid-access-token';
    final results = await Future.wait([
      client.identityAccountSearch(
        IdentityAccountSearchParams(cursor: null, limit: 1, name: null, phone: '__SEED_PHONE__'),
      ),
      client.identityAccountSearch(
        IdentityAccountSearchParams(cursor: null, limit: 1, name: null, phone: '__SEED_PHONE__'),
      ),
    ]);
    stdout.writeln('concurrent ok, items=${results.first.items.length}');
  } catch (e) {
    stderr.writeln('smoke failed: $e');
    exitCode = 1;
  } finally {
    client.close();
  }
}
