# identity.account.create：需登录；无 Bearer → -32020；成功后同手机号重复 → -32001
# 依赖 `just e2e` 清库 + seed admin（用其 token 建号；结果恒 privileged=false）

# create without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "identity.account.create",
  "params": {
    "name": "Alice",
    "phone": "13800000001",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists

# seed admin login
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 0.5,
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

POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "identity.account.create",
  "params": {
    "name": "Alice",
    "phone": "13800000001",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.jsonrpc" == "2.0"
jsonpath "$.id" == 1
jsonpath "$.result.name" == "Alice"
jsonpath "$.result.phone" == "13800000001"
jsonpath "$.result.id" isString
jsonpath "$.result.password" not exists
jsonpath "$.result.privileged" not exists
jsonpath "$.result.version" not exists
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token}}
Content-Type: application/json
Accept-Language: zh-CN
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "identity.account.create",
  "params": {
    "name": "Bob",
    "phone": "13800000001",
    "password": "otherpass1"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32001
jsonpath "$.error.message" == "账号已存在"
jsonpath "$.error.data.trace_id" isString
jsonpath "$.result" not exists

# 密码太短（< 8 位）→ -32602 Invalid params（Password 运行时校验）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "identity.account.create",
  "params": {
    "name": "Eve",
    "phone": "13800000003",
    "password": "short1"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32602
jsonpath "$.error.message" == "password: Password must be 8-64 characters long"
jsonpath "$.error.data.trace_id" isString
jsonpath "$.result" not exists
