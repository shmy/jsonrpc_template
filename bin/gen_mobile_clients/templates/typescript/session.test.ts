/** mock fetch 会话单测：`bun session.test.ts` */
import {
  JsonRpcClient,
  JsonRpcError,
  JsonRpcSessionExpiredError,
  type TokenResponse,
} from "./jsonRpcClient.ts";

type RpcBody = { method: string };

function jsonOk(result: unknown): Response {
  return new Response(JSON.stringify({ jsonrpc: "2.0", id: "1", result }), {
    status: 200,
  });
}

function jsonErr(code: number, message: string): Response {
  return new Response(
    JSON.stringify({ jsonrpc: "2.0", id: "1", error: { code, message } }),
    { status: 200 },
  );
}

function tokens(n: number): TokenResponse {
  return {
    access_token: `access-${n}`,
    refresh_token: `refresh-${n}`,
    token_type: "Bearer",
    expires_in: 3600,
  };
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

async function assertRejects(
  fn: () => Promise<unknown>,
  check: (e: unknown) => boolean,
): Promise<void> {
  try {
    await fn();
    throw new Error("expected rejection");
  } catch (e) {
    if (e instanceof Error && e.message === "expected rejection") throw e;
    assert(check(e), `unexpected rejection: ${String(e)}`);
  }
}

function authHeader(init?: RequestInit): string | undefined {
  return (init?.headers as Record<string, string> | undefined)?.Authorization;
}

async function testSingleFlightRefresh(): Promise<void> {
  let refreshCalls = 0;
  const fetchMock: typeof fetch = async (_url, init) => {
    const body = JSON.parse(String(init?.body)) as RpcBody;
    if (body.method === "identity.account.refresh") {
      refreshCalls += 1;
      await new Promise((r) => setTimeout(r, 30));
      return jsonOk(tokens(refreshCalls));
    }
    if (body.method === "identity.account.read") {
      if (authHeader(init) === "Bearer stale") {
        return jsonErr(-32021, "invalid access");
      }
      return jsonOk({ id: "1" });
    }
    return jsonErr(-32601, `unexpected ${body.method}`);
  };

  const client = new JsonRpcClient({
    endpoint: "http://test/jsonrpc",
    accessToken: "stale",
    refreshToken: "refresh-0",
    fetch: fetchMock,
  });
  await Promise.all([
    client.identityAccountRead({ id: "1" }),
    client.identityAccountRead({ id: "1" }),
  ]);
  assert(refreshCalls === 1, `expected 1 refresh, got ${refreshCalls}`);
  assert(client.accessToken === "access-1", "access should rotate once");
}

async function testOnly32004ClearsSession(): Promise<void> {
  let n = 0;
  const fetchMock: typeof fetch = async (_url, init) => {
    const body = JSON.parse(String(init?.body)) as RpcBody;
    if (body.method === "identity.account.refresh") {
      n += 1;
      return n === 1
        ? jsonErr(-32005, "rate limited")
        : jsonErr(-32004, "replay");
    }
    return jsonErr(-32601, "nope");
  };
  const client = new JsonRpcClient({
    endpoint: "http://test/jsonrpc",
    refreshToken: "r",
    fetch: fetchMock,
  });

  await assertRejects(
    () => client.refreshSession("r"),
    (e) => e instanceof JsonRpcError && e.code === -32005,
  );
  assert(client.refreshToken === "r", "rate-limit must keep refresh");

  await assertRejects(
    () => client.refreshSession("r"),
    (e) => e instanceof JsonRpcSessionExpiredError,
  );
  assert(client.accessToken == null && client.refreshToken == null, "cleared");
}

async function testEpochDropsOrphanTicket(): Promise<void> {
  let resolveRefresh!: (v: Response) => void;
  const refreshGate = new Promise<Response>((r) => {
    resolveRefresh = r;
  });
  const fetchMock: typeof fetch = async (_url, init) => {
    const body = JSON.parse(String(init?.body)) as RpcBody;
    if (body.method === "identity.account.refresh") return refreshGate;
    return jsonErr(-32601, "nope");
  };
  const client = new JsonRpcClient({
    endpoint: "http://test/jsonrpc",
    refreshToken: "r0",
    fetch: fetchMock,
  });
  const pending = client.refreshSession("r0");
  client.clearTokens();
  resolveRefresh(jsonOk(tokens(9)));
  await assertRejects(
    () => pending,
    (e) => e instanceof JsonRpcSessionExpiredError,
  );
  assert(client.accessToken == null && client.refreshToken == null, "no resurrect");
}

async function testUsedAccessShortCircuit(): Promise<void> {
  let refreshCalls = 0;
  const fetchMock: typeof fetch = async (_url, init) => {
    const body = JSON.parse(String(init?.body)) as RpcBody;
    if (body.method === "identity.account.refresh") {
      refreshCalls += 1;
      return jsonOk(tokens(1));
    }
    if (body.method === "identity.account.read") {
      if (authHeader(init) === "Bearer stale") return jsonErr(-32021, "stale");
      return jsonOk({ id: "ok" });
    }
    return jsonErr(-32601, "nope");
  };
  const client = new JsonRpcClient({
    endpoint: "http://test/jsonrpc",
    accessToken: "stale",
    refreshToken: "r0",
    fetch: fetchMock,
  });
  await client.refreshSession("r0");
  await client.identityAccountRead({ id: "1" });
  assert(refreshCalls === 1, "no second refresh after rotate");
}

async function main(): Promise<void> {
  await testSingleFlightRefresh();
  await testOnly32004ClearsSession();
  await testEpochDropsOrphanTicket();
  await testUsedAccessShortCircuit();
  console.log("session tests ok");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
