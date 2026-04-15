# 支付授权与 OAuth Device Flow 接口说明

本文档描述 `redot-api` 中与 **支付授权凭证**、**OAuth 2.0 Device Authorization Grant（RFC 8628）** 相关的 HTTP 接口。

- **Base URL**：下文用 `BASE_URL` 表示，例如本地默认 `http://127.0.0.1:8096`（以 `server.port` 为准）。
- **两种响应形态**：
  - **OAuth 裸 JSON**：`POST /api/v1/oauth/device_authorization`、`POST /api/v1/oauth/token` 成功/部分错误时，Body 为 RFC 风格字段（无统一 `TypedResult` 包装）。
  - **TypedResult**：`GET /api/v1/oauth/device/resolve` 及 `/api/v1/pay/authorization/*` 多数接口为 `{ "code": ..., "msg": ..., "data": ... }`（业务是否成功以 `code` 为准）。
- **全局参数校验**：带 `@Validated` 的接口若请求体不合法，由 `ControllerExceptionHandler` 处理，**HTTP 状态码常为 200**，Body 为 SaResult 形态（例如 `{ "code": 500, "msg": "client_id must not be blank", "data": null }`），与 App 其它接口一致；OAuth 的 `invalid_client`（HTTP 400）仅出现在控制器显式返回的业务错误分支。

---

## OAuth 2.0 Device Flow（RFC 8628）

设备或 Agent 在无浏览器场景下发起授权：先申请 `device_code` / `user_code`，用户在手机或浏览器打开授权页完成操作后，设备轮询 `token` 获取 `access_token` 与支付授权凭证。

### 与扫码二维码接口的差异（勿混用）


| 项目    | OAuth：`/api/v1/oauth/device_authorization`                                                          | 扫码 QR：`/api/v1/pay/authorization/qr`                |
| ----- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| 请求体   | **扁平 JSON**：`client_id`、`appName`、`deviceName`、`deviceSn`、`timestamp` 等（**无** `data` / `signature`） | **ECDSA**：`data`（Base64 明文 JSON）+ `signature`（十六进制） |
| 服务端类型 | 映射为 `OAuthDeviceRegistration`，与扫码验签后的载荷在类型上分离                                                       | 验签解析为 `CreateQrDataVO` 再落单                          |
| 成功响应  | OAuth 字段：`device_code`、`user_code`、`verification_uri_complete` 等                                    | `TypedResult`，内含二维码内容                               |


误将 OAuth 的 JSON 发到 `**/pay/authorization/qr`** 会因缺少 `data` 触发校验错误（例如 `msg` 含 `**data must not be blank**`）。请确认路径为 `**/api/v1/oauth/device_authorization**`（含 `**oauth**` 段）。

### 流程概览

```text
设备/Agent                         服务端                         用户
   | POST device_authorization  -->  创建待授权单 + Redis 会话
   |<-- device_code, user_code,      （与扫码共用 PayAuthorizationLinkService
        verification_uri_complete        生成 H5 深链）
   |
   | 轮询 POST /oauth/token     -->  未完成则 authorization_pending
   |                                 用户打开 verification_uri_complete
   |                                 在 App/H5 同意授权后
   |<-- access_token + pay_authorization_credential（成功时）
```

---

### 1. Device Authorization Request

**POST** `BASE_URL/api/v1/oauth/device_authorization`


| 项            | 说明                 |
| ------------ | ------------------ |
| 鉴权           | 无（`@SaIgnore`）     |
| Content-Type | `application/json` |


**请求体字段**


| 字段           | 类型     | 必填  | 说明                                                              |
| ------------ | ------ | --- | --------------------------------------------------------------- |
| `client_id`  | string | 是   | 客户端标识；若配置 `pay.oauth.device.allowed-client-ids`（非空逗号分隔），则须命中白名单 |
| `appName`    | string | 是   | 应用/技能名称                                                         |
| `publicKey`  | string | 否   | 可选；无设备签名场景可传空字符串                                                |
| `deviceName` | string | 是   | 设备名称                                                            |
| `deviceSn`   | string | 是   | 设备序列号                                                           |
| `timestamp`  | number | 是   | 毫秒时间戳，必须大于 0（`@Positive`）                                       |
| `scope`      | string | 否   | 预留；当前服务端落单未使用                                                   |


**成功：HTTP 200**，Body 为 OAuth 风格 JSON：


