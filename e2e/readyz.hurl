# Ready probe (DB pool acquire); body includes optional IMAGE_TAG
GET {{base_url}}/readyz
HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.image_tag" == null
