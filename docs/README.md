# docs/ —— OpenRPC Viewer（`/docs` 文档调试页）

本目录是 API 自带文档页的**全部前端资产**：零依赖、无构建步骤、三文件。

| 文件 | 角色 |
|------|------|
| `openrpc-viewer.js` | 引擎源文件（单文件 IIFE，内含 CSS 注入）。文档渲染 + 在线调试全在这里，**唯一允许手改的文件** |
| `openrpc-viewer.min.js` | 构建产物：`openrpc-viewer.js` 经 terser 压缩（`just docs-min`），`/docs` 内嵌的是它。**不要手改**；外部静态页面引用它也可省带宽 |
| `index.html` | 静态壳：`<script src="./openrpc-viewer.js" data-url="…">`。可双击/挂任意静态服务器用 |

## 与服务端的关系（关键）

`infra/http_server/lib.rs` 的 `docs_page()` 通过 **`concat!` + `include_str!`** 把
`openrpc-viewer.min.js` **内联**进一个自包含 HTML，挂在 `GET /docs`（同源、免 CORS 配置）：

```text
docs/openrpc-viewer.js  ──just docs-min（terser）──▶  openrpc-viewer.min.js  ──include_str!──▶  lib.rs::docs_page  ──▶  GET /docs
```

改 JS 后：跑 `just docs-min`（需要 node/npx，terser 钉主版本 5 保证产物稳定）。
`just dev`（cargo watch）侦测到 `min.js` 变化后会因 `include_str!` 依赖自动重编译
http_server 并重启服务；直接改源 `openrpc-viewer.js` **不会**反映到 `/docs`，
浏览器强刷（⌘⇧R，内联无缓存协商）前必须先重新生成 min.js。

整条路由受 `DOCS_ENABLED`（默认 `false`）门控：本地调试设 `DOCS_ENABLED=true`；
`just e2e` 会注入开启以便跑 `e2e/docs.hurl`。未开启时 `/docs` 不挂载（404），`/openrpc.json` 不受影响。

## 硬性约束（改 JS 前必读）

1. **JS 源文件里禁止出现字面量 `</script>`**（注释里也不行）——产物被内联进 Rust
   生成的 `<script>…</script>`，任何一处都会提前截断标签。需要示例时写 `<\/script>`。
   min.js 由 `just docs-min` 生成后自带校验（命中即报错退出）。
2. **改 `docs_page()` 的 HTML 壳时不要用 `format!`**——JS 内容里全是 `{}`；当前用
   `concat!` 拼接，保持即可。
3. **改 `openrpc-viewer.js` 后跑**：`just docs-min` → `node --check
   docs/openrpc-viewer.min.js` → `hurl --test --variable base_url=http://127.0.0.1:9000
   e2e/docs.hurl`。
4. 页面 URL 建议用 `http://127.0.0.1:9000/docs`（host 对上 `servers` 声明即同源）；
   用 `localhost` 打开而 `LISTEN_ADDR=0.0.0.0` 时，servers 里是 `127.0.0.1`，属跨源。

## viewer JS 内部结构（按段落顺序）

