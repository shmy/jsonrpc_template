# File 域：file.presign +（blob-fs）本地 PUT / 回源 + avatar commit
# 依赖：种子管理员（见 seed_admin）；api 需启用 blob-fs（just e2e 已带）

# 管理员登录
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000777",
    "password": "seed-pass-1"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.token_type" == "Bearer"
jsonpath "$.error" not exists
[Captures]
file_admin_at: jsonpath "$.result.access_token"

# 未登录：file.presign → authn
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "file.presign",
  "params": {
    "extension": "webp"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists

# 预签名：file.presign → PUT upload_url → GET 回源
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "file.presign",
  "params": {
    "extension": "webp"
  }
}
HTTP 200
[Captures]
presign_path: jsonpath "$.result.path"
presign_upload_url: jsonpath "$.result.upload_url"
[Asserts]
jsonpath "$.error" not exists
jsonpath "$.result.path" matches /^staging\//
jsonpath "$.result.upload_url" matches /\/files\/presigned\?/
jsonpath "$.result.content_type" == "image/webp"

PUT {{presign_upload_url}}
Content-Type: image/webp
file,fixtures/one-by-one.png;
HTTP 204

GET {{base_url}}/files/{{presign_path}}
HTTP 200
[Asserts]
header "Content-Type" == "image/webp"
header "X-Content-Type-Options" == "nosniff"
header "Content-Disposition" not exists
bytes count > 0

# staging 禁止派生：?s= 忽略，仍回原图（不落 *_s*.webp）
GET {{base_url}}/files/{{presign_path}}?s=256
HTTP 200
[Asserts]
header "Content-Type" == "image/webp"
bytes count > 0

# 危险扩展名拒签
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 3.5,
  "method": "file.presign",
  "params": {
    "extension": "svg"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32032
jsonpath "$.result" not exists

# 路径守卫：非 dated / 穿越 → 404（staging URL 泄露可读是有意权衡）
GET {{base_url}}/files/secrets/key.pem
HTTP 404

GET {{base_url}}/files/../Cargo.toml
HTTP 404

# staging 归属：presign 上传者 create 时 commit；avatar 变为正式 path
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "identity.account.create",
  "params": {
    "name": "e2e-file-avatar",
    "phone": "13900009997",
    "password": "e2euser12",
    "avatar": "{{presign_path}}"
  }
}
HTTP 200
[Captures]
file_avatar_account_id: jsonpath "$.result.id"
formal_avatar: jsonpath "$.result.avatar"
[Asserts]
jsonpath "$.error" not exists
jsonpath "$.result.avatar" matches /\/files\/[0-9]{4}\/[0-9]{2}\//
jsonpath "$.result.avatar" not matches /\/staging\//

# identity.account.current 出站 avatar 须拼绝对 URL（曾漏 fill 导致相对 path）
POST {{base_url}}/jsonrpc
Content-Type: application/json
{
  "jsonrpc": "2.0",
  "id": 4.1,
  "method": "identity.account.login",
  "params": {
    "phone": "13900009997",
    "password": "e2euser12"
  }
}
HTTP 200
[Captures]
file_avatar_user_at: jsonpath "$.result.access_token"
[Asserts]
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_avatar_user_at}}
Content-Type: application/json
{
  "jsonrpc": "2.0",
  "id": 4.2,
  "method": "identity.account.current"
}
HTTP 200
[Asserts]
jsonpath "$.error" not exists
jsonpath "$.result.avatar" == "{{formal_avatar}}"
jsonpath "$.result.avatar" matches /\/files\/[0-9]{4}\/[0-9]{2}\//

# commit 后正式 path 回源（服务端已拼绝对 URL；可信扩展可内联）
GET {{formal_avatar}}
HTTP 200
[Asserts]
header "Content-Type" == "image/webp"
header "X-Content-Type-Options" == "nosniff"
header "Content-Disposition" not exists
bytes count > 0

# URL 派生：非法 s → 400
GET {{formal_avatar}}?s=999
HTTP 400

# URL 派生：首次生成 + 缓存命中（均为 image/webp + 长期 Cache-Control）
GET {{formal_avatar}}?s=256
HTTP 200
[Asserts]
header "Content-Type" == "image/webp"
header "X-Content-Type-Options" == "nosniff"
header "Cache-Control" == "public, max-age=31536000, immutable"
bytes count > 0

GET {{formal_avatar}}?s=256
HTTP 200
[Asserts]
header "Content-Type" == "image/webp"
header "Cache-Control" == "public, max-age=31536000, immutable"
bytes count > 0

# 已消费的 staging 再绑 → -32033 file_attachment_not_found
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "identity.account.create",
  "params": {
    "name": "e2e-reuse-staging",
    "phone": "13900009996",
    "password": "e2euser12",
    "avatar": "{{presign_path}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32033
jsonpath "$.result" not exists

# update 回传出站 fill_url → strip 后 no-op，库内仍为相对 key
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 4.5,
  "method": "identity.account.update",
  "params": {
    "id": "{{file_avatar_account_id}}",
    "name": "e2e-file-avatar-renamed",
    "avatar": "{{formal_avatar}}",
    "remark": null
  }
}
HTTP 200
[Asserts]
jsonpath "$.error" not exists
jsonpath "$.result.name" == "e2e-file-avatar-renamed"
jsonpath "$.result.avatar" == "{{formal_avatar}}"

# 禁止挂接他人正式 path（相对 key）→ -32032
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 5.2,
  "method": "identity.account.create",
  "params": {
    "name": "e2e-steal-formal",
    "phone": "13900009995",
    "password": "e2euser12",
    "avatar": "2026/11/not-yours.webp"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32032
jsonpath "$.result" not exists

# 本站完整 /files URL（与出站 wire 同形）不得当外链绕过归属 → -32032
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 5.3,
  "method": "identity.account.create",
  "params": {
    "name": "e2e-steal-formal-url",
    "phone": "13900009994",
    "password": "e2euser12",
    "avatar": "{{formal_avatar}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32032
jsonpath "$.result" not exists

# >2MiB PUT：大于 BODY_LIMIT(1MiB) 与 Axum 默认(~2MiB)，走 PUBLIC_BODY_LIMIT(16MiB)
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
{
  "jsonrpc": "2.0",
  "id": 5.5,
  "method": "file.presign",
  "params": {
    "extension": "mp4"
  }
}
HTTP 200
[Captures]
large_path: jsonpath "$.result.path"
large_upload_url: jsonpath "$.result.upload_url"
[Asserts]
jsonpath "$.error" not exists

PUT {{large_upload_url}}
Content-Type: video/mp4
file,fixtures/over-2m.bin;
HTTP 204

# 非图片带 ?s=：忽略派生，仍按原文件回源（MIME 保留）
GET {{base_url}}/files/{{large_path}}?s=256
HTTP 200
[Asserts]
header "Content-Type" == "video/mp4"
bytes count > 0

# 清理测试账号
POST {{base_url}}/jsonrpc
Authorization: Bearer {{file_admin_at}}
Content-Type: application/json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "identity.account.delete",
  "params": {
    "id": "{{file_avatar_account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error" not exists
