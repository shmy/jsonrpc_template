import 'json_rpc_client.dart';

Future<void> main() async {
  final client = JsonRpcClient(acceptLanguage: 'zh');
  try {
    final tokens = await client.identityAccountLogin(
      IdentityAccountLoginParams(password: '__SEED_PASSWORD__', phone: '__SEED_PHONE__'),
    );
    client.accessToken = tokens.access_token;
    print('login ok, expires_in=${tokens.expires_in}');
  } catch (e) {
    print('login failed: $e');
  } finally {
    client.close();
  }
}
