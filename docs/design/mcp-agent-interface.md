# Remote Mic · 智能体接口（MCP）详细设计

- 状态：设计稿（未实现）
- 目标版本：v0.8.x（建议）
- 关联：`docs/项目/规划.md`、`AGENTS.md`
- 规范基线：**MCP 2026-07-28（modern，无 `initialize`，每请求 `_meta`）** +
  **2025-11-25（legacy，`initialize` 握手）双 era 兼容**。
- 结论：**对外只提供一个接口——MCP Server（stdio）**；应用内部用 localhost JSON-RPC 作为桥接层，不对外暴露。
- 界面：**通用设置里提供明确的「智能体接口」开关按钮，默认关闭**。

---

## 1. 目标与非目标

### 1.1 目标

让外部智能体（Claude Code / Codex / DSH / Cursor 等）能够：

1. 读取按键映射、触发时间、应用档案、快捷键、运行状态；
2. 修改全局映射（设置动作、取消关联、禁用/启用）；
3. 修改长按阈值 / 双击窗口；
4. 读取应用档案；第二阶段支持写入/删除；
5. 所有修改**立即热更新**调度器，不需要重启应用；
6. 用户能在 UI 里看到「智能体改了什么」，并能撤销/恢复。

### 1.2 非目标（明确不暴露）

- 不暴露驱动安装、Frida 注入、任意命令执行、更新安装、日志清空；
- 不暴露语音桥启停、`set_dispatch_enabled`（暂停按键接管）这类运行态控制；
- 不允许智能体启动/关闭应用进程；
- 不做远程访问（只绑 `127.0.0.1`）；
- 不把 `config.json` 直接当作对外 API（文件接口只作为内部实现）。

---

## 2. 为什么是 MCP（而不是 CLI / 直接 HTTP）

| 维度 | MCP Server（推荐） | CLI | 直接 HTTP API |
| --- | --- | --- | --- |
| 智能体发现工具 | ✅ `tools/list` + JSON Schema | ❌ 需要读文档、拼命令 | ⚠️ 需要自己写客户端 |
| 参数校验 | ✅ 结构化 schema | ❌ 字符串解析 | ✅ 但需自定义协议 |
| 跨 Agent 兼容 | ✅ 事实标准 | ⚠️ 每家适配命令 | ⚠️ 每家适配协议 |
| 安全面 | ✅ stdio，不监听端口 | ✅ 本地进程 | ⚠️ 端口 + token |
| 实现成本 | 中 | 低 | 中 |

CLI 仍可作为排障/脚本工具后续补充，但**不作为对外智能体接口**。

---

## 3. 总体架构

```
┌─────────────────────────────────────────────┐
│ 外部智能体（Claude Code / Codex / DSH …）     │
│   └─ MCP stdio（JSON-RPC 2.0，按行分隔）      │
└──────────────────┬──────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────┐
│ remote-mic-mcp.exe（随安装包分发的 sidecar）   │
│  - MCP modern/legacy（server/discover、tools/*）│
│  - 读 agent-api.json（端口 + token）          │
│  - 转发到 127.0.0.1:<port>/rpc               │
└──────────────────┬──────────────────────────┘
                   │ HTTP POST + Bearer token
                   ▼
┌─────────────────────────────────────────────┐
│ Remote Mic 主进程（Tauri）                    │
│  agent_api 模块（后台线程）                    │
│   - 只绑 127.0.0.1，随机端口                  │
│   - 校验 token / Host / Origin / 方法白名单    │
│   - 调用 service 层（与 UI 共用）              │
│   - 写 config + dispatcher.update_* + 事件    │
└─────────────────────────────────────────────┘
```

要点：

- **MCP 是唯一对外接口**；localhost RPC 只是内部传输层，不写进用户文档、不对外承诺；
- MCP sidecar 不含业务逻辑，只做「MCP ↔ RPC」翻译，避免双份校验逻辑；
- 应用内 service 层是 UI 与 MCP 的**单一事实来源**，Tauri command 退化为薄壳。

---

## 4. 组件设计

### 4.1 Service 层（新增，重构现有 commands）

现状：57 个 `#[tauri::command]` 里直接 `config_store().load()` → 修改 → `save()` → `dispatcher.update_*()`，业务逻辑与 Tauri `State` 耦合。

新增 `src-tauri/src/services/`（或 `support/services/`）：

| 模块 | 职责 |
| --- | --- |
| `services/mapping.rs` | 读目录/映射、set/clear/set_enabled、trigger timing |
| `services/profile.rs` | 档案列表、读写绑定、创建/删除、reload |
| `services/shortcuts.rs` | 快捷键库读写 |
| `services/config.rs` | **全局写锁** + load-modify-save 事务 + revision 自增 |
| `services/runtime.rs` | 运行状态、语音目标、eat mode（只读/受限写） |

`services/config.rs` 提供：

