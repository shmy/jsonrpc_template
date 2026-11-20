//! 冒烟脚手架：`templates/` 下的原文 + 统一占位符替换后写出到 `.clients/`。

const SEED_PHONE: &str = "13888888888";
const SEED_PASSWORD: &str = "admin123!";

struct File {
    path: &'static str,
    body: &'static str,
}

/// 各语言冒烟入口 / README / 构建元数据（不含 OpenRPC 生成的客户端本体）。
const FILES: &[File] = &[
    File {
        path: "swift/main.swift",
        body: include_str!("templates/swift/main.swift"),
    },
    File {
        path: "swift/README.md",
        body: include_str!("templates/swift/README.md"),
    },
    File {
        path: "swift/.gitignore",
        body: include_str!("templates/swift/.gitignore"),
    },
    File {
        path: "dart/main.dart",
        body: include_str!("templates/dart/main.dart"),
    },
    File {
        path: "dart/pubspec.yaml",
        body: include_str!("templates/dart/pubspec.yaml"),
    },
    File {
        path: "dart/README.md",
        body: include_str!("templates/dart/README.md"),
    },
    File {
        path: "dart/.gitignore",
        body: include_str!("templates/dart/.gitignore"),
    },
    File {
        path: "typescript/main.ts",
        body: include_str!("templates/typescript/main.ts"),
    },
    File {
        path: "typescript/README.md",
        body: include_str!("templates/typescript/README.md"),
    },
    File {
        path: "kotlin/Main.kt",
        body: include_str!("templates/kotlin/Main.kt"),
    },
    File {
        path: "kotlin/build.gradle.kts",
        body: include_str!("templates/kotlin/build.gradle.kts"),
    },
    File {
        path: "kotlin/settings.gradle.kts",
        body: include_str!("templates/kotlin/settings.gradle.kts"),
    },
    File {
        path: "kotlin/README.md",
        body: include_str!("templates/kotlin/README.md"),
    },
    File {
        path: "kotlin/.gitignore",
        body: include_str!("templates/kotlin/.gitignore"),
    },
];

pub fn root_readme(server: &str) -> String {
    render(include_str!("templates/README.md"), server)
}

pub fn outputs(server: &str) -> Vec<(&'static str, String)> {
    FILES
        .iter()
        .map(|f| (f.path, render(f.body, server)))
        .collect()
}

fn render(body: &str, server: &str) -> String {
    body.replace("__SERVER__", server)
        .replace("__SEED_PHONE__", SEED_PHONE)
        .replace("__SEED_PASSWORD__", SEED_PASSWORD)
}
