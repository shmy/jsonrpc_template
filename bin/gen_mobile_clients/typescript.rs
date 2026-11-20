use crate::emit::{DocStyle, SourceBuf, banner, doc_comment, each_method, each_model, w};
use crate::model::Ir;

/// Property name in TS interfaces; quote when not a simple identifier.
fn ts_prop(name: &str) -> String {
    if name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        name.to_string()
    } else {
        format!("\"{name}\"")
    }
}

pub fn generate(ir: &Ir) -> String {
    let mut o = SourceBuf::default();

    banner(&mut o, ir);
    o.blank();
    o.line(RUNTIME.trim_end());
    o.blank();

    each_model(&mut o, &ir.models, DocStyle::Block, |o, model| {
        w!(o, "export interface {} {{", model.name);
        for f in &model.fields {
            doc_comment(o, &f.description, DocStyle::Block, "  ");
            w!(o, "  {}: {};", ts_prop(&f.name), f.ts_type);
        }
        w!(o, "}}");
    });

    w!(o, "export type JsonRpcClientOptions = {{");
    w!(o, "  endpoint?: string;");
    w!(o, "  accessToken?: string | null;");
    w!(o, "  acceptLanguage?: string | null;");
    w!(o, "  fetch?: typeof fetch;");
    w!(o, "}};");
    o.blank();

    w!(o, "export class JsonRpcClient {{");
    w!(o, "  endpoint: string;");
    w!(o, "  accessToken: string | null;");
    w!(o, "  acceptLanguage: string | null;");
    w!(o, "  private readonly _fetch: typeof fetch;");
    o.blank();
    w!(o, "  constructor(options: JsonRpcClientOptions = {{}}) {{");
    w!(
        o,
        "    this.endpoint = options.endpoint ?? \"{}\";",
        ir.server
    );
    w!(o, "    this.accessToken = options.accessToken ?? null;");
    w!(
        o,
        "    this.acceptLanguage = options.acceptLanguage ?? null;"
    );
    w!(
        o,
        "    this._fetch = options.fetch ?? globalThis.fetch.bind(globalThis);"
    );
    w!(o, "  }}");
    o.blank();
    o.line(CLIENT_CORE.trim_end());
    o.blank();

    each_method(&mut o, &ir.methods, DocStyle::Block, "  ", |o, m| {
        let auth = m.auth_lit();
        let (sig, ty, arg, ret) = match (&m.params_type, &m.result_type) {
            (Some(pt), Some(rt)) => (format!("params: {pt}"), rt.as_str(), "params", true),
            (Some(pt), None) => (format!("params: {pt}"), "null", "params", false),
            (None, Some(rt)) => (String::new(), rt.as_str(), "undefined", true),
            (None, None) => (String::new(), "null", "undefined", false),
        };
        let promise = if ret {
            format!("Promise<{ty}>")
        } else {
            "Promise<void>".into()
        };
        w!(o, "  async {}({sig}): {promise} {{", m.fn_name);
        if ret {
            w!(
                o,
                "    return await this.call<{ty}>(\"{}\", {arg}, {auth});",
                m.name
            );
        } else {
            w!(
                o,
                "    await this.call<{ty}>(\"{}\", {arg}, {auth});",
                m.name
            );
        }
        w!(o, "  }}");
    });

    w!(o, "}}");
    o.blank();
    o.into_string()
}

const RUNTIME: &str = r#"
export class JsonRpcError extends Error {
  readonly code: number;
  readonly data: unknown;

  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.name = "JsonRpcError";
    this.code = code;
    this.data = data;
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
"#;

const CLIENT_CORE: &str = r#"
  private async call<T>(
    method: string,
    params: unknown,
    requiresAuth: boolean,
  ): Promise<T> {
    if (requiresAuth && (this.accessToken == null || this.accessToken === "")) {
      throw new JsonRpcAuthRequiredError(method);
    }
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (this.acceptLanguage != null) {
      headers["Accept-Language"] = this.acceptLanguage;
    }
    if (this.accessToken != null) {
      headers.Authorization = `Bearer ${this.accessToken}`;
    }
    const payload: Record<string, unknown> = {
      jsonrpc: "2.0",
      id: crypto.randomUUID(),
      method,
    };
    if (params !== undefined) {
      payload.params = params;
    }
    const response = await this._fetch(this.endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
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
"#;