```rust
pub struct ConfigService {
    store: ConfigStore,
    write_lock: Mutex<()>,
}

impl ConfigService {
    pub fn read<R>(&self, f: impl FnOnce(&Config) -> R) -> Result<R>;
    pub fn write<R>(&self, f: impl FnOnce(&mut Config) -> Result<R>) -> Result<(R, u64)>;
}
```

- `write` 内：加锁 → load → 修改 → 校验 → save → 释放锁；返回新 revision；
- dispatcher 热更新在锁外执行（避免持锁做系统调用），但要保证顺序：先写盘成功再更新；
- 所有 Tauri command 改为调用 service，不再各自 load/save。

### 4.2 应用内 agent_api 服务

新增 `src-tauri/src/agent_api/`：

- `mod.rs`：启动/停止、生命周期；
- `server.rs`：`TcpListener::bind(("127.0.0.1", 0))`，随机端口；
- `rpc.rs`：JSON-RPC 2.0 解析/分发/错误映射；
- `auth.rs`：token 生成、校验、文件 ACL；
- `discovery.rs`：`agent-api.json` 原子写/删除；
- `methods.rs`：RPC 方法 → service 层。

生命周期：

1. 应用启动（setup 末尾）读取配置 `agent_api_enabled`；
2. 若开启：生成 32 字节随机 token，绑定 `127.0.0.1:0`，写 `agent-api.json`；
3. 后台线程 accept，循环处理请求（简单线程池或串行处理；配置写本身有锁，串行足够）；
4. 应用退出：删除 `agent-api.json`，关闭 listener；
5. 设置页切换开关：启动/停止服务，重新生成/删除 token 文件。

### 4.3 MCP sidecar（双 era：modern + legacy）

新增 workspace crate：`crates/mcp-bridge/`（二进制 `remote-mic-mcp`）。

- 无 Tauri、无 GUI、**console subsystem**（stdout 只用于 MCP 协议）；
- 依赖：`serde`、`serde_json`、`ureq`（同步 HTTP，避免引入 tokio）；
- **双 era 兼容**（按 MCP 规范）：
  - **Modern（2026-07-28，默认）**：无 `initialize` 握手；每个请求在 `params._meta` 携带
    `io.modelcontextprotocol/protocolVersion`、`clientInfo`、`clientCapabilities`；
  - **Legacy（2025-11-25；可选 2025-06-18）**：`initialize` → `notifications/initialized` 握手；
  - 服务器按「请求是否带 modern `_meta`」选择 era；同一进程内两种客户端可并存。
- Modern 必备行为：
  - `server/discover` **必须实现**：返回 `supportedVersions`、`capabilities`、
    `_meta['io.modelcontextprotocol/serverInfo']`、`instructions`、`ttlMs`、`cacheScope`；
  - `tools/list`：`resultType: "complete"`、`tools`、`ttlMs`、`cacheScope`；工具顺序固定（确定性）；
    支持 `cursor` 分页（MVP 工具数少于一页，返回无 `nextCursor`）；工具集不随连接变化，`listChanged: false`；
  - `tools/call`：`resultType: "complete"`、`content`、`structuredContent`、`isError`；
  - 每个响应 `_meta` 带 `io.modelcontextprotocol/serverInfo`；
  - 不实现 2026-07-28 已移除的 `ping` / `logging/setLevel` / `notifications/roots/list_changed`；
  - 收到 `initialize` 时按 legacy 分支处理，不当作 modern 请求。
- Legacy 必备行为：
  - `initialize` 返回 `protocolVersion`（协商到 `2025-11-25`；可兼容 `2025-06-18`）、
    `capabilities.tools`、`serverInfo`；
  - `notifications/initialized` 后正常服务；
  - 兼容旧客户端的 `ping`（返回空 result）；
  - legacy 响应不强制 `resultType` / `ttlMs` / `cacheScope`（按旧规范）。
- 传输与生命周期：
  - stdio：一行一个 JSON-RPC 消息，消息内不得有换行；stdout 只允许合法 MCP 消息；
  - stderr 可写任意 UTF-8 日志；应用日志写文件，不写 stdout；
  - `notifications/cancelled`：标记取消，快速返回前检查；
  - stdin EOF → 立即退出（客户端关闭输入即优雅关闭）；
  - 无 session、无跨连接状态；每次调用只依赖 discovery 文件 + 应用 RPC。
- 启动时不要求应用已运行：`server/discover` / `tools/list` 仍可返回；`tools/call` 失败时返回可操作错误
  「请先启动 Remote Mic，并在 通用设置 → 智能体接入 中开启」。
- 为什么手写而不直接引 `rmcp`：MVP 需要同时覆盖 modern + legacy，手写约 300–400 行、依赖少；
  若后续规范再变，再评估官方 SDK。

### 4.4 设置页 UI

「通用设置」新增卡片：**智能体接入（MCP）**