```text
配置        SCHEMA_URL 解析优先级：?url= > script[data-url] > 同源 /openrpc.json > localhost:9000
状态        doc / state(endpoint|token|lang|theme|id) / editorCache / respHistory / currentMethodName / standalone + localStorage 恢复
            切方法：`flushEditorCache`（靠 currentMethodName，因 hashchange 时 hash 已变）+ `renderMethod` 末尾按 respHistory 重画响应
主题        CSS 变量驱动：data-theme=auto|dark|light；亮色变量块注入两份（light + prefers-color-scheme）；独立页同步 body 背景
调试辅助    fmtTime / short / isAuthRequired(x-auth) / setToken / pushHistory(cap 8) / resolveLang(系统→en-US|zh-CN)
工具        esc / mdLite(极简 markdown) / copyText / highlightJson(JSON 语法高亮+trace_id 标黄)
schema 工具  resolveRef($ref→components) / typeLabel(数组→Item[]) / schemaRows(嵌套→a.b、数组→name[].x，深 3 层) / paramsSchemaOf(由 content descriptors 重建)
样式        injectCss()：所有主题色收在 .orv 根上的一组 CSS 变量（--bg/--accent/--ok/--err/--amber/--code-bg…）
页面骨架     mount()：standalone 判定（自动建根 or 根直挂 body）→ 重置 body margin，不碰第三方宿主
导航        tagOf / navModel / renderNav(搜索按 名称+摘要；固定入口不参与过滤) / navLink(私有方法前缀小锁 SVG)
各页面       overview / errors / component / method(参数表+重建 schema 折叠+结果表+示例+调试面板)
调试请求     编辑器为 contenteditable div（innerText 读写、粘贴强制纯文本、清空复位占位符、随内容自动撑开）；
             sendRequest(单条 or `[` 开头 batch 数组) / curlText / renderResponse(历史 tabs、trace 胶囊、设为 Bearer、复制)
路由        location.hash：#/overview | #/errors | #/method/<name> | #/component/<name>
启动        bindHeader(endpoint/token/lang/theme 四个持久化) / bindNav(点击+键盘流 / ↓↑) / boot(先 applyTheme 再 fetch)
```

## 数据契约

- **`x-auth` 扩展字段**：`http_server::merge_rpc` 收集进 `PrivateRpc` 的方法名 →
  `axum_jsonrpc_openrpc::build_document` 给这些方法写 `x-auth: "required"`。
  viewer 据此渲染侧栏锁图标 + 方法页「需登录（Bearer）」徽章。新增私有方法自动带上，无需改前端。
- **endpoint 下拉严格取自 `doc.servers`**（文档为空才回退 schema 推导）；
  与页面同源的候选项**优先选中但不新增**（`localhost` vs `127.0.0.1` 对浏览器是跨源）。
- **Accept-Language**：`auto`(默认，实时解析 navigator) | `en-US` | `zh-CN`，与服务端
  Fluent bundle 一一对应；错误 message 文案随它变。
- **localStorage 键**：`openrpc-viewer:{endpoint,token,lang,theme}`；脏值只在仍是合法
  候选时恢复。编辑器文本 / 请求历史只在内存（`editorCache` / `respHistory`），刷新页面清空。
- **切方法保留**：参数编辑器按方法名缓存；曾发送的请求切回来会重画最新响应 + history tabs。
- **改源后必须 `just docs-min`**：`/docs` 内嵌的是 `openrpc-viewer.min.js`，只改 `.js` 源不会上线。
- **`/openrpc.json` 与 `/docs` 带 `Cache-Control: no-store`**（viewer fetch 也 `cache:'no-store'`）——
  曾因浏览器缓存旧文档导致徽章不渲染，勿删。

## 历史踩坑备忘

- axum 0.8 `Handler` 要求返回 `Future`：handler 必须 `async fn`，同步 `fn() -> Response` 编译不过（`docs_page` 是 async 的原因）。
- 长字符串（JWT、UUID）在 `grid-template-columns:1fr 1fr` 的响应窗格里会把列撑爆：
  `1fr` 的隐含下限是子项 min-content，而 `overflow-wrap:break-word` **不参与**
  min-content 计算，治不了——需 `overflow-wrap:anywhere`（计入 min-content）
  + grid 子项 `min-width:0` 双管齐下（`pre.orv-code` 即此形态，勿退回 break-word）。
- 侧栏在窄屏（<900px）单列布局下不要给 aside 设 `max-height` 裁剪——点击点会被下方内容截走。
- 响应窗格 `pre` 里没有 `<code>` 元素（高亮直接输出 span），别写针对 `pre code` 的样式。
- 示例完整性由服务端保证：`axum_jsonrpc_openrpc::document.rs` 的
  `example_value_from_schema` 对无显式示例的原始类型给占位值（`""`/`0`/`false`），
  viewer 只忠实渲染文档，不前端编数据。

## 迭代 checklist

- [ ] `just docs-min` 重新生成（若改了 `openrpc-viewer.js`）
- [ ] `node --check docs/openrpc-viewer.js docs/openrpc-viewer.min.js`（后者 docs-min 已含）
- [ ] 无 `</script>` 字面量（`rg "</script" docs/openrpc-viewer*.js` 应零命中；`index.html` / 本 README 不参与内嵌，无此约束）
- [ ] 服务重编译后 ⌘⇧R 强刷实测：方法页渲染、发送请求、token 闭环、搜索、主题切换
- [ ] `hurl --test --variable base_url=… e2e/docs.hurl`
- [ ] 改到 Rust 侧时：`cargo test -p axum_jsonrpc_openrpc -p http_server` + clippy
