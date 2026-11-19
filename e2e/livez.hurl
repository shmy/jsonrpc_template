# Liveness probe (no dependency checks; bypasses global rate limit)
GET {{base_url}}/livez
HTTP 200
[Asserts]
body == "OK"