- **界面上有明确开关按钮**：设置页卡片里的 Switch（不是隐藏配置），默认关闭；
- 开关状态持久化到 `config.json` 的 `agent_api_enabled`；
  开启立即启动 RPC 并生成 token，关闭立即停止并删除 token 文件；
- 状态 pill：运行中 / 已停止 / 启动失败（附原因）；显示端口与「token 已生成」（不显示完整 token）；
- 操作：
  - 「复制 MCP 配置」：生成 JSON 片段（Windows 路径）；开关关闭时提示先开启；
  - 「重新生成 token」；
  - 「查看工具列表」（只读弹窗）；
- 说明文案：
  - 仅本机可访问；智能体修改会立即生效；
  - 关闭后 MCP 立即失效；
  - 不暴露驱动/更新等危险操作。

MCP 配置片段（Windows）：

```json
{
  "mcpServers": {
    "remote-mic": {
      "command": "C:\\Program Files\\Remote Mic\\remote-mic-mcp.exe",
      "args": []
    }
  }
}
```

WSL 中的 Agent 用 Windows 互操作路径（需真机验证 stdio）：

```json
{
  "mcpServers": {
    "remote-mic": {
      "command": "/mnt/c/Program Files/Remote Mic/remote-mic-mcp.exe",
      "args": []
    }
  }
}
```

> 注意：WSL 里的进程如果直接跑 Linux 版 MCP，`127.0.0.1` 是 WSL 自己，不一定能访问 Windows 主进程；**推荐让 MCP 以 Windows 进程运行**（WSL interop 启动 .exe），这是选择 sidecar 而非 Linux 脚本的原因之一。

---

## 5. 发现文件与认证

### 5.1 `agent-api.json`

路径：`%LOCALAPPDATA%\RemoteMic\RC003\agent-api.json`

```json
{
  "enabled": true,
  "api_version": 1,
  "app_version": "0.7.4",
  "port": 54321,
  "token": "base64url-random-32-bytes",
  "pid": 12345,
  "started_at": 1760000000
}
```

- 原子写（tmp + rename）；
- ACL：仅当前用户可读（Windows ACL，参考现有 `ensure_runtime_acl` 的做法）；
- 应用退出/关闭开关时删除；
- token 每次启动重新生成；MCP 每次调用前读取文件（文件小，开销可忽略）；
- 文件不存在 / `enabled=false` → MCP 返回明确错误。

### 5.2 认证与请求约束

- 只绑 `127.0.0.1`（IPv4），不绑 `0.0.0.0`；
- 请求头 `Authorization: Bearer <token>`，常量时间比较；
- 校验 `Host` 必须是 `127.0.0.1:<port>`（防 DNS rebinding）；
- 如果带 `Origin` 头 → 拒绝（浏览器 CSRF 面）；
- 请求体上限 64 KiB；读超时 5s；每连接只处理一个请求；
- 限流：全局 20 req/s（令牌桶），超限返回 `-32029`；
- 方法白名单（见 §6），未知方法 `-32601`。

---

## 6. 内部 RPC 协议

### 6.1 传输

```
POST http://127.0.0.1:<port>/rpc
Content-Type: application/json
Authorization: Bearer <token>

{"jsonrpc":"2.0","id":1,"method":"mapping.set","params":{...}}
```

响应：

```json
{"jsonrpc":"2.0","id":1,"result":{"ok":true,"data":{...},"revision":42}}
```

