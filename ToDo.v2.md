# T-CLI 项目规格与进度（v2 · 2026-04）

> 本文件为 **实现进度与行为说明**（v2）；**原始愿景与阶段任务**仍以根目录 **`ToDo.md`** 为准。

## 1. 项目愿景（不变）

构建仿 Tempo（tempo.xyz）的 Rust CLI **`tcli`**，支撑 Agent 编排：**显式**调用 `tcli wallet` / `tcli request`；对可能返回 **HTTP 402** 的第三方 API，在 **`request` 内**完成可实现的自动重试与预算控制。**自然语言只在 Agent（如 Claude）侧**，不在 CLI 内解析。

## 2. 技术栈（当前）

| 组件 | 选用 |
|------|------|
| 语言 | Rust 2021 |
| CLI | `clap` v4 derive |
| 异步 | `tokio` |
| OAuth2 设备流 | `oauth2` + `reqwest` |
| 凭据 | **磁盘文件** `~/.tcli/wallet/oauth.json`（`0o600`），**非** keyring（原 `ToDo.md` 中的 keyring 仍为可选未来项） |
| HTTP | `reqwest` + `serde` / `serde_json` |
| 终端 | `webbrowser`、`indicatif` |

## 3. 源码布局（实际）

```
tcli/src/
├── main.rs           # 入口
├── lib.rs
├── cli.rs            # 子命令：wallet, add, request, list, update, remove, guide
├── auth.rs           # OAuth2 Device Flow（RFC 8628）
├── storage.rs        # oauth.json 读写
├── config.rs         # OAuth 端点解析（TCLI_AUTH_BASE / config.toml）
├── config_file.rs    # ~/.tcli/config.toml
├── api.rs            # tcli request：402、payment-token、x402、verbose
├── x402.rs           # 402 体解析、MPP 检测文案
└── tempo_reference.rs # `tcli guide` 文案（官方 tempo 命令对照）
```

## 4. 已实现功能摘要

### 4.1 指令分发（`cli.rs`）

- 显式子命令：`wallet`（login / logout / balance）、`add`、`request`、`list`、`update`、`remove`、**`guide`**。
- **未知首参数**：对齐 tempo 风格错误，并提示 `tcli add <name>`（扩展/服务安装暗示）。
- `add` / `list` / `update` / `remove`：当前为 **占位 stub**（打印 stub 文案）。

### 4.2 `tcli wallet login`（`auth.rs` + `storage.rs`）

- RFC 8628：设备码、`verification_uri`、轮询 `oauth/token`。
- 配置来源：**环境变量** 与 **`~/.tcli/config.toml` `[auth]`**（`TCLI_AUTH_BASE` 优先）；详见 `config.rs`。
- Token 写入 **`~/.tcli/wallet/oauth.json`**（或 `TCLI_HOME`）。

### 4.3 `tcli request`（`api.rs`）

- **curl 风格**：`-X`/`--request`、`--json`、`-d`（可重复）、`-H`、`--timeout`、`--dry-run`、`--max-spend` / `TCLI_MAX_SPEND`、**`-v`**（响应元数据走 **stderr**，body 走 **stdout** 便于管道）。
- **默认方法**：无 `-X` 时，有 `--json` 或 `-d` 则 **POST**，否则 **GET**。
- **402 处理顺序**（与真实 `tempo request` 差异见下）：
  1. **Payment token（demo）**：`POST` **`{auth_base}/issue-token`**，body 为 `{"original_url","response_status","response_body"}`；`auth_base` 与 **`tcli wallet login` 同源**：`TCLI_AUTH_BASE` → `[auth].base` → 默认 **`https://app.rp-2023app.com`**。成功则取 `payment_token`，原请求重试时带 **`X-Payment-Token`**（可用 `--payment-token-header` 改名）。  
     - **`~/.tcli/config.toml`**：`[payment_token] url` 可覆盖完整 URL；**`disable = true`** 则跳过此步。  
     - **`POST /issue-token` 失败**：不中断，**继续**后续逻辑（verbose 下打印说明）。
  2. **MPP**：若存在 **`WWW-Authenticate: Payment …`** → 报错并指向 **`tempo request`** / mpp.dev（demo 无法链上签名）。若已做过 payment-token 重试仍 MPP → 使用**另一段**说明文案。
  3. **Legacy x402 demo**：若 body 为 `{"x402":{...}}`，则校验 **`--max-spend`**、需已登录会话，重试带 **`X-x402-Accept`**。
  4. **Problem JSON**（如 `challengeId` + payment-required）：按 MPP 类处理 → 同上，指向官方 Tempo CLI。

### 4.4 `tcli guide`

- 打印官方 **`tempo wallet` / `tempo request`** 等能力摘要，以及 **`tcli` 能力与差异**（便于对照实现）。

## 5. 与官方 Tempo 的差异（必读）

| 能力 | 官方 `tempo request` | 当前 `tcli` |
|------|----------------------|-------------|
| 402 / MPP | 解析 `WWW-Authenticate: Payment`，钱包签名，`Authorization: Payment` | **不**实现链上支付；检测到 MPP 则**报错并引导使用 `tempo request`** |
| 402 / demo | 视产品而定 | **x402 JSON** + **`X-x402-Accept`** 重试；**issue-token** + **`X-Payment-Token`** 为服务端提供的演示/兼容路径（与 `auth_base` 同源） |
| 钱包 | Passkey / Tempo 托管链上密钥 | OAuth **Bearer** 会话 + 磁盘 token |

## 6. 配置与环境（摘要）

| 用途 | 说明 |
|------|------|
| `~/.tcli/config.toml` | `[auth]`：`base`、`client_id`、各 OAuth 路径等；`[payment_token]`：`url` 覆盖、`disable` |
| `TCLI_AUTH_BASE` | 与登录相同的 auth 服务根 URL；**payment token URL = `{base}/issue-token`**（无单独 payment-token 环境变量） |
| `TCLI_MAX_SPEND` / `--max-spend` | x402 demo 预算 |
| `TCLI_HOME` | 数据目录（默认 `~/.tcli`） |

## 7. 测试分层（仓库内）

- `tests/phase1_*`：CLI 解析与 help  
- `tests/phase3_*`：config.toml 与 env  

## 8. 待办 / 可增强项

- [ ] **`add` / `list` / `update` / `remove`**：从 stub 落实为真实服务 manifest 下载与目录管理（见原 `ToDo.md` §3.C）。  
- [ ] **可选**：将 token 存储迁到 **keyring**（与原规格书一致），或保持文件并文档化威胁模型。  
- [ ] **`tcli wallet balance`**：真实余额或会话信息（当前为占位）。  
- [ ] 安装路径、发布渠道与 `auth.tcli.dev` 等 **生产 URL** 在落地时替换占位说明。

## 9. 原阶段性任务与当前对应关系

| 阶段 | 原目标（见 `ToDo.md`） | 状态 |
|------|------------------------|------|
| 一 | clap 子命令 + 未知命令错误 | **已覆盖**（含 `guide`） |
| 二 | Device Flow | **已实现**（文件存 token，非 keyring） |
| 三 | 安全存储 | **部分**：磁盘 oauth.json + config.toml；keyring 未做 |
| 四 | 付费 HTTP / 402 / request | **部分**：x402 demo + payment-token + MPP 检测 + verbose；**非**完整 Tempo 链上支付 |

---

*详见原始规格：**`ToDo.md`**。*
