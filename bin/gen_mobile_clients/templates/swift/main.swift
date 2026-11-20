import Foundation

let client = JsonRpcClient(acceptLanguage: "zh")
do {
    let tokens = try await client.identityAccountLogin(
        IdentityAccountLoginParams(password: "admin123!", phone: "13888888888")
    )
    client.accessToken = tokens.access_token
    print("login ok, expires_in=\(tokens.expires_in)")
} catch {
    print("login failed: \(error)")
}
