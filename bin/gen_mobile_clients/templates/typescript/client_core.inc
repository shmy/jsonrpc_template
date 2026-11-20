  private static readonly maxNetworkRetries = 2;
  private static readonly timeoutMs = 30_000;

  private refreshInFlight: Promise<TokenResponse> | null = null;
  private sessionEpoch = 0;

  applyTokens(tokens: TokenResponse): void {
    this.accessToken = tokens.access_token;
    this.refreshToken = tokens.refresh_token;
  }

  clearTokens(): void {
    this.sessionEpoch += 1;
    this.accessToken = null;
    this.refreshToken = null;
  }

  refreshSession(refresh: string): Promise<TokenResponse> {
    if (this.refreshInFlight != null) return this.refreshInFlight;
    this.refreshToken = refresh;
    return this.refreshSingleFlight();
  }

  private async call<T>(
    method: string,
    params: unknown,
    requiresAuth: boolean,
  ): Promise<T> {
    if (requiresAuth) {
      await this.ensureAccessToken(method);
    }
    const usedAccess = this.accessToken;
    try {
      return await this.callWithNetworkRetry<T>(method, params, requiresAuth, usedAccess);
    } catch (err) {
      if (!(err instanceof JsonRpcError) || !requiresAuth || !err.isAccessAuthnFailure) {
        throw err;
      }
      if (
        this.accessToken != null &&
        this.accessToken !== "" &&
        this.accessToken !== usedAccess
      ) {
        return await this.callWithNetworkRetry<T>(
          method,
          params,
          requiresAuth,
          this.accessToken,
        );
      }
      await this.refreshSingleFlight();
      return await this.callWithNetworkRetry<T>(
        method,
        params,
        requiresAuth,
        this.accessToken,
      );
    }
  }

  private async ensureAccessToken(method: string): Promise<void> {
    if (this.accessToken != null && this.accessToken !== "") return;
    if (this.refreshToken != null && this.refreshToken !== "") {
      await this.refreshSingleFlight();
      return;
    }
    throw new JsonRpcAuthRequiredError(method);
  }

  private async callWithNetworkRetry<T>(
    method: string,
    params: unknown,
    requiresAuth: boolean,
    bearer: string | null,
  ): Promise<T> {
    let attempt = 0;
    for (;;) {
      try {
        return await this.callOnce<T>(method, params, requiresAuth, bearer);
      } catch (err) {
        if (
          err instanceof JsonRpcError ||
          err instanceof JsonRpcHttpError ||
          err instanceof JsonRpcAuthRequiredError ||
          err instanceof JsonRpcSessionExpiredError
        ) {
          throw err;
        }
        const retryable =
          err instanceof TypeError ||
          (err instanceof Error &&
            /ECONNREFUSED|ETIMEDOUT|ENOTFOUND|network|TimeoutError|aborted/i.test(
              err.message,
            ));
        if (!retryable || attempt >= JsonRpcClient.maxNetworkRetries) {
          throw err;
        }
        await new Promise((r) => setTimeout(r, 200 * 2 ** attempt));
        attempt += 1;
      }
    }
  }

  private async callOnce<T>(
    method: string,
    params: unknown,
    requiresAuth: boolean,
    bearer: string | null,
  ): Promise<T> {
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (this.acceptLanguage != null) {
      headers["Accept-Language"] = this.acceptLanguage;
    }
    if (requiresAuth && bearer != null && bearer !== "") {
      headers.Authorization = `Bearer ${bearer}`;
    }
    const payload: Record<string, unknown> = {
      jsonrpc: "2.0",
      id:
        globalThis.crypto?.randomUUID?.() ??
        `${Date.now()}-${Math.random().toString(16).slice(2)}`,
      method,
    };
    if (params !== undefined) {
      payload.params = params;
    }
    const response = await this._fetch(this.endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
      signal: AbortSignal.timeout(JsonRpcClient.timeoutMs),
    });
    const bodyText = await response.text();
    if (!response.ok) {
      throw new JsonRpcHttpError(response.status, bodyText);
    }
    const root = JSON.parse(bodyText) as {
      result?: T;
      error?: { code: number; message: string; data?: unknown };
    };
    if (root.error != null) {
      throw new JsonRpcError(root.error.code, root.error.message, root.error.data);
    }
    return root.result as T;
  }

  private refreshSingleFlight(): Promise<TokenResponse> {
    if (this.refreshInFlight != null) return this.refreshInFlight;
    let resolve!: (v: TokenResponse) => void;
    let reject!: (e: unknown) => void;
    const gate = new Promise<TokenResponse>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    this.refreshInFlight = gate;
    void this.rotateRefresh()
      .then(resolve, reject)
      .finally(() => {
        if (this.refreshInFlight === gate) {
          this.refreshInFlight = null;
        }
      });
    return gate;
  }

  private async rotateRefresh(): Promise<TokenResponse> {
    const current = this.refreshToken;
    if (current == null || current === "") {
      this.clearTokens();
      throw new JsonRpcSessionExpiredError();
    }
    const epoch = this.sessionEpoch;
    try {
      // refresh 非幂等：不做传输层自动重试
      const tokens = await this.callOnce<TokenResponse>(
        "identity.account.refresh",
        { refresh_token: current },
        false,
        null,
      );
      if (epoch !== this.sessionEpoch) {
        throw new JsonRpcSessionExpiredError();
      }
      this.applyTokens(tokens);
      return tokens;
    } catch (err) {
      if (err instanceof JsonRpcError && err.isRefreshInvalid) {
        this.clearTokens();
        throw new JsonRpcSessionExpiredError();
      }
      throw err;
    }
  }
