//! OpenRPC 文档页与 servers URL。

use std::net::{IpAddr, SocketAddr};

use axum::http::header;
use axum::response::{IntoResponse, Response};

/// OpenRPC `servers` URL：由监听地址生成。绑定到未指定 IP（`0.0.0.0` / `::`）时
/// 文档回退环回地址，保证宣称的 URL 客户端实际可达。
pub(crate) fn openrpc_url(addr: SocketAddr) -> String {
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            format!("http://127.0.0.1:{}/jsonrpc", addr.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => format!("http://[::1]:{}/jsonrpc", addr.port()),
        _ => format!("http://{addr}/jsonrpc"),
    }
}

/// `GET /docs`：自包含的 OpenRPC 文档调试页。
///
/// 页面内嵌 `docs/openrpc-viewer.min.js`（`docs/openrpc-viewer.js` 经 `just
/// docs-min`（terser）压缩生成的零依赖单文件 JS），同源读取 `/openrpc.json`
/// 渲染方法文档并可直接向 `/jsonrpc` 发调试请求——默认 `CORS_ORIGINS` 为空时
/// 外部静态页面会被浏览器跨域拦截，同源托管免任何 CORS 配置。
/// **改 viewer 逻辑请改源文件后跑 `just docs-min` 再强刷**，勿手改 min.js。
pub(crate) async fn docs_page() -> Response {
    const DOCS_HTML: &str = concat!(
        "<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        "<title>JSON-RPC API 文档</title></head><body>",
        "<div id=\"openrpc-viewer\"></div><script id=\"openrpc-viewer\">",
        include_str!("../../docs/openrpc-viewer.min.js"),
        "</script></body></html>"
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            // Inline shell embeds the viewer JS at build time; caches would hide JS edits.
            (header::CACHE_CONTROL, "no-store"),
        ],
        DOCS_HTML,
    )
        .into_response()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod openrpc_url_tests {
    use super::openrpc_url;
    use std::net::SocketAddr;
    use std::str::FromStr;

    #[test]
    fn follows_listen_addr() {
        assert_eq!(
            openrpc_url(SocketAddr::from_str("127.0.0.1:8080").unwrap()),
            "http://127.0.0.1:8080/jsonrpc"
        );
        assert_eq!(
            openrpc_url(SocketAddr::from_str("192.168.1.5:9000").unwrap()),
            "http://192.168.1.5:9000/jsonrpc"
        );
    }

    #[test]
    fn unspecified_ip_falls_back_to_loopback() {
        assert_eq!(
            openrpc_url(SocketAddr::from_str("0.0.0.0:8080").unwrap()),
            "http://127.0.0.1:8080/jsonrpc"
        );
        assert_eq!(
            openrpc_url(SocketAddr::from_str("[::]:8080").unwrap()),
            "http://[::1]:8080/jsonrpc"
        );
    }
}
