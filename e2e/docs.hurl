# /docs: 自包含 OpenRPC 文档调试页（内联 viewer JS，同源加载 /openrpc.json）
GET {{base_url}}/docs
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/html"
body contains "openrpc-viewer"
