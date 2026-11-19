# CORS（e2e 由 just e2e 注入 CORS_ORIGINS=http://example.com）

# 无 Origin → 不带 access-control-allow-origin
GET {{base_url}}/livez
HTTP 200
[Asserts]
header "access-control-allow-origin" not exists


# 白名单 Origin → 回显
GET {{base_url}}/livez
Origin: http://example.com
HTTP 200
[Asserts]
header "access-control-allow-origin" == "http://example.com"


# 非白名单 Origin → 不回显
GET {{base_url}}/livez
Origin: http://evil.example
HTTP 200
[Asserts]
header "access-control-allow-origin" not exists


# preflight：/jsonrpc POST + content-type
OPTIONS {{base_url}}/jsonrpc
Origin: http://example.com
Access-Control-Request-Method: POST
Access-Control-Request-Headers: content-type
HTTP 200
[Asserts]
header "access-control-allow-origin" == "http://example.com"
header "access-control-allow-methods" contains "POST"
