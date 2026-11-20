import Foundation

let client = JsonRpcClient(acceptLanguage: "zh")
do {
    let tokens = try await client.identityAccountLogin(
        IdentityAccountLoginParams(password: "__SEED_PASSWORD__", phone: "__SEED_PHONE__")
    )
    client.accessToken = tokens.access_token
    print("login ok, expires_in=\(tokens.expires_in)")
} catch {
    print("login failed: \(error)")
}
