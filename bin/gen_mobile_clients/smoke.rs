//! 各语言 login 冒烟脚手架（模板在 `templates/`；`.clients` gitignore，清目录后靠生成补回）。

type Out = (&'static str, String);

pub fn outputs(server: &str) -> Vec<Out> {
    let mut out = Vec::with_capacity(16);
    out.extend(swift(server));
    out.extend(dart(server));
    out.extend(typescript(server));
    out.extend(kotlin(server));
    out
}

fn copy(path: &'static str, body: &str) -> Out {
    (path, body.to_string())
}

fn with_server(path: &'static str, body: &str, server: &str) -> Out {
    (path, body.replace("__SERVER__", server))
}

fn swift(server: &str) -> [Out; 3] {
    [
        copy(
            "swift/main.swift",
            include_str!("templates/swift/main.swift"),
        ),
        with_server(
            "swift/README.md",
            include_str!("templates/swift/README.md"),
            server,
        ),
        copy(
            "swift/.gitignore",
            include_str!("templates/swift/.gitignore"),
        ),
    ]
}

fn dart(server: &str) -> [Out; 4] {
    [
        copy("dart/main.dart", include_str!("templates/dart/main.dart")),
        copy(
            "dart/pubspec.yaml",
            include_str!("templates/dart/pubspec.yaml"),
        ),
        with_server(
            "dart/README.md",
            include_str!("templates/dart/README.md"),
            server,
        ),
        copy("dart/.gitignore", include_str!("templates/dart/.gitignore")),
    ]
}

fn typescript(server: &str) -> [Out; 2] {
    [
        copy(
            "typescript/main.ts",
            include_str!("templates/typescript/main.ts"),
        ),
        with_server(
            "typescript/README.md",
            include_str!("templates/typescript/README.md"),
            server,
        ),
    ]
}

fn kotlin(server: &str) -> [Out; 5] {
    [
        copy("kotlin/Main.kt", include_str!("templates/kotlin/Main.kt")),
        copy(
            "kotlin/build.gradle.kts",
            include_str!("templates/kotlin/build.gradle.kts"),
        ),
        copy(
            "kotlin/settings.gradle.kts",
            include_str!("templates/kotlin/settings.gradle.kts"),
        ),
        with_server(
            "kotlin/README.md",
            include_str!("templates/kotlin/README.md"),
            server,
        ),
        copy(
            "kotlin/.gitignore",
            include_str!("templates/kotlin/.gitignore"),
        ),
    ]
}
