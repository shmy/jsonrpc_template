import 'json_rpc_client.dart';

Future<void> main() async {
  final client = JsonRpcClient(acceptLanguage: 'zh');
  try {
    final tokens = await client.identityAccountLogin(
      IdentityAccountLoginParams(password: 'admin123!', phone: '13888888888'),
    );
    client.accessToken = tokens.access_token;
    print('login ok, expires_in=${tokens.expires_in}');
  } catch (e) {
    print('login failed: $e');
  } finally {
    client.close();
  }
}
