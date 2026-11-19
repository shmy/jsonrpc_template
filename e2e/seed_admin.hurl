# MASTER=true 启动时种子管理员（SEED_ADMIN_*，由 just e2e 注入 13800000777 / seed-pass-1）
# 依赖 `just e2e` 先清库并以 master 启动
# privileged 可 update/delete 他人；read/search 任意登录可查全库；create 任意登录即可（结果非 privileged）

# seed admin can login
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
seed_token: jsonpath "$.result.access_token"

# privileged creates victim
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1.4,
  "method": "identity.account.create",
  "params": {
    "name": "SeedVictim",
    "phone": "13800000778",
    "password": "secret123"
  }
}
HTTP 200
[Captures]
victim_id: jsonpath "$.result.id"
[Asserts]
jsonpath "$.error" not exists

# privileged can search other phone
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1.5,
  "method": "identity.account.search",
  "params": {
    "phone": "13800000778"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.items" count == 1
jsonpath "$.result.items[0].id" == "{{victim_id}}"
jsonpath "$.error" not exists

# privileged can update other account
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1.6,
  "method": "identity.account.update",
  "params": {
    "id": "{{victim_id}}",
    "name": "SeedVictim Updated",
    "avatar": null,
    "remark": "by-admin"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.id" == "{{victim_id}}"
jsonpath "$.result.name" == "SeedVictim Updated"
jsonpath "$.error" not exists

# privileged can delete non-privileged other
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1.7,
  "method": "identity.account.delete",
  "params": {
    "id": "{{victim_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result" == null
jsonpath "$.error" not exists

# resolve seed admin id（search 自己手机号）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 1.8,
  "method": "identity.account.search",
  "params": {
    "phone": "13800000777"
  }
}
HTTP 200
[Captures]
admin_id: jsonpath "$.result.items[0].id"
[Asserts]
jsonpath "$.result.items" count == 1
jsonpath "$.error" not exists

# privileged 账号禁止删除（含自删）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: zh-CN
{
  "jsonrpc": "2.0",
  "id": 1.9,
  "method": "identity.account.delete",
  "params": {
    "id": "{{admin_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32009
jsonpath "$.error.message" == "无权操作该账号"
jsonpath "$.result" not exists

# seed admin session is a normal private-rpc session (logout works, revokes)
POST {{base_url}}/jsonrpc
Authorization: Bearer {{seed_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "identity.account.logout"
}
HTTP 200
[Asserts]
jsonpath "$.result" == null
jsonpath "$.error" not exists