| 字段                          | 说明                                                                                  |
| --------------------------- | ----------------------------------------------------------------------------------- |
| `device_code`               | 设备侧保密，仅用于轮询 `token`                                                                 |
| `user_code`                 | 用户可读短码（如 `XXXX-XXXX`）                                                               |
| `verification_uri`          | 配置项 `pay.oauth.device.verification-uri`，用于说明用户去哪授权                                  |
| `verification_uri_complete` | **与本次待授权单生成的 H5 授权页完整 URL 一致**（含 Base64 `data` 与 HMAC `signature` 查询参数，与扫码打开页面同源逻辑） |
| `expires_in`                | 秒，设备会话过期时间（`pay.oauth.device.expires-in-seconds`）                                   |
| `interval`                  | 秒，建议轮询间隔（`pay.oauth.device.interval-seconds`）                                       |


**业务错误：HTTP 400**，Body：

```json
{ "error": "invalid_client", "error_description": "..." }
```

典型原因：`client_id` 不在白名单、参数在业务层判定非法等（`IllegalArgumentException`）。

**服务端异常：HTTP 500**，Body：`{ "error": "server_error" }`。

**请求体验证失败**：走全局校验，多为 **HTTP 200** + `code`/`msg`（见文首说明），例如缺 `client_id`、`timestamp` 非法。

**curl**

```bash
curl -sS -X POST "${BASE_URL}/api/v1/oauth/device_authorization" \
  -H "Content-Type: application/json" \
  -d '{
    "client_id": "my-agent-client",
    "appName": "demo-app",
    "publicKey": "",
    "deviceName": "Device-1",
    "deviceSn": "SN-001",
    "timestamp": 1739579600000
  }'
```

---

### 2. Device Token（轮询）

**POST** `BASE_URL/api/v1/oauth/token`


| 项            | 说明                                                           |
| ------------ | ------------------------------------------------------------ |
| 鉴权           | 无                                                            |
| Content-Type | `application/x-www-form-urlencoded` **或** `application/json` |


参数可通过 **Query/Form** 或 **JSON Body**（`OAuthDeviceTokenRequest`）传递；实现中会合并：表单优先，缺省再从 JSON 取。

**参数**


| 字段            | 必填  | 说明                                                |
| ------------- | --- | ------------------------------------------------- |
| `grant_type`  | 是   | 固定：`urn:ietf:params:oauth:grant-type:device_code` |
| `device_code` | 是   | 上一步返回的 `device_code`                              |
| `client_id`   | 是   | 与上一步一致                                            |


**成功：HTTP 200**

```json
{
  "access_token": "<opaque>",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "pay.authorization",
  "pay_authorization_credential": { }
}
```

- `expires_in` 来自 `pay.oauth.device.access-token-ttl-seconds`（秒）。
- `pay_authorization_credential` 为 `PayAuthorizationCredentialVO`，仅当轮询成功且业务返回凭证时存在；结构以支付域为准。

**未完成或错误：HTTP 400**，Body：

```json
{ "error": "<error_code>", "error_description": "..." }
```


| `error`                  | 含义                               |
| ------------------------ | -------------------------------- |
| `authorization_pending`  | 用户尚未在 App/H5 完成授权                |
| `slow_down`              | 轮询过于频繁，应加大间隔后再试                  |
| `access_denied`          | 用户拒绝授权                           |
| `expired_token`          | 会话过期或 `device_code` 无效           |
| `invalid_grant`          | 如 `client_id` 与会话不一致             |
| `unsupported_grant_type` | `grant_type` 不是 device_code      |
| `invalid_request`        | 缺少 `device_code` / `client_id` 等 |


**curl（form-urlencoded）**

```bash
curl -sS -X POST "${BASE_URL}/api/v1/oauth/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "grant_type=urn:ietf:params:oauth:grant-type:device_code" \
  --data-urlencode "device_code=<上一步 device_code>" \
  --data-urlencode "client_id=my-agent-client"
```

**curl（JSON）**

```bash
curl -sS -X POST "${BASE_URL}/api/v1/oauth/token" \
  -H "Content-Type: application/json" \
  -d '{
    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
    "device_code": "<上一步 device_code>",
    "client_id": "my-agent-client"
  }'
```

---

### 3. 按 user_code 解析展示信息（可选）

**GET** `BASE_URL/api/v1/oauth/device/resolve?user_code=<USER_CODE>`


