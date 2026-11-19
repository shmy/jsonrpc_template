# identity.account：create → login → read/search → update → delete
# PublicRpc=login/refresh；create 需任意登录且恒 privileged=false（e2e 用 seed 起步建号）。
# read/search：任意登录可查全库；update/delete 他人须 privileged；privileged 账号禁止删除。

# seed admin login（起步建号用）
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 8,
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

# victim account（供越权 update/delete 断言）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 9,
  "method": "identity.account.create",
  "params": {
    "name": "Dave",
    "phone": "13800000004",
    "password": "secret123"
  }
}
HTTP 200
[Captures]
victim_id: jsonpath "$.result.id"
[Asserts]
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "identity.account.create",
  "params": {
    "name": "Carol",
    "phone": "13800000002",
    "password": "secret123",
    "avatar": "https://example.com/carol.png",
    "remark": "e2e"
  }
}
HTTP 200
[Captures]
account_id: jsonpath "$.result.id"
[Asserts]
jsonpath "$.result.name" == "Carol"
jsonpath "$.result.phone" == "13800000002"
jsonpath "$.result.password" not exists
jsonpath "$.result.privileged" not exists
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10.5,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000002",
    "password": "secret123"
  }
}
HTTP 200
[Captures]
access_token: jsonpath "$.result.access_token"
[Asserts]
jsonpath "$.result.access_token" isString
jsonpath "$.error" not exists

# non-privileged 也可 create（结果恒非 privileged）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10.55,
  "method": "identity.account.create",
  "params": {
    "name": "Eve",
    "phone": "13800000005",
    "password": "secret123"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.phone" == "13800000005"
jsonpath "$.result.privileged" not exists
jsonpath "$.error" not exists

# read without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10.6,
  "method": "identity.account.read",
  "params": {
    "id": "{{account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists

# search without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10.7,
  "method": "identity.account.search",
  "params": {
    "phone": "13800000002"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists

# delete without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10.8,
  "method": "identity.account.delete",
  "params": {
    "id": "{{account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists

# invalid bearer + private read → -32021
POST {{base_url}}/jsonrpc
Authorization: Bearer not-a-jwt
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 10.9,
  "method": "identity.account.read",
  "params": {
    "id": "{{account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32021
jsonpath "$.result" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 11,
  "method": "identity.account.read",
  "params": {
    "id": "{{account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.id" == "{{account_id}}"
jsonpath "$.result.name" == "Carol"
jsonpath "$.result.password" not exists
jsonpath "$.result.privileged" not exists
jsonpath "$.error" not exists

# read other account → 允许（非特权也可读）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: zh-CN
{
  "jsonrpc": "2.0",
  "id": 11.2,
  "method": "identity.account.read",
  "params": {
    "id": "{{victim_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.id" == "{{victim_id}}"
jsonpath "$.result.name" == "Dave"
jsonpath "$.result.password" not exists
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 11.5,
  "method": "identity.account.search",
  "params": {
    "phone": "13800000002",
    "limit": 10
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.items" count == 1
jsonpath "$.result.items[0].id" == "{{account_id}}"
jsonpath "$.error" not exists

# search other phone → 命中（与 read 一致）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 11.6,
  "method": "identity.account.search",
  "params": {
    "phone": "13800000004"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.items" count == 1
jsonpath "$.result.items[0].id" == "{{victim_id}}"
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 12,
  "method": "identity.account.update",
  "params": {
    "id": "{{account_id}}",
    "name": "Carol Updated",
    "avatar": "https://example.com/carol-v2.png",
    "remark": "updated"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.id" == "{{account_id}}"
jsonpath "$.result.name" == "Carol Updated"
jsonpath "$.result.avatar" == "https://example.com/carol-v2.png"
jsonpath "$.result.password" not exists
jsonpath "$.error" not exists

# update 省略 avatar → 头像不变（三态：缺省 ≠ null）
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 12.1,
  "method": "identity.account.update",
  "params": {
    "id": "{{account_id}}",
    "name": "Carol NameOnly",
    "remark": "name only"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result.name" == "Carol NameOnly"
jsonpath "$.result.avatar" == "https://example.com/carol-v2.png"
jsonpath "$.error" not exists

# update without bearer → -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 12.5,
  "method": "identity.account.update",
  "params": {
    "id": "{{account_id}}",
    "name": "nope",
    "avatar": null,
    "remark": null
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32020
jsonpath "$.result" not exists

# update other account → -32009
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 12.6,
  "method": "identity.account.update",
  "params": {
    "id": "{{victim_id}}",
    "name": "nope",
    "avatar": null,
    "remark": null
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32009
jsonpath "$.result" not exists

# delete other account → -32009
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 12.7,
  "method": "identity.account.delete",
  "params": {
    "id": "{{victim_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32009
jsonpath "$.result" not exists

# batch: 公开项（错误密码，不轮换会话）+ 私有项无 Bearer → 逐条 -32003 / -32020
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
[
  {
    "jsonrpc": "2.0",
    "id": 12.8,
    "method": "identity.account.login",
    "params": {
      "phone": "13800000002",
      "password": "wrongpass"
    }
  },
  {
    "jsonrpc": "2.0",
    "id": 12.9,
    "method": "identity.account.read",
    "params": {
      "id": "{{account_id}}"
    }
  }
]
HTTP 200
[Asserts]
jsonpath "$[0].error.code" == -32003
jsonpath "$[0].result" not exists
jsonpath "$[1].error.code" == -32020
jsonpath "$[1].result" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 14,
  "method": "identity.account.delete",
  "params": {
    "id": "{{account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.result" == null
jsonpath "$.error" not exists

# access after delete is revoked
POST {{base_url}}/jsonrpc
Authorization: Bearer {{access_token}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 16,
  "method": "identity.account.update",
  "params": {
    "id": "{{account_id}}",
    "name": "ghost",
    "avatar": null,
    "remark": null
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32022
jsonpath "$.result" not exists

# seed admin 读已删账号 → -32002
POST {{base_url}}/jsonrpc
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 16.5,
  "method": "identity.account.login",
  "params": {
    "phone": "13800000777",
    "password": "seed-pass-1"
  }
}
HTTP 200
[Captures]
admin_token2: jsonpath "$.result.access_token"
[Asserts]
jsonpath "$.error" not exists

POST {{base_url}}/jsonrpc
Authorization: Bearer {{admin_token2}}
Content-Type: application/json
Accept-Language: en
{
  "jsonrpc": "2.0",
  "id": 16.6,
  "method": "identity.account.read",
  "params": {
    "id": "{{account_id}}"
  }
}
HTTP 200
[Asserts]
jsonpath "$.error.code" == -32002
jsonpath "$.error.message" == "Account not found"
jsonpath "$.result" not exists