错误：

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32003,"message":"动作不存在","data":{"field":"action","hint":"先调用 mapping.catalog 获取可用动作"}}}
```

### 6.2 错误码

> 这些是**应用内部** RPC 错误码，不直接暴露给智能体。MCP sidecar 会把它们映射为
> `tools/call` 的 `isError: true` + `structuredContent.error.code`（字符串）；
> JSON-RPC 协议级错误才使用标准 `-326xx` / `-32022`。

| code | 含义 |
| --- | --- |
| `-32001` | 应用未运行 / 接口未启用 |
| `-32002` | token 无效 / 未授权 |
| `-32003` | 参数校验失败（`data.field` 指明字段） |
| `-32004` | 目标不存在（按键/触发/档案/快捷键） |
| `-32005` | 版本/ revision 冲突 |
| `-32006` | 当前状态下不允许（例如麦克风不支持双击） |
| `-32007` | 内部错误（附日志提示） |
| `-32601` | 方法不存在 |
| `-32029` | 限流 |

### 6.3 Revision（乐观并发）

在 `Config` 增加：

```rust
#[serde(default)]
pub revision: u64,
```

- 每次 service `write` 成功后 `revision += 1`；
- RPC 返回 `revision`；
- 写方法支持可选 `if_revision`：不匹配返回 `-32005`，避免智能体基于旧状态覆盖用户刚做的修改；
- UI 刷新事件携带 revision；
- 旧配置读成 0，第一次写变成 1，无需迁移。

### 6.4 方法列表

| method | 读/写 | 说明 |
| --- | --- | --- |
| `agent.health` | 读 | 健康检查，返回 `api_version`/`app_version` |
| `agent.capabilities` | 读 | 工具/方法能力、限制（不返回 token） |
| `mapping.catalog` | 读 | 按键/触发/动作分类/默认绑定 |
| `mapping.list` | 读 | `{scope?: "global"\|profile_id}` → 绑定列表（含 enabled） |
| `mapping.set` | 写 | `{button, trigger, action, scope?, if_revision?}`；保存即启用 |
| `mapping.clear` | 写 | `{button, trigger, scope?, if_revision?}`；删除绑定 |
| `mapping.set_enabled` | 写 | `{button, trigger, enabled, scope?, if_revision?}` |
| `timing.get` | 读 | 长按阈值/双击窗口 |
| `timing.set` | 写 | `{long_press_ms, double_click_ms}`，范围 clamp |
| `profiles.list` | 读 | 应用档案（含绑定） |
| `profiles.set_binding` | 写 | 第二阶段 |
| `profiles.clear_binding` | 写 | 第二阶段 |
| `profiles.create` / `profiles.delete` | 写 | 第二阶段，`confirm: true` |
| `shortcuts.list` | 读 | 命名快捷键库 |
| `shortcuts.save` / `shortcuts.delete` | 写 | 第二阶段 |
| `voice.get` / `voice.set` | 读/写 | 语音目标（预留值校验） |
| `eat.get` | 读 | 吃掉模式状态（写不暴露，避免智能体误改拦截行为） |
| `runtime.status` | 读 | 连接/语音桥/旁路状态（只读） |

**动作参数**：优先使用 `mapping.catalog` 返回的 `action_key`；自定义快捷键用 `combo:lctrl+k` 形式；**禁止 `disabled` 动作**（新语义用 `enabled:false`）。

**麦克风**：只允许 `press` / `release`；其它触发返回 `-32006`。

---

## 7. MCP 工具定义（按 2026-07-28 规范）

### 7.1 工具定义字段

每个 tool 必须提供：

- `name`：`remote_mic_*`，1–128 字符，仅 `A-Z/a-z/0-9/_/-/.`，服务内唯一；
- `title`：给人看的短标题（中文）；
- `description`：英文，说明用途、副作用、何时该调用 / 不该调用；
- `inputSchema`：JSON Schema 2020-12（默认 dialect），`type: "object"`、`additionalProperties: false`；
- `outputSchema`：JSON Schema 2020-12，定义 `structuredContent`；
- `annotations`：`readOnlyHint` / `destructiveHint` / `idempotentHint` / `openWorldHint`，按实际行为设置；
- `icons`：可选，MVP 不做。

工具列表必须**顺序固定**；`tools/list` 的 `listChanged` 为 `false`（工具集不动态变化）。

### 7.2 工具清单

| tool | title | annotations（readOnly / destructive / idempotent / openWorld） | 输入 | 返回 data |
| --- | --- | --- | --- | --- |
| `remote_mic_get_status` | 读取运行状态 | true / false / true / false | 无 | 版本、连接、语音桥、旁路、revision |
| `remote_mic_get_mapping_catalog` | 读取映射目录 | true / false / true / false | 无 | 按键/触发/动作/默认绑定 |
| `remote_mic_list_mappings` | 读取当前映射 | true / false / true / false | `{scope?}` | 绑定列表 + revision |
| `remote_mic_set_mapping` | 设置按键映射 | false / false / true / false | `{button, trigger, action, scope?, if_revision?}` | 旧绑定、新绑定、revision |
| `remote_mic_clear_mapping` | 取消关联 | false / true / true / false | `{button, trigger, scope?, if_revision?}` | 被删绑定、revision |
| `remote_mic_set_mapping_enabled` | 禁用/启用绑定 | false / false / true / false | `{button, trigger, enabled, scope?, if_revision?}` | 新状态、revision |
| `remote_mic_get_trigger_timing` | 读取触发时间 | true / false / true / false | 无 | 长按阈值、双击窗口 |
| `remote_mic_set_trigger_timing` | 设置触发时间 | false / false / true / false | `{long_press_ms, double_click_ms}` | 新值、revision |
| `remote_mic_list_profiles` | 读取应用档案 | true / false / true / false | 无 | 档案 + 绑定 |
| `remote_mic_list_shortcuts` | 读取快捷键库 | true / false / true / false | 无 | 快捷键列表 |
| `remote_mic_get_voice_target` | 读取语音目标 | true / false / true / false | 无 | 目标 |
| `remote_mic_get_eat_mode` | 读取吃掉模式 | true / false / true / false | 无 | bool |

> 第二阶段（P3）再增加 `remote_mic_set_profile_binding` / `clear_profile_binding` /
> `create_profile` / `delete_profile` / `save_shortcut` / `delete_shortcut` / `set_voice_target`。
> `delete_profile` 必须带 `confirm: true`。

### 7.3 结果与错误（规范）

**成功结果**（`tools/call`）：

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "resultType": "complete",
    "content": [
      { "type": "text", "text": "{"ok":true,"message":"已保存：音量+ · 单击 → 音量 +","data":{},"revision":42}" }
    ],
    "structuredContent": {
      "ok": true,
      "message": "已保存：音量+ · 单击 → 音量 +",
      "data": { "button": "volume_up", "trigger": "single_click", "action": "system_volume_up" },
      "revision": 42,
      "warnings": []
    },
    "isError": false
  }
}
```

