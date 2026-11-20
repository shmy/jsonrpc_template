import Foundation

let client = JsonRpcClient(acceptLanguage: "zh")
do {
    _ = try await client.identityAccountLogin(
        IdentityAccountLoginParams(password: "__SEED_PASSWORD__", phone: "__SEED_PHONE__")
    )
    print("login ok, access=\(client.accessToken != nil) refresh=\(client.refreshToken != nil)")

    // 无效 access → -32021 → 单飞 refresh → 重试
    client.accessToken = "invalid-access-token"
    async let a = client.identityAccountSearch(
        IdentityAccountSearchParams(cursor: nil, limit: 1, name: nil, phone: "__SEED_PHONE__")
    )
    async let b = client.identityAccountSearch(
        IdentityAccountSearchParams(cursor: nil, limit: 1, name: nil, phone: "__SEED_PHONE__")
    )
    let (ra, _) = try await (a, b)
    print("concurrent ok, items=\(ra.items.count)")
} catch {
    fputs("smoke failed: \(error)\n", stderr)
    exit(1)
}
