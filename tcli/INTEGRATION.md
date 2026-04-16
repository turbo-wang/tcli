# tcli 集成说明（Agent / OpenClaw / 自动化）

本文描述 **`tcli` 客户端**与自动化/Agent 对接时的行为约定，便于少解析歧义、少踩坑。  
**HTTP 接口契约以你们后端发布的 API 文档为准**；此处不重复服务端字段说明。

---

## 数据目录

| 变量 / 路径 | 含义 |
|-------------|------|
| `TCLI_HOME` | 未设置时默认为 `~/.tcli`（或当前用户主目录下的 `.tcli`） |
| `$TCLI_HOME/config.toml` | 可选；`[auth]` 等与 OAuth 相关的配置 |
| `$TCLI_HOME/wallet/oauth.json` | 登录成功后保存的 access token（敏感，权限 `0600`） |
| `$TCLI_HOME/wallet/.device_login_poll.json` | 内部轮询状态（登录过程使用，勿依赖其长期存在） |
| `$TCLI_HOME/device_sn` | 设备 OAuth `deviceSn`，首次生成后复用 |

`~/.openclaw/workspace/tcli-login/<session>/` 下每次 `wallet login` 会新建会话目录，用于放置 **二维码 PNG** 与 **`result.json`**（见下文）。

---

## 配置摘要（`config.toml`）

默认由程序内置；可通过 `config.toml` 或环境变量覆盖。

- **`TCLI_AUTH_BASE`**：覆盖 `[auth].base`（OAuth 服务根 URL）。
- **`[auth]`**：`base`、`client_id`、`device_authorization_path`、`token_path`、`app_name`、`device_name`、可选 `oauth_scope` 等，与设备授权 / token 请求一致即可。

具体键名与默认值见源码 `tcli/src/config_file.rs`。

---

## `tcli wallet login`（默认：后台轮询）

### 设计目标（两次工具调用）

1. 执行 **`tcli wallet login`**（主进程很快退出）。
2. 在命令**已返回**之后，**只读一次**同一会话目录下的 **`result.json`**，判断登录是否成功（无需在登录过程中并行 `read`）。

主进程在启动后台子进程（`tcli wallet login --poll-state <state文件>`）前，会把二维码写入磁盘并打印约定输出。

### 标准输出（stdout）— 固定 **3 行**

便于脚本/Agent **按行**解析，无需猜 JSON：

| 行号 | 格式 | 含义 |
|------|------|------|
| 1 | 绝对路径 | 本次会话的 `login_qr.png` 路径 |
| 2 | `MEDIA:` + 与第 1 行**相同**的绝对路径 | 与常见 Agent 约定一致，可整行复制到聊天 |
| 3 | `VERIFICATION_CODE:` + 用户可读短码 | 与 App 中展示的用户码一致，便于核对 |

示例：

```text
/Users/you/.openclaw/workspace/tcli-login/1739…/login_qr.png
MEDIA:/Users/you/.openclaw/workspace/tcli-login/1739…/login_qr.png
VERIFICATION_CODE:ABCD-EFGH
```

### 标准错误（stderr）— 给人看的说明

为非结构化说明文字（英文）：扫码提示、与 App 核对验证码、**轮询间隔与最长等待时间**（来自设备授权响应中的 `interval` / `expires_in`）、以及 **`result.json` 的绝对路径**。  
**不包含**可在浏览器直接完成设备授权的 URL（需扫码）。

### 会话目录与 `result.json`

- 二维码与 **`result.json`** 位于**同一会话目录**（即 `login_qr.png` 的父目录）。
- 后台轮询结束后会**原子写入** `result.json`（先写临时文件再 `rename`）。

**成功**（不含 access_token 原文）：

```json
{
  "status": "ok",
  "oauth_path": "/path/to/.tcli/wallet/oauth.json",
  "expires_at": 1739579600
}
```

**失败**（用户拒绝、过期、网络或业务错误等）：

```json
{
  "status": "error",
  "message": "…"
}
```

集成方应只依赖 `status` / `message` / `oauth_path` / `expires_at` 等字段；**不要**假设 stderr 里可解析结构化错误码。

---

## 轮询行为（实现摘要）

- Token 轮询使用 **`POST`**，**`Content-Type: application/json`**，body 为 OAuth device token 请求（`grant_type`、`device_code`、`client_id`）。
- 在设备授权返回的 **`expires_in`** 截止前轮询；间隔取响应中的 **`interval`**（实现侧对间隔有下限，避免过密请求）。
- HTTP **400** 的响应体可能在 stderr 中打印摘要，便于排查（实现细节以当前版本为准）。

---

## `tcli wallet whoami` / `tcli wallet balance`

当前实现为 **仅本地**：检查 `oauth.json` 是否存在及可选的 `expires_at` 是否未过期。

- 有一组未过期会话：标准输出一行 **`ok`**
- 否则：**`not logged in`**

**不**调用后端「会话自省」类接口；若产品后续提供官方 introspection URL，再在客户端侧扩展。

---

## `tcli wallet logout`

删除本地 `oauth.json`（并在 stderr 提示已登出）。

---

## 详细日志

全局 **`tcli -v`**（或子命令支持的 verbose）会在 stderr 打印 URL、部分请求元数据等；**不要在生产环境日志中回显 token 原文**。

---

## 版本与变更

集成逻辑以 **`tcli` 源码与本文档同版本**为准；升级 CLI 时请扫一眼本文件与 `CHANGELOG`（若有）。
