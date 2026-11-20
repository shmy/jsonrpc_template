import { JsonRpcClient } from "./jsonRpcClient.ts";

const client = new JsonRpcClient({ acceptLanguage: "zh" });
try {
  await client.identityAccountLogin({
    password: "__SEED_PASSWORD__",
    phone: "__SEED_PHONE__",
  });
  console.log(
    `login ok, access=${client.accessToken != null} refresh=${client.refreshToken != null}`,
  );

  // 无效 access → -32021 → 单飞 refresh → 重试
  client.accessToken = "invalid-access-token";
  const results = await Promise.all([
    client.identityAccountSearch({
      cursor: null,
      limit: 1,
      name: null,
      phone: "__SEED_PHONE__",
    }),
    client.identityAccountSearch({
      cursor: null,
      limit: 1,
      name: null,
      phone: "__SEED_PHONE__",
    }),
  ]);
  console.log(`concurrent ok, items=${results[0].items.length}`);
} catch (e) {
  console.error(`smoke failed: ${e}`);
  process.exitCode = 1;
}
