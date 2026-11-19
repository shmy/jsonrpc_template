/**
 * OpenRPC Viewer —— 引入即渲染的 JSON-RPC 文档 + 调试页（零依赖，单文件）
 *
 * 用法：
 *   <div id="openrpc-viewer"><\/div>
 *   <script src="openrpc-viewer.js" data-url="http://localhost:9000/openrpc.json"><\/script>
 *
 * 或由 API 服务同源托管（本仓库 `GET /docs` 即此形态，免 CORS 配置）。
 *
 * schema 地址优先级：`?url=` 查询参数 > script[data-url] > 同源 /openrpc.json
 *   > http://localhost:9000/openrpc.json
 *
 * 调试：endpoint 严格取自文档 `servers`（与页面同源的项优先选中）；login/refresh
 * 响应可一键「设为 Bearer」；编辑器粘贴 `[` 开头的请求对象数组即 batch；`/` 聚焦搜索。
 */
(function () {
  'use strict';

  // ---------------- 配置 ----------------
  var qs = new URLSearchParams(location.search);
  var dataUrl = document.currentScript ? document.currentScript.getAttribute('data-url') : null;

  function defaultSchemaUrl() {
    if (dataUrl) return dataUrl;
    if (location.protocol === 'http:' || location.protocol === 'https:') {
      return location.origin + '/openrpc.json';
    }
    return 'http://localhost:9000/openrpc.json';
  }

  var SCHEMA_URL = qs.get('url') || defaultSchemaUrl();
  var LS = 'openrpc-viewer';

  // ---------------- 状态 ----------------
  var doc = null;
  var state = {
    endpoint: '',
    token: '',
    lang: 'auto',
    theme: 'auto',
    id: Math.floor(Math.random() * 1e6)
  };
  var editorCache = {}; // method name -> 编辑器文本，切页不丢
  var respHistory = {}; // method name -> 响应历史（最新在前，cap 8）
  var currentMethodName = null; // 当前方法页（hashchange 时 hash 已变，靠它刷缓存）
  var standalone = false;

  try {
    state.endpoint = localStorage.getItem(LS + ':endpoint') || '';
    state.token = localStorage.getItem(LS + ':token') || '';
    var savedLang = localStorage.getItem(LS + ':lang');
    if (savedLang === 'auto' || savedLang === 'en-US' || savedLang === 'zh-CN') {
      state.lang = savedLang;
    }
    var savedTheme = localStorage.getItem(LS + ':theme');
    if (savedTheme === 'auto' || savedTheme === 'dark' || savedTheme === 'light') {
      state.theme = savedTheme;
    }
  } catch (e) { /* 隐私模式等 localStorage 不可用 */ }

  // ---------------- 主题 ----------------
  var mediaLight = window.matchMedia
    ? window.matchMedia('(prefers-color-scheme: light)')
    : null;

  function isLightResolved() {
    if (state.theme === 'light') return true;
    if (state.theme === 'dark') return false;
    return !!(mediaLight && mediaLight.matches);
  }

  function applyTheme() {
    if (!root) return;
    root.dataset.theme = state.theme;
    if (standalone) {
      document.body.style.background = isLightResolved() ? '#f7f8fa' : '#0b0e16';
    }
  }

  // ---------------- 调试辅助 ----------------
  function fmtTime(ts) {
    var d = new Date(ts);
    function p(n) { return (n < 10 ? '0' : '') + n; }
    return p(d.getHours()) + ':' + p(d.getMinutes()) + ':' + p(d.getSeconds());
  }

  function short(s, n) {
    s = String(s);
    return s.length > n ? s.slice(0, n) + '…' : s;
  }

  function isAuthRequired(m) {
    return !!(m && m['x-auth'] === 'required');
  }

  function setToken(value) {
    state.token = value;
    var input = root.querySelector('#orv-token');
    if (input) input.value = value;
    try { localStorage.setItem(LS + ':token', value); } catch (e) { /* ignore */ }
  }

  function pushHistory(name, entry) {
    var list = respHistory[name] || (respHistory[name] = []);
    list.unshift(entry);
    if (list.length > 8) list.length = 8;
  }

  // Accept-Language「跟随系统」选项的实时解析：en* → en-US，其余回退 zh-CN（与页面文案一致）。
  function resolveLang() {
    var nav = (navigator.languages && navigator.languages[0]) || navigator.language || '';
    return String(nav).toLowerCase().indexOf('en') === 0 ? 'en-US' : 'zh-CN';
  }

  // ---------------- 工具 ----------------
  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  // 极简 markdown：段落 / **粗体** / `代码`
  function mdLite(text) {
    if (!text) return '';
    return String(text)
      .split(/\n\n+/)
      .map(function (para) {
        return (
          '<p>' +
          esc(para)
            .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
            .replace(/`([^`]+?)`/g, '<code>$1</code>')
            .replace(/\n/g, '<br>') +
          '</p>'
        );
      })
      .join('');
  }

  function copyText(text, btn) {
    var restore = function () {
      if (!btn) return;
      var old = btn.textContent;
      btn.textContent = '已复制';
      setTimeout(function () { btn.textContent = old; }, 1200);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(restore, function () { legacyCopy(text); restore(); });
    } else {
      legacyCopy(text);
      restore();
    }
  }

  function legacyCopy(text) {
    var ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand('copy'); } catch (e) { /* ignore */ }
    document.body.removeChild(ta);
  }

  // JSON 语法高亮（先转义 & < >，保留引号便于正则；文本节点中裸引号安全）
  function highlightJson(value) {
    var text = value === undefined ? 'null' : JSON.stringify(value, null, 2);
    if (typeof text !== 'string') text = String(text);
    text = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    return text.replace(
      /("(?:\\u[\da-fA-F]{4}|\\[^u]|[^\\"])*"(?:\s*:)?|\b(?:true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g,
      function (match) {
        var cls = 'orv-t-num';
        if (match.charAt(0) === '"') {
          cls = /:\s*$/.test(match) ? 'orv-t-key' : 'orv-t-str';
          if (match.indexOf('"trace_id"') === 0) cls += ' orv-t-trace';
        } else if (match === 'true' || match === 'false') cls = 'orv-t-bool';
        else if (match === 'null') cls = 'orv-t-null';
        return '<span class="' + cls + '">' + match + '</span>';
      }
    );
  }

  // ---------------- schema 工具 ----------------
  function resolveRef(schema) {
    if (!schema || typeof schema !== 'object' || !schema.$ref) return schema;
    var name = String(schema.$ref).replace(/^#\/components\/schemas\//, '')
      .replace(/^#\/(?:definitions|\$defs)\//, '');
    var comps = (doc && doc.components && doc.components.schemas) || {};
    return comps[name] || schema;
  }

  function refName(ref) {
    return String(ref || '').replace(/^#\/components\/schemas\//, '')
      .replace(/^#\/(?:definitions|\$defs)\//, '') || 'ref';
  }

  function typeLabel(schema) {
    if (!schema || typeof schema !== 'object') return 'any';
    if (schema.$ref) return refName(schema.$ref);
    var isArr = schema.type === 'array' ||
      (Array.isArray(schema.type) && schema.type.indexOf('array') >= 0);
    if (isArr) return schema.items ? typeLabel(schema.items) + '[]' : 'array';
    var t = Array.isArray(schema.type) ? schema.type.join(' | ') : schema.type;
    if (t) return t + (schema.format ? ':' + schema.format : '');
    if (schema.properties) return 'object';
    if (schema.enum) return 'enum';
    return 'any';
  }

  // 由 OpenRPC content descriptors 重建完整参数 schema（文档按参数拆开存储）：
  // by-name → {object, properties, required}；by-position → {array, prefixItems}。
  function paramsSchemaOf(method) {
    if (!method.params.length) return { type: 'null' };
    if (method.paramStructure === 'by-position') {
      return {
        type: 'array',
        prefixItems: method.params.map(function (p) { return p.schema; })
      };
    }
    var props = {};
    var required = [];
    method.params.forEach(function (p) {
      props[p.name] = p.schema;
      if (p.required) required.push(p.name);
    });
    var schema = { type: 'object', properties: props };
    if (required.length) schema.required = required;
    return schema;
  }

  // 把 schema 展开成表格行：嵌套对象用点分路径、数组元素用 name[]，深度限制 3 层。
  function schemaRows(schema, prefix, depth, rows) {
    schema = resolveRef(schema);
    if (!schema || typeof schema !== 'object' || depth > 3) return rows;
    var props = schema.properties;
    if (!props) {
      var isArr = schema.type === 'array' ||
        (Array.isArray(schema.type) && schema.type.indexOf('array') >= 0);
      if (isArr && schema.items) {
        return schemaRows(schema.items, prefix ? prefix + '[].' : '[].', depth + 1, rows);
      }
      return rows;
    }
    var required = {};
    (schema.required || []).forEach(function (r) { required[r] = true; });
    Object.keys(props).forEach(function (key) {
      var prop = props[key];
      var resolved = resolveRef(prop);
      var name = (prefix || '') + key;
      var desc = resolved.description || prop.description || '';
      var childObj = resolved.properties ? resolved : null;
      var childArr = resolved.items && resolveRef(resolved.items).properties ? resolved : null;
      rows.push({
        name: name,
        type: typeLabel(prop),
        required: !!required[key],
        desc: desc
      });
      if (childObj) {
        schemaRows(childObj, name + '.', depth + 1, rows);
      } else if (childArr) {
        schemaRows(resolveRef(resolved.items), name + '[].', depth + 1, rows);
      }
    });
    return rows;
  }

  // 从 schema 递归生成示例值（examples/example/default/enum/const 优先，其次按类型兜底）
  function schemaDefault(schema, depth) {
    depth = depth || 0;
    schema = resolveRef(schema);
    if (!schema || typeof schema !== 'object' || depth > 6) return null;
    if (Array.isArray(schema.examples) && schema.examples.length) return schema.examples[0];
    if ('example' in schema) return schema.example;
    if ('default' in schema && schema.default !== undefined) return schema.default;
    if (schema.const !== undefined) return schema.const;
    if (Array.isArray(schema.enum) && schema.enum.length) return schema.enum[0];
    var t = Array.isArray(schema.type) ? schema.type[0] : schema.type;
    if (schema.properties) {
      var out = {};
      for (var key in schema.properties) out[key] = schemaDefault(schema.properties[key], depth + 1);
      return out;
    }
    switch (t) {
      case 'string': return '';
      case 'integer':
      case 'number': return 0;
      case 'boolean': return false;
      case 'array': return [];
      case 'null': return null;
      default: return null;
    }
  }

  // 取某方法的默认调试参数（优先用 OpenRPC examples，再按 schema 兜底）
  function defaultParams(method) {
    if (!method.params || !method.params.length) return '';
    var example = (method.examples || []).filter(function (e) { return e.params && e.params.length; })[0];
    var byName = {};
    if (example) {
      example.params.forEach(function (p) { byName[p.name] = p.value; });
    }
    var positional = method.paramStructure === 'by-position';
    var values = method.params.map(function (p) {
      return (p.name in byName) ? byName[p.name] : schemaDefault(p.schema);
    });
    return JSON.stringify(positional ? values : zipObject(method.params, values), null, 2);
  }

  function zipObject(params, values) {
    var obj = {};
    params.forEach(function (p, i) { obj[p.name] = values[i]; });
    return obj;
  }

  // ---------------- LLM 导出 ----------------
  // 把方法（或整份 API）组合成自包含 Markdown：LLM 不需追问即可正确发请求。
  function mdEscapeCell(s) {
    return String(s || '').replace(/\|/g, '\\|').replace(/`/g, "'").replace(/\n+/g, ' ');
  }

  function mdExampleRequest(method) {
    var req = { jsonrpc: '2.0', id: 1, method: method.name };
    var defaults = defaultParams(method);
    if (defaults) req.params = JSON.parse(defaults);
    return req;
  }

  function mdExampleResponse(method) {
    var exResult = (method.examples || [])[0];
    exResult = exResult && exResult.result ? exResult.result.value
      : schemaDefault(method.result && method.result.schema);
    return { jsonrpc: '2.0', id: 1, result: exResult === undefined ? null : exResult };
  }

  function methodMarkdown(method) {
    var lines = [];
    lines.push('## ' + method.name);
    if (method.summary) lines.push('', method.summary);
    if (method.description) lines.push('', method.description);
    lines.push(
      '',
      '- **Endpoint**: `POST ' + state.endpoint + '`（JSON-RPC 2.0）',
      '- **Auth**: ' + (isAuthRequired(method)
        ? '需要 `Authorization: Bearer <access_token>` 请求头'
        : '匿名可调用')
    );

    lines.push('', '### Params', '');
    if (method.params.length) {
      lines.push('| Name | Type | Required | Description |', '|---|---|---|---|');
      method.params.forEach(function (p) {
        lines.push('| `' + p.name + '` | ' + mdEscapeCell(typeLabel(p.schema)) + ' | ' +
          (p.required ? 'required' : 'optional') + ' | ' +
          mdEscapeCell(p.summary || (p.schema && p.schema.description)) + ' |');
      });
    } else {
      lines.push('无参数（请求可省略 `params` 字段）。');
    }

    lines.push('', '### Result', '');
    if (method.result) {
      var rows = schemaRows(method.result.schema, '', 0, []);
      if (rows.length) {
        lines.push('| Field | Type | Required | Description |', '|---|---|---|---|');
        rows.forEach(function (r) {
          lines.push('| `' + r.name + '` | ' + mdEscapeCell(r.type) + ' | ' +
            (r.required ? 'required' : 'optional') + ' | ' + mdEscapeCell(r.desc) + ' |');
        });
      } else {
        lines.push(typeLabel(method.result.schema) === 'null'
          ? '无返回值（`result` 为 `null`）。'
          : '类型：`' + typeLabel(method.result.schema) + '`。');
      }
    } else {
      lines.push('无返回值（`result` 为 `null`）。');
    }

    lines.push('', '### Example', '', '请求：', '```json',
      JSON.stringify(mdExampleRequest(method), null, 2), '```', '', '响应：', '```json',
      JSON.stringify(mdExampleResponse(method), null, 2), '```');
    return lines.join('\n');
  }

  function allMethodsMarkdown() {
    var parts = [];
    parts.push('# ' + ((doc.info && doc.info.title) || 'JSON-RPC API') +
      '（v' + ((doc.info && doc.info.version) || '?') + '）');
    if (doc.info && doc.info.description) parts.push('', doc.info.description);
    parts.push(
      '',
      '- **Transport**: JSON-RPC 2.0 over HTTP，`POST ' + state.endpoint + '`',
      '- **Envelope**: `{ "jsonrpc": "2.0", "id": <number>, "method": "<name>", "params": <object> }`',
      '- **Auth**: 标注需要登录的方法必须带 `Authorization: Bearer <access_token>`；其余匿名。',
      '- **Errors**: 协议层错误 `-32600..-32603`；业务错误为 `-320xx` 稳定码段，见各方法描述。'
    );
    doc.methods.forEach(function (m) {
      parts.push('', '---', '', methodMarkdown(m));
    });
    return parts.join('\n');
  }

  // ---------------- 样式 ----------------
  function injectCss() {
    var lightVars =
      '--bg:#f7f8fa;--bg-soft:#f2f4f7;--panel:#ffffff;--fg:#1f2430;--muted:#687186;' +
      '--desc-fg:#3f4757;--muted-strong:#4b5563;--inline-code-fg:#1f2430;' +
      '--border:rgba(15,23,42,.12);--border-soft:rgba(15,23,42,.08);' +
      '--accent:#4f5bd5;--accent-2:#8b5cf6;--accent-soft:rgba(79,91,213,.10);' +
      '--ok:#059669;--ok-soft:rgba(5,150,105,.12);--err:#dc2626;--err-soft:rgba(220,38,36,.10);' +
      '--amber:#b45309;--amber-soft:rgba(180,83,9,.10);' +
      '--header-bg:rgba(247,248,250,.82);color-scheme:light;';
    var css = [
      '.orv{--bg:#0b0e16;--bg-soft:#0e1220;--panel:#121624;--fg:#e7eaf3;--muted:#8a93ab;',
      '--border:rgba(148,163,184,.16);--border-soft:rgba(148,163,184,.09);',
      '--accent:#7c8cff;--accent-2:#a78bfa;--accent-soft:rgba(124,140,255,.13);',
      '--ok:#34d399;--ok-soft:rgba(52,211,153,.13);--err:#fb7185;--err-soft:rgba(251,113,133,.13);',
      '--amber:#fbbf24;--amber-soft:rgba(251,191,36,.12);--code-bg:#0d1020;',
      '--header-bg:rgba(11,14,22,.75);--desc-fg:#c3cadb;--muted-strong:#aeb6c9;',
      '--inline-code-fg:#dfe4f2;',
      'color-scheme:dark;color:var(--fg);min-height:100vh;',
      'font:14px/1.65 system-ui,-apple-system,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;',
      'background:',
      'radial-gradient(1100px 480px at 85% -10%,rgba(124,140,255,.09),transparent 60%),',
      'radial-gradient(900px 420px at -5% 0%,rgba(167,139,250,.07),transparent 55%),',
      'var(--bg)}',
      '.orv ::selection{background:rgba(124,140,255,.35)}',
      '.orv *{box-sizing:border-box}',
      '.orv code,.orv pre,.orv .orv-mono{font-family:ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace}',
      '.orv p{margin:.4em 0}',
      '.orv h2{font-size:22px;letter-spacing:-.02em;font-weight:650;margin:0 0 6px}',
      '.orv h3{font-size:15px;font-weight:600;margin:26px 0 10px}',
      '.orv .orv-header{display:flex;flex-wrap:wrap;align-items:center;gap:18px;padding:14px 28px;',
      'position:sticky;top:0;z-index:20;border-bottom:1px solid var(--border-soft);',
      'background:var(--header-bg);backdrop-filter:blur(14px);-webkit-backdrop-filter:blur(14px)}',
      '.orv .orv-brand{display:flex;align-items:center;gap:12px;margin-right:auto;min-width:0}',
      '.orv .orv-brand h1{font-size:16px;font-weight:600;letter-spacing:-.01em;margin:0;',
      'white-space:nowrap;overflow:hidden;text-overflow:ellipsis}',
      '.orv .orv-logo{font-size:20px;background:linear-gradient(135deg,var(--accent),var(--accent-2));',
      '-webkit-background-clip:text;background-clip:text;color:transparent}',
      '.orv .orv-meta{font-size:12px;color:var(--muted);margin-top:1px}',
      '.orv .orv-controls{display:flex;flex-wrap:wrap;align-items:flex-end;gap:10px;',
      'font-size:11.5px;color:var(--muted);letter-spacing:.02em}',
      '.orv .orv-controls label{display:flex;flex-direction:column;gap:4px}',
      '.orv .orv-controls input,.orv .orv-controls select{background:var(--panel);',
      'border:1px solid var(--border);border-radius:8px;padding:6px 10px;font-size:13px;',
      'color:var(--fg);min-width:0;transition:border-color .15s,box-shadow .15s}',
      '.orv .orv-controls input:focus,.orv .orv-controls select:focus{outline:none;',
      'border-color:var(--accent);box-shadow:0 0 0 3px rgba(124,140,255,.18)}',
      '.orv #orv-endpoint{width:250px}.orv #orv-token{width:190px}',
      '.orv .orv-schema-link{color:var(--accent);font-size:12px;text-decoration:none;',
      'align-self:center;opacity:.85;transition:opacity .12s}',
      '.orv .orv-schema-link:hover{opacity:1;text-decoration:underline}',
      '.orv .orv-layout{display:grid;grid-template-columns:300px minmax(0,1fr);min-height:calc(100vh - 64px)}',
      '.orv .orv-aside{border-right:1px solid var(--border-soft);padding:18px 14px;',
      'position:sticky;top:64px;height:calc(100vh - 64px);overflow-y:auto;',
      'scrollbar-width:thin;scrollbar-color:rgba(148,163,184,.3) transparent}',
      '.orv #orv-search{width:100%;background:var(--panel);border:1px solid var(--border);',
      'border-radius:9px;padding:8px 12px;font-size:13px;color:var(--fg);margin-bottom:14px;',
      'transition:border-color .15s,box-shadow .15s}',
      '.orv #orv-search::placeholder{color:var(--muted);opacity:.7}',
      '.orv #orv-search:focus{outline:none;border-color:var(--accent);box-shadow:0 0 0 3px rgba(124,140,255,.18)}',
      '.orv .orv-nav-group{font-size:10.5px;font-weight:600;text-transform:uppercase;letter-spacing:.12em;',
      'color:var(--muted);margin:18px 0 6px;padding:0 10px}',
      '.orv .orv-nav-item{display:block;width:100%;text-align:left;border:0;background:none;cursor:pointer;',
      'color:var(--fg);font:inherit;font-size:13px;padding:7px 10px;border-radius:8px;word-break:break-all;',
      'position:relative;transition:background .12s,color .12s}',
      '.orv .orv-nav-item .orv-mono{font-size:12.5px}',
      '.orv .orv-nav-item:hover{background:rgba(148,163,184,.09)}',
      '.orv .orv-nav-item.orv-active{background:var(--accent-soft);color:var(--accent);font-weight:600}',
      '.orv .orv-nav-item.orv-active::before{content:"";position:absolute;left:-14px;top:22%;bottom:22%;',
      'width:3px;border-radius:2px;background:linear-gradient(180deg,var(--accent),var(--accent-2))}',
      '.orv .orv-nav-item.orv-deprecated{text-decoration:line-through;color:var(--muted)}',
      '.orv .orv-nav-sub{display:block;font-size:11px;line-height:1.35;color:var(--muted);',
      'margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;transition:color .12s}',
      '.orv .orv-nav-item:hover .orv-nav-sub{color:var(--muted-strong)}',
      '.orv .orv-auth-icon{width:11px;height:11px;margin-right:6px;vertical-align:middle;',
      'color:var(--amber);flex:none}',
      '.orv .orv-badge-auth{background:var(--amber-soft);color:var(--amber)}',
      '.orv .orv-resp-tabs{display:flex;gap:6px;flex-wrap:wrap;margin-top:10px}',
      '.orv .orv-resp-tab{border:1px solid var(--border-soft);background:transparent;color:var(--muted);',
      'border-radius:99px;padding:2px 10px;font:11px ui-monospace,Menlo,monospace;cursor:pointer;',
      'transition:color .12s,border-color .12s}',
      '.orv .orv-resp-tab:hover{color:var(--fg);border-color:var(--border)}',
      '.orv .orv-resp-tab-active,.orv .orv-resp-tab-active:hover{color:var(--accent);',
      'border-color:var(--accent);background:var(--accent-soft)}',
      '.orv .orv-pane-head{display:flex;align-items:center;justify-content:space-between;margin:8px 0 4px}',
      '.orv .orv-pane-head .orv-sec-label{margin:0}',
      '.orv .orv-btn-mini{border:1px solid var(--border);background:transparent;color:var(--muted);',
      'border-radius:6px;font-size:10.5px;padding:2px 8px;cursor:pointer;',
      'transition:color .12s,border-color .12s}',
      '.orv .orv-btn-mini:hover{color:var(--accent);border-color:var(--accent)}',
      '.orv .orv-main{padding:34px 44px 80px;max-width:920px;min-width:0}',
      '.orv .orv-summary{color:var(--fg);font-weight:600;font-size:15px;margin:2px 0 8px}',
      '.orv .orv-desc{color:var(--desc-fg);margin:10px 0;font-size:13.5px}',
      '.orv code{background:rgba(148,163,184,.13);padding:.1em .45em;border-radius:5px;',
      'font-size:.92em;color:var(--inline-code-fg)}',
      '.orv .orv-badge{display:inline-block;font-size:11px;font-weight:500;padding:2px 9px;border-radius:99px;',
      'background:rgba(148,163,184,.1);color:var(--muted);margin:0 6px 6px 0;vertical-align:middle}',
      '.orv .orv-badge.orv-bad{background:var(--err-soft);color:var(--err)}',
      '.orv .orv-badge.orv-required{background:var(--err-soft);color:var(--err)}',
      '.orv table{border-collapse:collapse;width:100%;margin:10px 0;font-size:13px}',
      '.orv th,.orv td{padding:8px 12px;text-align:left;vertical-align:top;',
      'border-bottom:1px solid var(--border-soft)}',
      '.orv th{font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:.06em;',
      'color:var(--muted);background:rgba(148,163,184,.05)}',
      '.orv tr:last-child td{border-bottom:0}',
      '.orv details{border:1px solid var(--border-soft);background:var(--bg-soft);',
      'border-radius:10px;padding:10px 14px;margin:10px 0}',
      '.orv summary{cursor:pointer;font-size:12.5px;color:var(--muted);user-select:none;transition:color .12s}',
      '.orv summary:hover{color:var(--fg)}',
      '.orv pre.orv-code{background:var(--code-bg);border:1px solid var(--border-soft);color:#dbe2f0;',
      'border-radius:12px;padding:16px;white-space:pre-wrap;overflow-wrap:anywhere;',
      'font-size:12.5px;line-height:1.6;margin:10px 0}',
      '.orv .orv-t-key{color:#82aaff}.orv .orv-t-str{color:#7ee787}',
      '.orv .orv-t-num{color:#ffab70}.orv .orv-t-bool{color:#db8ff5}.orv .orv-t-null{color:#7d8590}',
      '.orv .orv-t-trace{color:var(--amber);text-decoration:underline}',
      '.orv .orv-try{border:1px solid var(--border);border-radius:14px;padding:18px;margin-top:18px;',
      'background:linear-gradient(180deg,rgba(124,140,255,.05),transparent 45%),var(--panel)}',
      '.orv .orv-try-head{display:flex;align-items:center;justify-content:space-between;margin-bottom:12px}',
      '.orv .orv-try-title{font-size:11px;font-weight:700;color:var(--muted);text-transform:uppercase;',
      'letter-spacing:.12em;margin-bottom:0}',
      '.orv .orv-try .orv-editor{width:100%;min-height:96px;max-height:60vh;overflow:auto;background:var(--code-bg);',
      'border:1px solid var(--border-soft);border-radius:10px;padding:12px;font-size:12.5px;',
      'line-height:1.55;color:#dbe2f0;white-space:pre-wrap;overflow-wrap:break-word;caret-color:var(--accent);',
      'font-family:ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace;transition:border-color .15s,box-shadow .15s}',
      '.orv .orv-try .orv-editor:focus{outline:none;border-color:var(--accent);',
      'box-shadow:0 0 0 3px rgba(124,140,255,.15)}',
      '.orv .orv-try .orv-editor:empty::before{content:attr(data-placeholder);color:var(--muted);',
      'opacity:.7;pointer-events:none}',
      '.orv .orv-try-actions{display:flex;gap:10px;align-items:center;margin-top:12px}',
      '.orv .orv-kbd-hint{font-size:11px;color:var(--muted);margin-left:auto}',
      '.orv .orv-btn{border:0;background:linear-gradient(135deg,var(--accent),var(--accent-2));color:#fff;',
      'border-radius:9px;padding:7px 16px;font-size:13px;font-weight:600;cursor:pointer;',
      'box-shadow:0 2px 12px rgba(124,140,255,.25);',
      'transition:filter .15s,transform .12s,box-shadow .15s}',
      '.orv .orv-btn:hover{filter:brightness(1.08);transform:translateY(-1px);',
      'box-shadow:0 4px 18px rgba(124,140,255,.35)}',
      '.orv .orv-btn:active{transform:translateY(0)}',
      '.orv .orv-btn.orv-btn-ghost{background:transparent;border:1px solid var(--border);',
      'color:var(--accent);box-shadow:none;font-weight:500}',
      '.orv .orv-btn.orv-btn-ghost:hover{border-color:var(--accent);background:var(--accent-soft);',
      'box-shadow:none;transform:none}',
      '.orv .orv-btn:focus-visible,.orv .orv-nav-item:focus-visible{outline:2px solid var(--accent);outline-offset:2px}',
      '.orv .orv-resp{margin-top:16px}',
      '.orv .orv-resp-head{display:flex;align-items:center;gap:12px;flex-wrap:wrap}',
      '.orv .orv-pill{font:600 11.5px ui-monospace,Menlo,monospace;padding:3px 11px;',
      'border-radius:99px;letter-spacing:.03em}',
      '.orv .orv-pill-ok{background:var(--ok-soft);color:var(--ok)}',
      '.orv .orv-pill-err{background:var(--err-soft);color:var(--err)}',
      '.orv .orv-latency{font-size:12px;color:var(--muted);font-variant-numeric:tabular-nums}',
      '.orv .orv-trace{border:1px solid rgba(251,191,36,.35);background:var(--amber-soft);color:var(--amber);',
      'border-radius:99px;padding:3px 10px;font:11px ui-monospace,Menlo,monospace;cursor:pointer;',
      'transition:filter .12s}',
      '.orv .orv-trace:hover{filter:brightness(1.12)}',
      '.orv .orv-resp-actions{margin-left:auto;display:flex;gap:8px}',
      '.orv .orv-resp-grid{display:grid;grid-template-columns:1fr 1fr;gap:12px;margin-top:12px}',
      '.orv .orv-resp-grid>*{min-width:0}',
      '.orv .orv-sec-label{font-size:10.5px;font-weight:600;color:var(--muted);text-transform:uppercase;',
      'letter-spacing:.12em;margin:8px 4px 4px}',
      '.orv .orv-error{border:1px solid rgba(251,113,133,.35);background:var(--err-soft);color:var(--err);',
      'border-radius:10px;padding:12px 14px;margin-top:12px;font-size:13px}',
      '.orv .orv-hint{color:var(--muted);font-size:12.5px;margin-top:8px}',
      '.orv .orv-pending{color:var(--muted);font-size:13px}',
      '.orv .orv-load-error{max-width:620px;margin:90px auto;padding:0 24px;text-align:center}',
      '.orv .orv-load-error h2{color:var(--err)}',
      '.orv .orv-skeleton{padding:48px;color:var(--muted);font-size:14px}',
      '.orv ::-webkit-scrollbar{width:8px;height:8px}',
      '.orv ::-webkit-scrollbar-thumb{background:rgba(148,163,184,.28);border-radius:4px}',
      '.orv ::-webkit-scrollbar-track{background:transparent}',
      '.orv[data-theme="light"]{' + lightVars + '}',
      '@media (prefers-color-scheme: light){',
      '.orv[data-theme="auto"]{' + lightVars + '}',
      '}',
      '@media (max-width:900px){',
      '.orv .orv-layout{grid-template-columns:1fr}',
      '.orv .orv-aside{position:static;height:auto;border-right:0;border-bottom:1px solid var(--border-soft)}',
      '.orv .orv-main{padding:24px 20px 60px}',
      '.orv .orv-resp-grid{grid-template-columns:1fr}',
      '.orv #orv-endpoint{width:180px}.orv #orv-token{width:150px}',
      '}'
    ].join('');
    var style = document.createElement('style');
    style.id = 'orv-style';
    style.textContent = css;
    document.head.appendChild(style);
  }

  // ---------------- 页面骨架 ----------------
  function mount() {
    var root = document.getElementById('openrpc-viewer');
    if (!root) {
      root = document.createElement('div');
      root.id = 'openrpc-viewer';
      document.body.appendChild(root);
      standalone = true;
    } else if (root.parentElement === document.body) {
      standalone = true;
    }
    if (standalone) {
      // 独立页形态：去掉 body 默认 margin（暗色下会露白边）；背景色由 applyTheme 同步。
      // 嵌入第三方页面深处时不碰宿主样式。
      document.body.style.margin = '0';
    }
    root.className = 'orv';
    root.innerHTML =
      '<header class="orv-header">' +
      '  <div class="orv-brand"><span class="orv-logo">&#9670;</span>' +
      '    <div><h1 id="orv-title">OpenRPC</h1><div class="orv-meta" id="orv-meta"></div></div></div>' +
      '  <div class="orv-controls">' +
      '    <label>Endpoint <select id="orv-endpoint"></select></label>' +
      '    <label>Bearer Token <input id="orv-token" type="password" spellcheck="false" placeholder="私有方法需要"></label>' +
      '    <label>Accept-Language <select id="orv-lang"><option value="auto">跟随系统</option><option value="en-US">en-US</option><option value="zh-CN">zh-CN</option></select></label>' +
      '    <label>主题 <select id="orv-theme"><option value="auto">跟随系统</option><option value="dark">暗色</option><option value="light">亮色</option></select></label>' +
      '    <a class="orv-schema-link" id="orv-schema-link" target="_blank" rel="noopener">openrpc.json</a>' +
      '  </div>' +
      '</header>' +
      '<div class="orv-layout">' +
      '  <aside class="orv-aside">' +
      '    <input id="orv-search" placeholder="搜索方法…" spellcheck="false">' +
      '    <nav id="orv-nav"></nav>' +
      '  </aside>' +
      '  <main class="orv-main" id="orv-main"></main>' +
      '</div>';
    return root;
  }

  var root, navEl, mainEl, searchEl;

  // ---------------- 导航 ----------------
  function tagOf(method) {
    if (method.tags && method.tags.length) return method.tags[0].name;
    return String(method.name).split('.')[0] || 'ungrouped';
  }

  function navModel() {
    var groups = [];
    var byTag = {};
    doc.methods.forEach(function (m) {
      var tag = tagOf(m);
      if (!byTag[tag]) { byTag[tag] = []; groups.push(tag); }
      byTag[tag].push(m);
    });
    return groups.sort().map(function (tag) { return { tag: tag, methods: byTag[tag] }; });
  }

  function renderNav() {
    var filter = (searchEl.value || '').toLowerCase();
    var html = '';
    var matched = 0;

    // 固定入口（概览/标准错误码）不受搜索影响；组件与方法按 名称/摘要 过滤。
    html += navLink('#/overview', '概览');
    var schemas = (doc.components && doc.components.schemas) || {};
    var componentHtml = Object.keys(schemas).sort()
      .filter(function (name) {
        return !filter || name.toLowerCase().indexOf(filter) >= 0;
      })
      .map(function (name) {
        matched++;
        return navLink('#/component/' + encodeURIComponent(name), name);
      })
      .join('');
    if (componentHtml) {
      html += '<div class="orv-nav-group">组件</div>' + componentHtml;
    }
    if (doc.components && doc.components.errors && Object.keys(doc.components.errors).length) {
      html += '<div class="orv-nav-group">协议</div>';
      html += navLink('#/errors', '标准错误码');
    }

    navModel().forEach(function (group) {
      var items = group.methods.filter(function (m) {
        if (!filter) return true;
        return (
          m.name.toLowerCase().indexOf(filter) >= 0 ||
          (m.summary || '').toLowerCase().indexOf(filter) >= 0
        );
      });
      if (!items.length) return;
      matched += items.length;
      html += '<div class="orv-nav-group">' + esc(group.tag) + '</div>';
      items.forEach(function (m) {
        html += navLink('#/method/' + encodeURIComponent(m.name), m.name,
          m.deprecated ? 'orv-deprecated' : '', m.summary, isAuthRequired(m));
      });
    });

    if (filter && !matched) {
      html += '<div class="orv-hint" style="padding:8px">无匹配方法</div>';
    }
    navEl.innerHTML = html;
    highlightActive();
  }

  function navLink(hash, label, extraClass, sub, auth) {
    return (
      '<button class="orv-nav-item ' + (extraClass || '') + '" data-hash="' + esc(hash) + '"' +
      (auth ? ' title="需要 Bearer Token"' : '') + '>' +
            (auth
        ? '<svg class="orv-auth-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 7V5a3.5 3.5 0 0 1 7 0v2" fill="none" stroke="currentColor" stroke-width="1.8"/><rect x="3" y="7" width="10" height="6.5" rx="1.5" fill="currentColor"/></svg>'
        : '') +
      '<span class="orv-mono">' + esc(label) + '</span>' +
      (sub ? '<span class="orv-nav-sub">' + esc(sub) + '</span>' : '') +
      '</button>'
    );
  }

  function highlightActive() {
    var current = location.hash || '#/overview';
    var items = navEl.querySelectorAll('.orv-nav-item');
    items.forEach(function (el) {
      el.classList.toggle('orv-active', el.getAttribute('data-hash') === current);
    });
  }

  // ---------------- 各页面 ----------------
  function renderOverview() {
    var count = doc.methods.length;
    mainEl.innerHTML =
      '<h2>' + esc((doc.info && doc.info.title) || 'API') + '</h2>' +
      '<div class="orv-meta">' +
      '  <span class="orv-badge">OpenRPC ' + esc(doc.openrpc || '1.3.2') + '</span>' +
      '  <span class="orv-badge">version ' + esc((doc.info && doc.info.version) || '-') + '</span>' +
      '  <span class="orv-badge">' + count + ' 个方法</span>' +
      '</div>' +
      '<div class="orv-desc">' + mdLite((doc.info && doc.info.description) || '') + '</div>' +
      '<div style="margin:6px 0 18px"><button class="orv-btn orv-btn-ghost" id="orv-copy-all-llm" ' +
      'title="全部方法组合成一份 Markdown，适合放进 system prompt">Copy 全部方法 to LLM</button></div>' +
      '<h3>Servers</h3>' +
      '<table><tr><th>name</th><th>url</th></tr>' +
      ((doc.servers || []).map(function (s) {
        return '<tr><td>' + esc(s.name) + '</td><td><code>' + esc(s.url) + '</code></td></tr>';
      }).join('') || '<tr><td colspan="2">（未声明）</td></tr>') +
      '</table>' +
      '<div class="orv-hint">调试私有方法：先调用 <code>identity.account.login</code> 拿到 access token，' +
      '粘贴到右上角 Bearer Token。失败请求的 <code>trace_id</code> 可用于服务端日志检索。</div>';

    var copyAll = mainEl.querySelector('#orv-copy-all-llm');
    if (copyAll) {
      copyAll.addEventListener('click', function () {
        copyText(allMethodsMarkdown(), copyAll);
      });
    }
  }

  function renderErrors() {
    var errors = (doc.components && doc.components.errors) || {};
    var rows = Object.keys(errors).sort().map(function (name) {
      var e = errors[name];
      return '<tr><td>' + esc(name) + '</td><td><code>' + esc(e.code) + '</code></td><td>' + esc(e.message) + '</td></tr>';
    }).join('');
    mainEl.innerHTML =
      '<h2>标准错误码</h2>' +
      '<div class="orv-desc"><p>JSON-RPC 2.0 协议层错误。业务域错误为 <code>-320xx</code> 稳定码段，' +
      '响应中的 <code>error.data.trace_id</code> 可用于追踪服务端日志。</p></div>' +
      '<table><tr><th>name</th><th>code</th><th>message</th></tr>' + rows + '</table>';
  }

  function renderComponent(name) {
    var schema = doc.components && doc.components.schemas && doc.components.schemas[name];
    if (!schema) { renderNotFound(); return; }
    mainEl.innerHTML =
      '<h2 class="orv-mono">' + esc(name) + '</h2>' +
      '<div class="orv-meta"><span class="orv-badge">schema</span></div>' +
      '<div class="orv-desc">' + mdLite(schema.description || '') + '</div>' +
      '<pre class="orv-code">' + highlightJson(schema) + '</pre>';
  }

  function renderMethod(method) {
    currentMethodName = method.name;
    var required = method.params.filter(function (p) { return p.required; }).length;
    var html = '';

    html += '<h2 class="orv-mono">' + esc(method.name) + '</h2>';
    html += '<div class="orv-meta">';
    if (method.deprecated) html += '<span class="orv-badge orv-bad">deprecated</span>';
    (method.tags || []).forEach(function (t) { html += '<span class="orv-badge">' + esc(t.name) + '</span>'; });
    if (isAuthRequired(method)) {
      html += '<span class="orv-badge orv-badge-auth">需登录（Bearer）</span>';
    }
    html += '<span class="orv-badge">params: ' + esc(method.paramStructure || 'by-name') + '</span>';
    html += '<span class="orv-badge">' + method.params.length + ' 个参数' + (required ? '（' + required + ' 必填）' : '') + '</span>';
    html += '</div>';
    if (method.summary) html += '<p class="orv-summary">' + esc(method.summary) + '</p>';
    if (method.description) html += '<div class="orv-desc">' + mdLite(method.description) + '</div>';

    // 参数表
    if (method.params.length) {
      html += '<h3>参数</h3><table><tr><th>名称</th><th>类型</th><th>必填</th><th>说明</th></tr>';
      method.params.forEach(function (p) {
        var desc = p.summary || (p.schema && p.schema.description) || '';
        html += '<tr><td><code>' + esc(p.name) + '</code></td>' +
          '<td><code>' + esc(typeLabel(p.schema)) + '</code></td>' +
          '<td>' + (p.required ? '<span class="orv-badge orv-required">required</span>' : '') + '</td>' +
          '<td>' + mdLite(desc) + '</td></tr>';
      });
      html += '</table>';
      html += '<details><summary>schema（原始 JSON）</summary><pre class="orv-code">' +
        highlightJson(paramsSchemaOf(method)) + '</pre></details>';
    } else {
      html += '<h3>参数</h3><div class="orv-hint">无参数</div>';
    }

    // 结果
    html += '<h3>结果</h3>';
    if (method.result) {
      html += '<div class="orv-meta"><span class="orv-badge"><code>' + esc(typeLabel(method.result.schema)) + '</code></span></div>';
      if (method.result.summary || (method.result.schema && method.result.schema.description)) {
        html += '<div class="orv-desc">' + mdLite(method.result.summary || method.result.schema.description) + '</div>';
      }
      var resultRows = schemaRows(method.result.schema, '', 0, []);
      if (resultRows.length) {
        html += '<table><tr><th>字段</th><th>类型</th><th>必填</th><th>说明</th></tr>';
        resultRows.forEach(function (r) {
          var indent = (r.name.match(/[.\[\]]/g) || []).length;
          html += '<tr><td style="padding-left:' + (12 + indent * 14) + 'px"><code>' +
            esc(r.name) + '</code></td>' +
            '<td><code>' + esc(r.type) + '</code></td>' +
            '<td>' + (r.required ? '<span class="orv-badge orv-required">required</span>' : '') + '</td>' +
            '<td>' + mdLite(r.desc) + '</td></tr>';
        });
        html += '</table>';
      }
      html += '<details><summary>schema（原始 JSON）</summary><pre class="orv-code">' +
        highlightJson(method.result.schema) + '</pre></details>';
    } else {
      html += '<div class="orv-hint">无返回值（结果为 null）</div>';
    }

    // 示例
    if (method.examples && method.examples.length) {
      html += '<h3>示例</h3>';
      method.examples.forEach(function (ex) {
        var exParams = ex.params.map(function (p) { return p.value; });
        var paramsValue = method.paramStructure === 'by-position' ? exParams : zipObject(ex.params, exParams);
        html += '<details><summary>' + esc(ex.name) + '</summary>' +
          (ex.params && ex.params.length
            ? '<div class="orv-sec-label">params</div><pre class="orv-code">' +
              highlightJson(paramsValue) + '</pre>'
            : '') +
          (ex.result
            ? '<div class="orv-sec-label">result</div><pre class="orv-code">' +
              highlightJson(ex.result.value) + '</pre>'
            : '') +
          '</details>';
      });
    }

    // 调试面板
    html += '<div class="orv-try">' +
      '<div class="orv-try-head"><span class="orv-try-title">调试</span>' +
      '<button class="orv-btn-mini" id="orv-copy-llm" title="组合为自包含 Markdown，粘贴给 LLM 直接集成">Copy to LLM</button></div>' +
      '<div id="orv-params" class="orv-editor" contenteditable="true" spellcheck="false" ' +
      'data-placeholder="参数 JSON（留空则不带 params）"></div>' +
      '<div class="orv-try-actions">' +
      '  <button class="orv-btn" id="orv-send">发送请求</button>' +
      '  <button class="orv-btn orv-btn-ghost" id="orv-reset-params">重置参数</button>' +
      '  <span class="orv-kbd-hint">Ctrl / ⌘ + Enter 发送 · 粘贴请求对象数组即 batch</span>' +
      '</div>' +
      '<div class="orv-resp" id="orv-resp"></div>' +
      '</div>';

    mainEl.innerHTML = html;

    var editor = mainEl.querySelector('#orv-params');
    // contenteditable 编辑器：innerText 读写（渲染换行即逻辑换行）。
    // 清空时浏览器可能残留 <br> 导致占位符消失，输入事件里检测空文本就整个清掉。
    editor.innerText = (method.name in editorCache) ? editorCache[method.name] : defaultParams(method);
    editor.addEventListener('input', function () {
      if (!editor.innerText.trim()) editor.innerHTML = '';
      editorCache[method.name] = editor.innerText;
    });
    // 粘贴强制纯文本：富文本/HTML 会带进嵌套标签污染 JSON。
    editor.addEventListener('paste', function (ev) {
      ev.preventDefault();
      var text = ev.clipboardData ? ev.clipboardData.getData('text/plain') : '';
      document.execCommand('insertText', false, text);
    });

    var resetBtn = mainEl.querySelector('#orv-reset-params');
    resetBtn.addEventListener('click', function () {
      editor.innerText = defaultParams(method);
      editorCache[method.name] = editor.innerText;
    });

    var send = function () { sendRequest(method, editor); };
    mainEl.querySelector('#orv-send').addEventListener('click', send);
    editor.addEventListener('keydown', function (ev) {
      if ((ev.ctrlKey || ev.metaKey) && ev.key === 'Enter') { ev.preventDefault(); send(); }
    });
    // blur 时再刷一次：contenteditable 偶发不触发 input（IME / 程序化粘贴），切页前兜底。
    editor.addEventListener('blur', function () {
      editorCache[method.name] = editor.innerText;
    });
    mainEl.querySelector('#orv-copy-llm').addEventListener('click', function () {
      copyText(methodMarkdown(method), this);
    });

    // 切回本方法时：恢复上次的请求历史（最新一条 + tabs）。
    var out = mainEl.querySelector('#orv-resp');
    if ((respHistory[method.name] || []).length) {
      renderResponse(out, method, 0);
    }
  }

  function renderNotFound() {
    mainEl.innerHTML = '<h2>未找到</h2><div class="orv-hint">hash 路由指向的条目不存在。</div>';
  }

  // ---------------- 调试请求 ----------------
  function curlText(req, headers) {
    var lines = ["curl -X POST '" + state.endpoint + "' \\"];
    lines.push("  -H 'Content-Type: application/json'");
    if (headers['Accept-Language']) lines.push("  -H 'Accept-Language: " + headers['Accept-Language'] + "'");
    if (headers['Authorization']) lines.push("  -H 'Authorization: " + headers['Authorization'] + "'");
    lines.push("  -d '" + JSON.stringify(req) + "'");
    return lines.join(" \\\n");
  }

  function sendRequest(method, editor) {
    var out = mainEl.querySelector('#orv-resp');
    var text = (editor.innerText || '').trim();
    var headers = {
      'Content-Type': 'application/json',
      'Accept-Language': state.lang === 'auto' ? resolveLang() : state.lang
    };
    if (state.token) headers['Authorization'] = 'Bearer ' + state.token;

    var payload; // POST 出去的字符串
    var preview; // 「请求」窗格展示的 JSON 值
    if (text.charAt(0) === '[') {
      // batch 模式：编辑器内容是完整请求对象数组
      var batch;
      try {
        batch = JSON.parse(text);
      } catch (err) {
        out.innerHTML = '<div class="orv-error">batch 数组不是合法 JSON：' + esc(err.message) + '</div>';
        return;
      }
      if (!Array.isArray(batch) || !batch.length) {
        out.innerHTML = '<div class="orv-error">batch 模式需要非空的请求对象数组（以 [ 开头）</div>';
        return;
      }
      for (var i = 0; i < batch.length; i++) {
        var item = batch[i];
        if (!item || typeof item !== 'object' || Array.isArray(item) ||
          typeof item.method !== 'string') {
          out.innerHTML =
            '<div class="orv-error">batch 第 ' + (i + 1) +
            ' 个元素必须是含 method 字段的请求对象</div>';
          return;
        }
        if (item.id === undefined) item.id = ++state.id;
      }
      payload = JSON.stringify(batch);
      preview = batch;
    } else {
      var params;
      if (text) {
        try {
          params = JSON.parse(text);
        } catch (err) {
          out.innerHTML = '<div class="orv-error">参数不是合法 JSON：' + esc(err.message) + '</div>';
          return;
        }
      }
      var req = { jsonrpc: '2.0', id: ++state.id, method: method.name };
      if (text && params !== undefined) req.params = params;
      payload = JSON.stringify(req);
      preview = req;
    }

    out.innerHTML = '<div class="orv-pending">发送中…</div>';
    var started = performance.now();
    fetch(state.endpoint, { method: 'POST', headers: headers, body: payload })
      .then(function (res) {
        return res.text().then(function (bodyText) {
          pushHistory(method.name, {
            ts: Date.now(),
            req: preview,
            headers: headers,
            status: res.status,
            latency: Math.round(performance.now() - started),
            bodyText: bodyText
          });
          renderResponse(out, method, 0);
        });
      })
      .catch(function (err) {
        out.innerHTML =
          '<div class="orv-error">请求失败：' + esc(err.message) +
          '<div class="orv-hint">请确认 API 正在运行且 Endpoint 可达。' +
          '若本页面与 API 不同源，还需在服务端配置 CORS_ORIGINS。</div></div>';
      });
  }

  function renderResponse(out, method, idx) {
    var entries = respHistory[method.name] || [];
    var entry = entries[idx];
    if (!entry) return;

    var body;
    try { body = JSON.parse(entry.bodyText); } catch (e) { body = entry.bodyText; }
    var items = Array.isArray(body) ? body : [body];
    var traceIds = [];
    items.forEach(function (it) {
      var t = it && typeof it === 'object' && it.error && it.error.data && it.error.data.trace_id;
      if (t && traceIds.indexOf(String(t)) < 0) traceIds.push(String(t));
    });
    var ok = entry.status >= 200 && entry.status < 300 &&
      !items.some(function (it) { return it && typeof it === 'object' && it.error; });

    var html = '<div class="orv-resp-head">';
    html += '<span class="orv-pill ' + (ok ? 'orv-pill-ok' : 'orv-pill-err') + '">' +
      entry.status + '</span>';
    html += '<span class="orv-latency">' + entry.latency + ' ms</span>';
    traceIds.slice(0, 3).forEach(function (t) {
      html += '<button class="orv-trace" data-trace="' + esc(t) + '" title="点击复制：' +
        esc(t) + '">trace_id: ' + esc(short(t, 8)) + '…</button>';
    });
    html += '<span class="orv-resp-actions">';
    if (!Array.isArray(body) && body && typeof body === 'object' && body.result &&
      typeof body.result.access_token === 'string' && body.result.access_token) {
      html += '<button class="orv-btn orv-btn-ghost" data-act="use-token">设为 Bearer</button>';
    }
    if (ok && /\.logout$/.test(method.name)) {
      html += '<button class="orv-btn orv-btn-ghost" data-act="clear-token">清除 Bearer</button>';
    }
    html += '<button class="orv-btn orv-btn-ghost" data-act="curl">复制 curl</button>';
    html += '</span></div>';

    if (entries.length > 1) {
      html += '<div class="orv-resp-tabs">';
      entries.forEach(function (e, i) {
        html += '<button class="orv-resp-tab ' + (i === idx ? 'orv-resp-tab-active' : '') +
          '" data-idx="' + i + '">' + fmtTime(e.ts) + ' · ' + e.status + '</button>';
      });
      html += '</div>';
    }

    html += '<div class="orv-resp-grid">';
    html += '<div><div class="orv-pane-head"><span class="orv-sec-label">请求</span>' +
      '<button class="orv-btn-mini" data-act="copy-req">复制</button></div>' +
      '<pre class="orv-code">' + highlightJson(entry.req) + '</pre></div>';
    html += '<div><div class="orv-pane-head"><span class="orv-sec-label">响应</span>' +
      '<button class="orv-btn-mini" data-act="copy-resp">复制</button></div>' +
      '<pre class="orv-code">' +
      (typeof body === 'string' ? esc(body) : highlightJson(body)) + '</pre></div>';
    html += '</div>';
    out.innerHTML = html;

    out.querySelector('[data-act="curl"]').addEventListener('click', function () {
      copyText(curlText(entry.req, entry.headers), this);
    });
    var copyReqBtn = out.querySelector('[data-act="copy-req"]');
    copyReqBtn.addEventListener('click', function () {
      copyText(JSON.stringify(entry.req, null, 2), copyReqBtn);
    });
    var copyRespBtn = out.querySelector('[data-act="copy-resp"]');
    copyRespBtn.addEventListener('click', function () {
      copyText(typeof body === 'string' ? body : JSON.stringify(body, null, 2), copyRespBtn);
    });
    var useTokenBtn = out.querySelector('[data-act="use-token"]');
    if (useTokenBtn) {
      useTokenBtn.addEventListener('click', function () {
        setToken(body.result.access_token);
        useTokenBtn.textContent = '已设为 Bearer';
      });
    }
    var clearTokenBtn = out.querySelector('[data-act="clear-token"]');
    if (clearTokenBtn) {
      clearTokenBtn.addEventListener('click', function () {
        setToken('');
        clearTokenBtn.textContent = '已清除';
      });
    }
    out.querySelectorAll('.orv-trace').forEach(function (btn) {
      btn.addEventListener('click', function () {
        copyText(btn.getAttribute('data-trace'), btn);
      });
    });
    out.querySelectorAll('.orv-resp-tab').forEach(function (btn) {
      btn.addEventListener('click', function () {
        renderResponse(out, method, parseInt(btn.getAttribute('data-idx'), 10) || 0);
      });
    });
  }

  // ---------------- 路由 ----------------
  function findMethod(name) {
    for (var i = 0; i < doc.methods.length; i++) {
      if (doc.methods[i].name === name) return doc.methods[i];
    }
    return null;
  }

  // 切页前把当前方法编辑器刷进 editorCache（contenteditable 的 input 偶发漏触发）。
  // 注意：hashchange 触发时 location.hash 已是新值，必须用 currentMethodName。
  function flushEditorCache() {
    if (!currentMethodName) return;
    var editor = mainEl && mainEl.querySelector('#orv-params');
    if (editor) editorCache[currentMethodName] = editor.innerText;
  }

  function route() {
    flushEditorCache();
    currentMethodName = null;
    var hash = location.hash || '#/overview';
    highlightActive();
    if (hash === '#/overview' || hash === '' || hash === '#') return renderOverview();
    if (hash === '#/errors') return renderErrors();
    if (hash.indexOf('#/method/') === 0) {
      var m = findMethod(decodeURIComponent(hash.slice('#/method/'.length)));
      return m ? renderMethod(m) : renderNotFound();
    }
    if (hash.indexOf('#/component/') === 0) {
      return renderComponent(decodeURIComponent(hash.slice('#/component/'.length)));
    }
    renderOverview();
  }

  // ---------------- 启动 ----------------
  function bindHeader() {
    var endpointSelect = root.querySelector('#orv-endpoint');
    var tokenInput = root.querySelector('#orv-token');
    var langSelect = root.querySelector('#orv-lang');

    // 候选项：严格来自 doc.servers（去重）；文档未声明时回退到 schema 推导地址。
    var options = [];
    (doc.servers || []).forEach(function (s) {
      if (s.url && options.indexOf(s.url) < 0) options.push(s.url);
    });
    if (!options.length) options.push(SCHEMA_URL.replace(/\/openrpc\.json\/?$/, '/jsonrpc'));
    endpointSelect.innerHTML = options
      .map(function (url) { return '<option value="' + esc(url) + '">' + esc(url) + '</option>'; })
      .join('');

    // 已保存的选择仅在仍有效时恢复；默认优先 servers 里与当前页面同源的地址（若有）。
    var pageEndpoint = location.origin + '/jsonrpc';
    var chosen = (state.endpoint && options.indexOf(state.endpoint) >= 0)
      ? state.endpoint
      : (options.indexOf(pageEndpoint) >= 0 ? pageEndpoint : options[0]);
    state.endpoint = chosen;
    endpointSelect.value = chosen;
    endpointSelect.addEventListener('change', function () {
      state.endpoint = endpointSelect.value;
      try { localStorage.setItem(LS + ':endpoint', state.endpoint); } catch (e) { /* ignore */ }
    });

    tokenInput.value = state.token;
    langSelect.value = state.lang;
    tokenInput.addEventListener('change', function () {
      state.token = tokenInput.value.trim();
      try { localStorage.setItem(LS + ':token', state.token); } catch (e) { /* ignore */ }
    });
    langSelect.addEventListener('change', function () {
      state.lang = langSelect.value;
      try { localStorage.setItem(LS + ':lang', state.lang); } catch (e) { /* ignore */ }
    });

    var link = root.querySelector('#orv-schema-link');
    link.setAttribute('href', SCHEMA_URL);

    var themeSelect = root.querySelector('#orv-theme');
    themeSelect.value = state.theme;
    themeSelect.addEventListener('change', function () {
      state.theme = themeSelect.value;
      try { localStorage.setItem(LS + ':theme', state.theme); } catch (e) { /* ignore */ }
      applyTheme();
    });
  }

  function bindNav() {
    navEl.addEventListener('click', function (ev) {
      var btn = ev.target.closest ? ev.target.closest('.orv-nav-item') : null;
      if (!btn) return;
      location.hash = btn.getAttribute('data-hash').slice(1); // 去掉前导 #
    });
    searchEl.addEventListener('input', renderNav);
    window.addEventListener('hashchange', route);

    // 键盘流：/ 聚焦搜索；搜索框 ↓ 进入列表；列表 ↑↓ 移动（Enter 即打开）。
    document.addEventListener('keydown', function (ev) {
      if (ev.key !== '/') return;
      var t = ev.target;
      var tag = t && t.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' ||
        (t && t.isContentEditable)) return;
      ev.preventDefault();
      searchEl.focus();
      searchEl.select();
    });
    searchEl.addEventListener('keydown', function (ev) {
      if (ev.key === 'ArrowDown') {
        var first = navEl.querySelector('.orv-nav-item');
        if (first) { ev.preventDefault(); first.focus(); }
      } else if (ev.key === 'Escape') {
        searchEl.blur();
      }
    });
    navEl.addEventListener('keydown', function (ev) {
      if (ev.key !== 'ArrowDown' && ev.key !== 'ArrowUp') return;
      var items = Array.prototype.slice.call(navEl.querySelectorAll('.orv-nav-item'));
      var i = items.indexOf(document.activeElement);
      if (i < 0) return;
      ev.preventDefault();
      var next = ev.key === 'ArrowDown' ? i + 1 : i - 1;
      if (next >= 0 && next < items.length) items[next].focus();
    });
  }

  function renderLoadError(err) {
    mainEl.innerHTML =
      '<div class="orv-load-error">' +
      '  <h2>无法加载 OpenRPC 文档</h2>' +
      '  <p><code>' + esc(SCHEMA_URL) + '</code></p>' +
      '  <div class="orv-error">' + esc((err && err.message) || String(err)) + '</div>' +
      '  <div class="orv-hint">' +
      '    <p>常见原因：</p>' +
      '    <p>1. API 未启动，或监听地址 / 端口不对；</p>' +
      '    <p>2. 本页面与 API 不同源，浏览器拦截了跨域请求——' +
      '直接打开 API 自带的 <a href="' + esc(SCHEMA_URL.replace(/\/openrpc\.json\/?$/, '/docs')) + '"><code>/docs</code></a>' +
      '（同源、免 CORS 配置），或在服务端配置 <code>CORS_ORIGINS</code>。</p>' +
      '  </div>' +
      '</div>';
  }

  function boot() {
    injectCss();
    root = mount();
    navEl = root.querySelector('#orv-nav');
    mainEl = root.querySelector('#orv-main');
    searchEl = root.querySelector('#orv-search');

    // 主题尽早生效（加载失败页也要正确配色）；跟随系统时监听变化实时切换。
    applyTheme();
    if (mediaLight && mediaLight.addEventListener) {
      mediaLight.addEventListener('change', function () {
        if (state.theme === 'auto') applyTheme();
      });
    }

    mainEl.innerHTML = '<div class="orv-skeleton">正在加载 ' + esc(SCHEMA_URL) + ' …</div>';

    fetch(SCHEMA_URL, { cache: 'no-store' })
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (parsed) {
        doc = parsed;

        root.querySelector('#orv-title').textContent = (doc.info && doc.info.title) || 'JSON-RPC API';
        root.querySelector('#orv-meta').textContent =
          'v' + ((doc.info && doc.info.version) || '?') + ' · ' + doc.methods.length + ' 方法';

        bindHeader();
        bindNav();
        renderNav();
        route();
      })
      .catch(renderLoadError);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
