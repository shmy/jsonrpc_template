export class JsonRpcError extends Error {
  readonly code: number;
  readonly data: unknown;

  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.name = "JsonRpcError";
    this.code = code;
    this.data = data;
  }

  /** access 缺失 / 无效 / 已吊销（`http_auth` `-32020..-32022`） */
  get isAccessAuthnFailure(): boolean {
    return this.code === -32020 || this.code === -32021 || this.code === -32022;
  }

  /** refresh 无效或 replay（identity `-32004`） */
  get isRefreshInvalid(): boolean {
    return this.code === -32004;
  }
}

export class JsonRpcHttpError extends Error {
  readonly status: number;
  readonly body: string;

  constructor(status: number, body: string) {
    super(`HTTP ${status}`);
    this.name = "JsonRpcHttpError";
    this.status = status;
    this.body = body;
  }
}

export class JsonRpcAuthRequiredError extends Error {
  readonly method: string;

  constructor(method: string) {
    super(`Bearer required for ${method}`);
    this.name = "JsonRpcAuthRequiredError";
    this.method = method;
  }
}

export class JsonRpcSessionExpiredError extends Error {
  constructor() {
    super("session expired; login required");
    this.name = "JsonRpcSessionExpiredError";
  }
}