- `content` 里始终附一份 JSON 文本（向后兼容不支持 `structuredContent` 的客户端）；
- `structuredContent` 必须符合该工具的 `outputSchema`；
- `resultType` 固定 `"complete"`（modern）。

**工具执行错误**（参数校验、不存在、状态不允许、应用未运行等）：

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "resultType": "complete",
    "content": [
      { "type": "text", "text": "{"ok":false,"error":{"code":"validation_error","message":"动作不存在","field":"action"}}" }
    ],
    "structuredContent": {
      "ok": false,
      "error": { "code": "validation_error", "message": "动作不存在", "field": "action" },
      "hint": "调用 remote_mic_get_mapping_catalog 获取可用 action_key"
    },
    "isError": true
  }
}
```

- **不要**把业务错误包装成 JSON-RPC error；规范要求输入校验等错误用 `isError: true` 让模型自纠；
- JSON-RPC error 只用于：未知工具、请求结构不合法、服务器内部错误、版本不支持（`-32022`）；
- 应用内部 RPC 的错误码不直接暴露，映射为字符串 `error.code`（`validation_error` /
  `not_found` / `state_error` / `conflict` / `app_not_running` / `unauthorized` / `rate_limited`）。

### 7.4 工具定义示例

```json
{
  "name": "remote_mic_set_mapping",
  "title": "设置按键映射",
  "description": "Create or update one key binding in the global mapping or an app profile. The change is applied immediately and is hot-reloaded by the running app. Use remote_mic_get_mapping_catalog first to discover valid action keys.",
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "button": { "type": "string", "description": "Button key, e.g. volume_up, ok, mic" },
      "trigger": { "type": "string", "enum": ["single_click", "double_click", "long_press", "press", "release"] },
      "action": { "type": "string", "description": "action_key from the catalog, or combo:lctrl+k" },
      "scope": { "type": "string", "description": "global (default) or an app profile id" },
      "if_revision": { "type": "integer", "minimum": 0, "description": "Optional optimistic concurrency guard" }
    },
    "required": ["button", "trigger", "action"]
  },
  "outputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "ok": { "type": "boolean" },
      "message": { "type": "string" },
      "data": { "type": "object" },
      "revision": { "type": "integer" },
      "warnings": { "type": "array", "items": { "type": "string" } },
      "error": {
        "type": "object",
        "properties": {
          "code": { "type": "string" },
          "message": { "type": "string" },
          "field": { "type": "string" }
        },
        "required": ["code", "message"]
      }
    },
    "required": ["ok"]
  },
  "annotations": {
    "readOnlyHint": false,
    "destructiveHint": false,
    "idempotentHint": true,
    "openWorldHint": false
  }
}
```

### 7.5 命名与描述约定

- 工具名前缀 `remote_mic_`（避免与其它 MCP server 冲突）；
- `description` 英文、`title` 中文；描述里写明「立即生效」「可逆/不可逆」「是否需要先读目录」；
- 不把 token、内部路径、日志内容放进工具输出；
- 破坏性工具（删除档案等）必须在描述里明确，并要求 `confirm: true`。

### 7.6 规范符合性清单（2026-07-28）

| 规范项 | 本服务做法 |
| --- | --- |
| 生命周期 | modern 无握手；legacy `initialize` + `notifications/initialized`；双 era |
| 版本协商 | 每请求 `_meta.protocolVersion`；不支持返回 `-32022` + `supported`；`server/discover` 可预先发现 |
| 传输 | stdio，按行分隔；stdout 只写 MCP 消息；stderr 日志 |
| 能力 | `capabilities.tools = { listChanged: false }`；不声明 resources/prompts |
| 工具发现 | `tools/list` 确定性顺序、`resultType`、`ttlMs`、`cacheScope`、`cursor` 分页 |
| 工具调用 | `tools/call` 返回 `content` + `structuredContent` + `isError`；文本 JSON 兜底 |
| 输出 schema | 每个工具提供 `outputSchema`，`structuredContent` 必须符合 |
| 错误 | 协议错误用 JSON-RPC error；业务/校验错误用 `isError: true` |
| 取消 | 处理 `notifications/cancelled`，返回前检查 |
| 安全 | 输入校验、访问控制、限流、输出清洗、human-in-the-loop、审计日志 |
| 缓存 | 列表类结果带 `ttlMs` / `cacheScope`；不依赖跨请求状态 |
| 状态 | 无 session、无跨连接状态；每次调用独立 |

---

## 8. 数据一致性

### 8.1 并发写

当前 `save_mapping`、`clear_mapping_binding`、`set_mapping_binding_enabled`、`save_shortcut`、profile 写命令都是「各自 load → 改 → save」，**没有全局锁**；UI 与 MCP 同时写会丢更新。设计：

- `ConfigService` 全局 `Mutex` 串行化所有 `config.json` 写；
- profile 写操作也走同一把锁（profile 文件是独立的，但 reload/dispatcher 更新需要顺序）；
- 写成功后统一 `dispatcher.update_mapping` / `set_profiles` / `set_trigger_timing`；
- 读操作不需要锁（load 是只读；但为了 revision 一致性，可在写锁内读）。

### 8.2 UI 刷新与撤销

- service 写成功后 `app.emit("config-changed", {revision, source:"agent"|"ui"})`；
- 映射页监听 → `refreshMappings()` / `refreshProfiles()`；
- 若用户正在编辑某格，保留选中，只刷新数据；
- Toast：「智能体修改了按键映射」+ 撤销入口（用返回的旧绑定 + revision 校验；revision 不匹配则提示「内容已变化，无法撤销」）。

### 8.3 与现有 undo 的关系

当前 UI 的删除撤销只保存了被删条目，没有 revision；智能体若在撤销前改了同一条绑定，撤销会覆盖新改动。

设计：UI undo 也带 revision 校验——撤销前比较当前 revision 与删除时的 revision；
不一致时提示「内容已变化，无法撤销」，不再写入。agent 修改不主动清空 toast，
但 toast 的撤销会因 revision 不匹配而安全失败。

---

## 9. 安全设计

威胁模型：**同机、同用户的本地进程**。token 不能防同用户恶意进程（它们能读 token 文件），但能防：

- 其他用户进程；
- 浏览器网页（无 token + 校验 Origin）；
- 误连到错误端口；
- 局域网/远程（不监听外网）。

措施：

1. 默认关闭，用户显式开启；
2. 只绑 `127.0.0.1`，随机端口；
3. 每次启动随机 token，token 文件 ACL 仅当前用户；
4. Host/Origin 校验；
5. 方法白名单，**不暴露**注入/驱动/更新/任意命令；
6. 破坏性操作（删除档案等）要求 `confirm:true`，并返回影响摘要；
7. 限流 + 请求体上限；
8. 审计日志：`[agent] method=... result=... revision=...`（不记录 token）；
9. 设置页可随时关闭/轮换 token；
10. 卸载时删除 `agent-api.json`（NSIS 自定义清理）。

### 9.1 与 MCP 规范安全要求对齐

MCP 2026-07-28 对服务器提出 MUST 级要求，本设计逐条落实：

- **校验所有工具输入**：JSON Schema + 业务校验（按键/触发/动作/范围/scope）；
- **访问控制**：默认关闭 + 明确开关 + 本机 token；工具白名单；
- **限流**：20 req/s，请求体 64 KiB；
- **清洗输出**：工具只返回配置数据，不返回 token、内部路径、日志原文；
- **human-in-the-loop**：设置页明确开关与运行状态；工具注解准确标注 readOnly/destructive；
  客户端应自行弹确认，服务端对删除档案等要求 `confirm:true`；
- **工具注解视为 untrusted**：客户端不应仅凭注解放行敏感操作；服务端仍做真实校验。

已知残余风险（写进文档，不假装解决）：

- 同用户进程可读 token 文件；
- `eat-mode.txt`、Frida 17331 端口等旧文件/端口不是本 API 的防护范围；
- MCP sidecar 本身可被替换/调试（用户机器上的本地程序无法完全防）。

---

## 10. 打包与分发

- 新增 workspace crate `crates/mcp-bridge`，产物 `remote-mic-mcp.exe`；
- Tauri `bundle.externalBin` 增加 `binaries/remote-mic-mcp`；
- 构建脚本（CI / 本地）把 `target/<profile>/remote-mic-mcp.exe` 复制为
  `src-tauri/binaries/remote-mic-mcp-x86_64-pc-windows-msvc.exe`；
- NSIS 安装后 sidecar 位于安装目录（与 `remote-mic.exe` 同级），设置页显示实际路径；
- 卸载时删除 sidecar 与 `agent-api.json`（token 文件在 LOCALAPPDATA，需 NSIS 脚本处理）；
- CI 增加：构建 sidecar、`cargo test -p mcp-bridge`、安装包内校验 sidecar 存在。

---

## 11. 失败模式与文案

| 场景 | 行为 |
| --- | --- |
| 应用未运行 | MCP 工具返回「Remote Mic 未运行，请先启动应用」 |
| 接口未开启 | 「请在 通用设置 → 智能体接入 中开启」 |
| token 失效（应用重启） | MCP 重新读 `agent-api.json`；失败提示重新复制配置 |
| 版本不匹配 | 返回 `api_version` 与最低要求，提示升级 |
| 应用正在退出 | 连接失败，MCP 返回可重试错误 |
| 参数非法 | 结构化 `field` + `hint` |
| revision 冲突 | 提示重新读取后重试 |
| 限流 | 提示稍后重试 |

---

## 12. 测试计划

### 12.1 单元测试

- `ConfigService`：写锁串行、revision 自增、旧配置 revision=0、备份回退；
- service 层：set/clear/enabled、麦克风触发校验、动作校验、范围 clamp；
- RPC：认证失败、Host/Origin、方法白名单、错误码、请求体上限、限流；
- MCP modern：`server/discover`、`_meta` 校验、版本不支持 `-32022`、`tools/list`
  (`resultType`/`ttlMs`/`cacheScope`/顺序固定)、`tools/call` (`content`/`structuredContent`/`isError`)；
- MCP legacy：`initialize` → `notifications/initialized`、旧版 `tools/list`/`tools/call`；
- MCP 工具 schema：每个 tool 的 `title`/`description`/`inputSchema`/`outputSchema`/`annotations` 快照测试；
- stdout 纯净性：任何日志/panic 都不得写 stdout；未知方法、参数错误、取消通知各一条用例；
- 并发：两个线程同时写同一绑定，最终 revision 单调、无丢失。

### 12.2 集成测试

- 启动 Tauri 测试实例（或 mock RPC server）+ MCP 进程，跑完整 tools/call；
- 真机：DSH / Claude Code 各配置一次，执行「读映射 → 改音量+单击 → UI 立即变绿/生效」；
- 断开/重启应用后 MCP 自动恢复；
- 关闭开关后 MCP 立即失败。

### 12.3 CI

- `cargo test --workspace --exclude remote-mic` + `cargo test -p remote-mic`；
- `cargo clippy --workspace --exclude remote-mic --all-targets -- -D warnings`；
- `npm run typecheck` / `npm run build`；
- 构建 sidecar 并检查安装包包含 `remote-mic-mcp.exe`。

---

## 13. 分阶段实施

| 阶段 | 内容 | 可交付 |
| --- | --- | --- |
| P0 | Service 层重构 + ConfigService 写锁 + revision + 现有命令薄壳化 | 行为不变，测试全绿 |
| P1 | 应用内 agent_api（开关/token/发现文件/RPC）+ 设置页卡片 | 可用 curl 调 RPC |
| P2 | MCP sidecar（modern + legacy 双 era、`server/discover`、工具 schema）+ mapping/timing/status 工具 + 打包 | 智能体可读写全局映射 |
| P3 | profiles/shortcuts/voice + UI 事件/撤销 + 文档 | 完整 MVP |
| P4（可选） | CLI、Linux MCP、MCP HTTP transport | 扩展 |

建议 MVP 先做 **P0–P2**，P3 跟进。

---

## 14. 这次梳理发现的其他问题（按优先级）

> 这些不是 MCP 特有，但会影响 MCP 的可靠性和现有稳定性。

### 高

1. **配置写没有全局串行化**：多个 command 各自 load-modify-save，UI 与 MCP 并发会丢更新；profile 写与 reload 也有竞态。→ `ConfigService` 写锁。
2. **外部改 `config.json` 不热更新且会被覆盖**：当前没有 config watcher；如果智能体直接改文件，运行中的应用不会感知，下一次保存还会覆盖。→ MCP 必须走 service，不把文件当 API；文档明确。
3. **没有 revision/乐观并发**：UI undo、智能体批量修改、用户同时操作三者会互相覆盖。→ 增加 `Config.revision` + `if_revision`。

### 中

4. **业务逻辑耦合 Tauri `State`**：57 个 command 无法直接复用；RPC 要么复制逻辑，要么重构。→ service 层。
5. **错误只有字符串**：智能体无法区分「参数错/不存在/状态不允许」。→ 结构化错误码。
6. **UI 不会因外部修改刷新**：没有 `config-changed` 事件，MCP 改完界面还是旧的。→ 事件 + 监听。
7. **UI 撤销没有版本校验**：agent 改完后点撤销会覆盖新值。→ revision 或清空 undo。
8. **MCP sidecar 版本漂移**：应用升级后旧 MCP 可能调用不存在的方法。→ `api_version` 握手 + 兼容策略。
9. **WSL 互操作未验证**：Windows exe 从 WSL 启动的 stdio/路径/权限需要真机测试；失败则需 Linux 构建或 Node 脚本兜底。
10. **`get_mapping_catalog` 每次调用写 INFO 日志**：智能体频繁调用会刷日志；RPC 路径应降为 DEBUG 或去掉。

### 低

11. `parse_action` 仍接受 `disabled`，但目录已移除该动作；MCP 必须拒绝，避免产生「旧禁用动作」绑定。
12. `eat-mode.txt` 是无鉴权热文件；属于既有内部接口，不在 MCP 防护范围，文档标注。
13. Frida 内部端口 `17331` 无鉴权（仅 127.0.0.1）；与 MCP 无关，但安全评估时需列出。
14. `remote-mic.exe` 是 GUI subsystem，不能直接拿来做 stdio MCP；必须独立 console sidecar。
15. 安装/卸载对 `agent-api.json`、sidecar 的清理需要 NSIS 脚本，否则残留 token 文件。
16. 应用退出时 RPC 线程可能阻塞在 accept/读；需要 shutdown 信号 + 超时。
17. 随机端口 + 发现文件存在启动竞态：MCP 读到旧文件/半写文件；需原子写 + pid/健康检查。
18. 工具返回内容可能较大（catalog/全部档案）；需要截断/分页或只返回必要字段。

### MCP 规范相关（本次对照 2026-07-28 发现）

19. **最新规范是 stateless**：2026-07-28 移除了 `initialize` 握手，每个请求靠 `_meta` 携带协议版本；
    只实现旧握手会导致新客户端不可用。→ 必须做 modern + legacy 双 era。
20. **`server/discover` 是 MUST**：不是可选优化，modern 客户端会用它探测/选版本；必须实现。
21. **`tools/list` 的 `resultType` / `ttlMs` / `cacheScope` 是规范字段**：modern 下缺失会被严格客户端拒绝。
22. **业务错误不能塞进 JSON-RPC error**：输入校验/状态不允许等要用 `isError: true` 的 tool result，
    否则模型无法自纠（规范明确要求）。
23. **工具名有字符/长度约束**：1–128 字符，只允许 `A-Za-z0-9_-.`；`remote_mic_*` 符合。
24. **modern 不实现 `ping` / `logging/setLevel` / `roots` / `sampling`**：这些在 2026-07-28 被移除或弃用，
    不要照旧文档实现；legacy 分支才需要兼容 `ping`。
25. **human-in-the-loop 是规范建议**：工具注解只是提示，客户端/用户必须能拒绝；设置页开关 + 审计日志是必要 UI。
26. **`tools/list` 必须确定性排序且不随连接变化**：工具集固定，`listChanged: false`，便于客户端缓存。

---

## 15. 已拍板 / 待确认

### 已确认

- 界面提供**明确的「智能体接口」开关按钮**，默认关闭；
- MCP 规范基线：**2026-07-28 modern + 2025-11-25 legacy 双 era**；
- `eat_mode` 只读；应用档案 / 快捷键写入放 P3；
- 工具 `description` 英文、`title` 中文；
- sidecar 随安装包分发；
- UI 撤销与智能体修改冲突用 revision 提示，不自动覆盖。

### 待确认

- 是否同时兼容 `2025-06-18` legacy（建议支持）；
- 是否保留 `remote_mic_` 工具前缀（建议保留）；
- 设置页是否展示「最近智能体操作」审计列表（建议 P3）。

---

## 16. 验收标准（MVP）

- [ ] 设置页有明确的「智能体接口」开关；默认关闭；关闭后 MCP 立即不可用；
- [ ] MCP sidecar 在 Claude Code / DSH 中可被识别；
- [ ] Modern 客户端：`server/discover` 返回 supportedVersions / capabilities / serverInfo；
- [ ] Modern 请求缺少 `_meta` 时返回规范错误；版本不支持返回 `-32022`；
- [ ] Legacy 客户端：`initialize` → `notifications/initialized` 可用；
- [ ] `tools/list`：`resultType`、`ttlMs`、`cacheScope`、工具顺序固定；
- [ ] 每个工具都有 `title` / `description` / `inputSchema` / `outputSchema` / `annotations`；
- [ ] `tools/call`：`content` + `structuredContent` + `isError`；参数错误用 `isError:true`；
- [ ] stdout 只有合法 MCP 消息，日志只走 stderr / 文件；
- [ ] 智能体能读取映射目录 / 当前映射 / 触发时间；
- [ ] 智能体能设置、删除、禁用/启用全局绑定，UI 立即刷新且按键立即生效；
- [ ] 应用未运行 / 接口关闭 / token 失效时返回可操作错误；
- [ ] 并发写不丢更新，revision 单调；
- [ ] 安装包包含 sidecar，卸载清理 token 文件；
- [ ] 文档：用户接入说明 + 安全说明 + 工具列表。

---

## 17. 参考规范

- [MCP Specification 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28)
- [MCP Tools (2026-07-28)](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)
- [MCP Versioning / Lifecycle (2026-07-28)](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP server/discover (2026-07-28)](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
- [MCP stdio Transport (2026-07-28)](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)
- [MCP 2026-07-28 Changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- [MCP 2025-11-25 Changelog（legacy 基线）](https://modelcontextprotocol.io/specification/2025-11-25/changelog)
