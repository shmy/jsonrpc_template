//! Generate Swift / Kotlin / Dart JSON-RPC clients from an OpenRPC JSON file.
//!
//! 默认读 `.clients/openrpc.json`（由 `just gen-clients` 拉取），不碰业务 crate。
//!
//! ```bash
//! just gen-clients
//! cargo run -p gen_mobile_clients
//! cargo run -p gen_mobile_clients -- --document path/to/openrpc.json
//! ```

mod dart;
mod emit;
mod kotlin;
mod model;
mod swift;

use clap::Parser;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "gen_mobile_clients",
    about = "Generate Swift / Kotlin / Dart clients from OpenRPC JSON"
)]
struct Args {
    /// OpenRPC JSON（默认：`<out>/openrpc.json`）
    #[arg(long)]
    document: Option<PathBuf>,

    /// 输出目录（默认：`<workspace>/.clients`）
    #[arg(long)]
    out: Option<PathBuf>,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_document(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))
}

fn rel(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn readme(server: &str) -> String {
    format!(
        r#"# Generated mobile JSON-RPC clients

由 `gen_mobile_clients` 从 OpenRPC JSON 生成（业务 crate 无改动）。

## 重新生成

```bash
just gen-clients
cargo run -p gen_mobile_clients
```

`just gen-clients`：`curl` → `.clients/openrpc.json` → Swift / Kotlin / Dart（目录已 gitignore）。

## 产物

| 文件 | 说明 |
|------|------|
| `openrpc.json` | OpenRPC 文档 |
| `swift/JsonRpcClient.swift` | `URLSession` + `Codable` |
| `kotlin/JsonRpcClient.kt` | `HttpURLConnection` + kotlinx.serialization |
| `dart/json_rpc_client.dart` | `package:http` + `dart:convert` |

默认 endpoint：`{server}`

## 用法

```swift
let client = JsonRpcClient()
let tokens = try await client.identityAccountLogin(
  IdentityAccountLoginParams(password: "admin123!", phone: "13888888888")
)
client.accessToken = tokens.access_token
```

```kotlin
val client = JsonRpcClient()
val tokens = client.identityAccountLogin(
  IdentityAccountLoginParams(password = "admin123!", phone = "13888888888")
)
client.accessToken = tokens.access_token
```

```dart
final client = JsonRpcClient();
final tokens = await client.identityAccountLogin(
  IdentityAccountLoginParams(password: 'admin123!', phone: '13888888888'),
);
client.accessToken = tokens.access_token;
```

## 注意

- `x-auth: required` 缺 `accessToken` 时本地抛错。
- update avatar 三态（省略 / null / 字符串）未建模 omit。
- Kotlin：`kotlinx-serialization-json`；Dart：`http: ^1.2.0`。
"#
    )
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let root = workspace_root()
        .canonicalize()
        .unwrap_or_else(|_| workspace_root());
    let out = args.out.unwrap_or_else(|| root.join(".clients"));
    let document = args.document.unwrap_or_else(|| out.join("openrpc.json"));

    let doc = load_document(&document).map_err(|e| {
        format!("{e}\n提示: 先 `just gen-clients`（需 api 在听），或 `--document <openrpc.json>`")
    })?;
    let ir = model::parse_document(&doc)?;

    let outputs: [(&str, String); 4] = [
        ("swift/JsonRpcClient.swift", swift::generate(&ir)),
        ("kotlin/JsonRpcClient.kt", kotlin::generate(&ir)),
        ("dart/json_rpc_client.dart", dart::generate(&ir)),
        ("README.md", readme(&ir.server)),
    ];

    println!("document {}", rel(&document, &root));
    for (rel_path, contents) in &outputs {
        let path = out.join(rel_path);
        write_file(&path, contents)?;
        println!("wrote {}", rel(&path, &root));
    }
    println!(
        "methods={} models={} server={}",
        ir.methods.len(),
        ir.models.len(),
        ir.server
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