| 项   | 说明                                                                    |
| --- | --------------------------------------------------------------------- |
| 鉴权  | 无                                                                     |
| 响应  | `TypedResult<DeviceSessionPreview>`；会话无效或已授权完成时可能返回错误 `code`（如参数类错误码） |


**data 字段（成功时）**


| 字段                 | 说明                      |
| ------------------ | ----------------------- |
| `appName`          | 应用名                     |
| `deviceName`       | 设备名                     |
| `expiresInSeconds` | 剩余有效秒数（会话仍有效且未完成支付侧授权时） |


**curl**

```bash
curl -sS "${BASE_URL}/api/v1/oauth/device/resolve?user_code=ABCD-EFGH"
```

---

### OAuth Device 相关配置（节选）


| 配置项                                         | 说明                         |
| ------------------------------------------- | -------------------------- |
| `pay.oauth.device.verification-uri`         | `verification_uri` 默认落地页   |
| `pay.oauth.device.expires-in-seconds`       | 设备会话 TTL（秒）                |
| `pay.oauth.device.interval-seconds`         | 建议轮询间隔（秒）                  |
| `pay.oauth.device.access-token-ttl-seconds` | 颁发的 `access_token` 存活时间（秒） |
| `pay.oauth.device.allowed-client-ids`       | 非空时，`client_id` 白名单（逗号分隔）  |


H5 深链的 `data`/`signature` 仍由 `pay.authorization.hmac.secret`、`pay.authorization.agentic-auth-url` 等与 `**PayAuthorizationLinkService**` 共用逻辑生成（与扫码一致）。

---

## 支付授权凭证（二维码 / H5）

### 创建授权二维码（ECDSA）

**POST** `BASE_URL/api/v1/pay/authorization/qr`

- **鉴权**：无。
- **Body**：JSON，**必须**包含 `data`（Base64 编码的设备侧 JSON）与 `signature`（十六进制 ECDSA 签名）。**不能与 OAuth Device 的扁平 JSON 混用。**

**响应**：`TypedResult<CreatePayAuthorizationQrResponse>`，`data` 含二维码内容及建议过期时间等。

```bash
curl -sS -X POST "${BASE_URL}/api/v1/pay/authorization/qr" \
  -H "Content-Type: application/json" \
  -d '{
    "data": "<Base64(CreateQrDataVO JSON)>",
    "signature": "<hex>"
  }'
```

### 解析待授权信息（H5）

**POST** `BASE_URL/api/v1/pay/authorization/get`

- Body：`data`、`signature`（与 H5 URL 一致），见 `GetPendingCredentialRequest`。

### 同意授权

**POST** `BASE_URL/api/v1/pay/authorization/allow`

- **鉴权**：需要登录（Sa-Token）。
- Body：`data`、`signature`、`days`、`perTransactionLimit`、`dailyLimit` 等，见 `AllowAuthorizationRequest`。

### 拒绝授权

**POST** `BASE_URL/api/v1/pay/authorization/refuse`

- **鉴权**：需要登录。

### 查询二维码授权状态

**GET** `BASE_URL/api/v1/pay/authorization/qr/get?qrCode=<二维码内容>`

- **鉴权**：无。`qrCode` 为创建接口返回的业务 sn。

```bash
curl -sS "${BASE_URL}/api/v1/pay/authorization/qr/get?qrCode=redotpay%3Axxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
```

---

## CreateQrDataVO / 设备明文 JSON 参考（用于扫码 `data`）

设备侧参与 ECDSA 签名的明文对象（再 Base64 后放入 `data`）常见字段：


| 字段           | 说明              |
| ------------ | --------------- |
| `appName`    | 应用/技能名          |
| `publicKey`  | ECDSA 公钥（与验签一致） |
| `deviceName` | 设备名             |
| `deviceSn`   | 设备序列号           |
| `timestamp`  | 毫秒时间戳           |


服务端还会补充 `sn`、`ipAddress`、`city` 等（见 `PendingCredentialVO`）。

---

## 支付授权通用配置（节选）


| 配置                                   | 说明                                        |
| ------------------------------------ | ----------------------------------------- |
| `pay.authorization.hmac.secret`      | H5 深链查询参数 `signature` 的 HMAC 密钥           |
| `pay.authorization.agentic-auth-url` | 授权页 URL 模板（如含 `%s` 占位 `data`、`signature`） |


