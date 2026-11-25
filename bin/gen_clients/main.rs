//! Generate Swift / Kotlin / Dart / TypeScript JSON-RPC clients from OpenRPC JSON.
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
mod smoke;
mod swift;
mod typescript;

#[cfg(test)]
mod session_invariants;

use clap::Parser;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "gen_mobile_clients",
    about = "Generate Swift / Kotlin / Dart / TypeScript clients from OpenRPC JSON"
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

    let mut outputs = vec![
        ("swift/JsonRpcClient.swift", swift::generate(&ir)),
        ("kotlin/JsonRpcClient.kt", kotlin::generate(&ir)),
        ("dart/json_rpc_client.dart", dart::generate(&ir)),
        ("typescript/jsonRpcClient.ts", typescript::generate(&ir)),
        ("README.md", smoke::root_readme(&ir.server)),
    ];
    outputs.extend(smoke::outputs(&ir.server));

    println!("document {}", rel(&document, &root));
    for (rel_path, contents) in outputs {
        let path = out.join(rel_path);
        write_file(&path, &contents)?;
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
