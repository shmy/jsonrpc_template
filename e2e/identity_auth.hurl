# identity.account.login / refresh / logout / change_password（需 Redis + JWT_SECRET）
# 固定手机号，依赖 `just e2e` 先清库；用 seed 登录拿 Bearer 再 create（任意登录均可 create）

# seed admin login
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000777",
    "password": "seed-pass-1"
  }
}
HTTP 200
[Captures]
admin_token: jsonpath "$.result.access_token"
[Asserts]
jsonpath "$.error" not exists

# create（任意登录即可；结果恒 privileged=false；此处用 seed token 起步）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "identity.account.create",
  "params": {
    "name": "Auth User",
    "phone": "13800000999",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.phone" == "13800000999"
jsonpath "$.result.password" not exists
jsonpath "$.result.privileged" not exists
jsonpath "$.error" not exists


# change_password without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1.5,
  "method": "identity.account.change_password",
  "params": {
    "old_password": "secret123",
    "new_password": "secret456"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists


# login success
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000999",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.token_type" == "Bearer"
jsonpath "$.result.access_token" isString
jsonpath "$.result.refresh_token" isString
jsonpath "$.result.expires_in" isInteger
jsonpath "$.error" not exists
[Captures]
access_token: jsonpath "$.result.access_token"
refresh_token: jsonpath "$.result.refresh_token"


# bad password → -32003
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: zh-CN
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000999",
    "password": "wrongpass"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32003
jsonpath "$.error.message" == "手机号或密码错误"
jsonpath "$.result" not exists


# logout without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.error.message" == "Access token is missing"
jsonpath "$.result" not exists


# logout with bearer
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.result" == null
jsonpath "$.error" not exists


# access after logout → revoked
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32022
jsonpath "$.result" not exists


# login again for refresh rotation
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000999",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.refresh_token" isString
jsonpath "$.error" not exists
[Captures]
refresh_token: jsonpath "$.result.refresh_token"


# refresh rotates tokens
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 8,
  "method": "identity.account.refresh",
  "params": {
    "refresh_token": "{{refresh_token}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.access_token" isString
jsonpath "$.result.refresh_token" isString
jsonpath "$.error" not exists
[Captures]
access_token: jsonpath "$.result.access_token"


# replay old refresh → -32004（并吊销当前会话；e2e 以 REFRESH_GRACE_TTL=0s 跑）
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 9,
  "method": "identity.account.refresh",
  "params": {
    "refresh_token": "{{refresh_token}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32004
jsonpath "$.result" not exists


# access from rotated session is revoked after replay
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32022
jsonpath "$.result" not exists


# malformed JWT → -32021
POST {{base_url}}/jsonrpc
Authorization: Bearer not-a-jwt
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 11,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32021
jsonpath "$.result" not exists


# empty refresh → -32602
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 12,
  "method": "identity.account.refresh",
  "params": {
    "refresh_token": ""
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32602
jsonpath "$.result" not exists


# login for change_password
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 13,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000999",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.access_token" isString
jsonpath "$.error" not exists
[Captures]
access_token: jsonpath "$.result.access_token"


# change_password success（并吊销会话）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 14,
  "method": "identity.account.change_password",
  "params": {
    "old_password": "secret123",
    "new_password": "secret456"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result" == null
jsonpath "$.error" not exists


# old access revoked after change_password
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 15,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32022
jsonpath "$.result" not exists


# login with new password
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 16,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000999",
    "password": "secret456"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.access_token" isString
jsonpath "$.error" not exists
[Captures]
access_token: jsonpath "$.result.access_token"


# change_password wrong old → -32008
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 16.5,
  "method": "identity.account.change_password",
  "params": {
    "old_password": "wrongpass1",
    "new_password": "secret789"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32008
jsonpath "$.error.message" == "Incorrect original password"
jsonpath "$.result" not exists


# change_password with same old/new → -32006
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 17,
  "method": "identity.account.change_password",
  "params": {
    "old_password": "secret456",
    "new_password": "secret456"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32006
jsonpath "$.error.message" == "New password must be different from the current one"
jsonpath "$.result" not exists