---

## 环境与调试

1. 生产/预发请使用 **HTTPS** 与真实域名。
2. `/pay/authorization/allow`、`/refuse` 需先完成 App 登录再携带会话调用。
3. 扫码路径下 `data` / `signature` 须与设备端 ECDSA 实现一致，否则验签失败。
4. OAuth Device 路径**不要求** ECDSA；若调错接口或旧进程未更新，请对照本文「与扫码二维码接口的差异」与路径排查。

---

## tcli / OpenClaw：`wallet login` 与 `result.json`

设计目标：**两次工具调用** —— (1) 执行 `tcli wallet login`；(2) 在命令**已返回**后**只读一次**同一会话目录下的 `result.json`，判断登录是否完成（无需在登录过程中并行 `read`）。

### 路径与产物

| 产物 | 位置 |
| --- | --- |
| 二维码 PNG | `~/.openclaw/workspace/tcli-login/<session>/login_qr.png` |
| 轮询状态（内部） | `$TCLI_HOME/wallet/.device_login_poll.json` |
| OAuth token（敏感） | `$TCLI_HOME/wallet/oauth.json` |
| **给 OpenClaw 读的结果** | **同会话目录** `.../tcli-login/<session>/result.json` |

- **stdout**：仅一行，为 `login_qr.png` 的绝对路径（便于解析后展示图片）。
- **stderr**：含 `verification_code`、`auth_url`，以及 **`result.json` 的完整路径**（便于复制一次读取）。
- 后台进程结束轮询后，**原子写入** `result.json`（先写 `result.json.part` 再 `rename` 为 `result.json`）。

### `result.json` 字段（不含 access_token）

成功：

```json
{
  "status": "ok",
  "oauth_path": "/path/to/.tcli/wallet/oauth.json",
  "expires_at": 1739579600
}
```

失败（用户拒绝、过期、网络等）：

```json
{
  "status": "error",
  "message": "…"
}
```

### 对接 redot 生产 `BASE_URL` 示例

生产环境示例基址：**`https://app.rp-2023app.com`**。在 `~/.tcli/config.toml` 中需与本文 **OAuth Device** 路径一致，例如：

```toml
[auth]
base = "https://app.rp-2023app.com"
client_id = "<须在 pay.oauth.device.allowed-client-ids 白名单内>"
device_authorization_path = "/api/v1/oauth/device_authorization"
token_path = "/api/v1/oauth/token"
app_name = "YourApp"
device_name = "YourDevice"
# oauth_scope = "pay.authorization"   # 可选；不设则不发送 scope 字段
```

`deviceSn` 由 tcli 首次运行时写入 `$TCLI_HOME/device_sn` 并复用。也可用环境变量 `TCLI_AUTH_BASE=https://app.rp-2023app.com` 覆盖 `base`（路径仍取自配置文件中的 `device_authorization_path` / `token_path`）。

本地若对接**非**本文路径的临时服务（例如仓库内其它脚本），只需在 `config.toml` 里改写 `device_authorization_path` / `token_path`；**与后端契约以本文为准**。

### 与本文档接口是否「即插即用」

| 环节 | 说明 |
| --- | --- |
| **URL** | 无 `config.toml` 时 tcli 默认 `[auth].base` 为 **`https://app.rp-2023app.com`**，`device_authorization_path` / `token_path` 与 **§1 / §2** 一致；其它环境可用 `TCLI_AUTH_BASE` 或 `config.toml` 中的 `base` 覆盖。 |
| **Device Authorization 请求体** | **tcli** 使用 **POST `application/json`**，字段与 **§1** 对齐：`client_id`、`appName`、`publicKey`（可为空串）、`deviceName`、`deviceSn`、`timestamp`（毫秒）、可选 `scope`。响应按 OAuth 字段解析为 RFC 8628 结构。 |
| **Token 轮询** | **§2**：tcli 以 **`application/json`** 发送 `OAuthDeviceTokenRequest`（`grant_type`、`device_code`、`client_id`）轮询 `token`；**在 `device_authorization` 响应的 `expires_in` 截止前**持续轮询，间隔取响应中的 `interval`（不低于 **1s**）。成功响应若含 `pay_authorization_credential` 等扩展字段，tcli 当前仅将 `access_token` 等写入 `oauth.json`，扩展字段可后续扩展。 |

