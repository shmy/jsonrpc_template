import { JsonRpcClient } from "./jsonRpcClient.ts";

const client = new JsonRpcClient({ acceptLanguage: "zh" });
try {
  const tokens = await client.identityAccountLogin({
    password: "admin123!",
    phone: "13888888888",
  });
  client.accessToken = tokens.access_token;
  console.log(`login ok, expires_in=${tokens.expires_in}`);
} catch (e) {
  console.log(`login failed: ${e}`);
}
