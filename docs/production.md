# 生产部署要点

模板默认把 TLS、全站限流、WAF 交给反向代理 / 网关；应用只做贵路径 method 限流与可信代理 IP 解析。

## 反代对齐

| 应用配置 | 建议反代设置 | 说明 |
|----------|--------------|------|
| `BODY_LIMIT_BYTES`（默认 1 MiB，RPC + PrivateRouter） | `client_max_body_size` ≥ 该值 | PublicRouter / 本地预签名 PUT 另用 `PUBLIC_BODY_LIMIT_BYTES`（16 MiB）；勿用更小的外层 limit 包住更大上传路由 |
| `REQUEST_TIMEOUT`（默认 60s） | `proxy_read_timeout` / `proxy_send_timeout` ≥ 该值 | 超时由应用先切断时，反代应返回上游错误而非自己抢先 504 |
| `CORS_ORIGINS` | 浏览器跨域时配置；同域反代可留空 | 空 = 不加 CORS 中间件 |
| `TRUSTED_PROXIES` | 填入反代 CIDR | 非空才解析 `X-Forwarded-For`；漏配时 login/refresh 的 Redis IP 窗会按共享出口连坐 |

nginx 示意：

```nginx
upstream jsonrpc_api {
    server 127.0.0.1:8080;
}

server {
    listen 443 ssl http2;
    # ssl_certificate …;

    client_max_body_size 16m;
    proxy_read_timeout 60s;
    proxy_send_timeout 60s;

    location / {
        proxy_pass http://jsonrpc_api;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

对应 `.env` 片段：`TRUSTED_PROXIES=10.0.0.0/8`（按实际反代网段改）。

## 限流

- **全站 IP / 路径限流**：放网关或 WAF；应用**没有**全站中间件。
- **应用内**：仅 login/refresh 的 IP 滑动窗 + login 按手机号固定窗（`RATE_LIMIT_*`）。

## 多实例

- 仅一个进程 `MASTER=true`（migrate + seed + blob GC）；其余 `MASTER=false`（默认）。
- Schema 也可不经 api：`just migration-apply`。
- 会话在 Redis；JWT 密钥与（blob-fs 时）`BLOB_FS_PRESIGN_SECRET` 全实例一致。
- **blob-fs**：对象与 `?s=` 派生缓存在本地盘——多实例须共享同一 `BLOB_FS_ROOT` 卷；否则 GC/读路径会按实例分裂。COS 无此约束。
- 非 master 的 `readyz` 只探 Db/Redis，**不**等 migration 完成；滚动发布时先让 master migrate（或 `just migration-apply`）再放流量。

## 本地依赖（compose 不含 api）

```bash
just db-up             # 仅 Postgres + Redis
# DATABASE_URL → …@127.0.0.1:5433/… ；CACHE_REDIS_URL → redis://127.0.0.1:6380
cargo run -p api
```

生产用二进制 / 编排自行部署；不要用 compose 跑 api。
