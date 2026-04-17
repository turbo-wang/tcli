# MPP 支付凭证与收据：公开文档摘录与索引

本文档整理自 **[mpp.dev](https://mpp.dev)** 与 **[docs.tempo.xyz](https://docs.tempo.xyz)** 中与「支付凭证构造」「支付收据」相关的说明，便于与仓库内 `agentic-mpp-requirements-and-api.md` 对照。**规范以 mpp.dev 为准**；Tempo 官方文档侧重链能力与 CLI/SDK 集成，凭证字段级定义主要引用 MPP 站点。

---

## 1. 结论速览

| 主题 | 主要来源 | 说明 |
| --- | --- | --- |
| **客户端支付凭证（Credential）如何构造** | [mpp.dev/protocol/credentials](https://mpp.dev/protocol/credentials) | `Authorization: Payment <base64url(JSON)>`；内含 `challenge`、`source`、`payload`；Tempo `charge` 下 `payload.type` 为 `transaction` / `hash` / `proof`。 |
| **服务端支付收据（Receipt）** | [mpp.dev/protocol/receipts](https://mpp.dev/protocol/receipts) | 成功响应头 `Payment-Receipt`；内容为 base64url 编码的 JSON（`challengeId`、`method`、`reference`、`settlement`、`status`、`timestamp`）。 |
| **Challenge（402 侧）** | [mpp.dev/protocol/challenges](https://mpp.dev/protocol/challenges) | `WWW-Authenticate: Payment ...`；`request` 为 base64url 的 JSON（金额、币种、收款方等）。凭证需回应某一 Challenge。 |
| **HTTP 402 与错误类型** | [mpp.dev/protocol/http-402](https://mpp.dev/protocol/http-402) | 与凭证校验失败时的 Problem Details 等。 |
| **Tempo 上的 MPP 流程（叙述）** | [docs.tempo.xyz/guide/machine-payments](https://docs.tempo.xyz/guide/machine-payments) | 402 → Challenge → 客户端支付 → `Authorization: Payment` 携带 Credential → 200 + Receipt。 |
| **Tempo CLI 与凭证** | [docs.tempo.xyz/cli/request](https://docs.tempo.xyz/cli/request) | `tempo request` 自动处理 402、签名上链、重试并带上 [credential](https://mpp.dev/protocol/credentials)；需先 `tempo wallet login`。 |

**docs.tempo.xyz 上未发现**与 mpp.dev「Credentials」页面**同级**的、单独成篇的「凭证 JSON 每个字段」规范——**字段级构造说明以 mpp.dev 为主**。

---

## 2. mpp.dev：Credential（客户端提交的支付证明）

**文档：** [Credentials | MPP](https://mpp.dev/protocol/credentials)

- **传输：** 放在 **`Authorization`** 头，格式为：`Authorization: Payment <credential>`。
- **编码：** Credential 为 **base64url 编码的 JSON 对象**。

**JSON 结构（文档示例）：**

```json
{
  "challenge": {
    "id": "qB3wErTyU7iOpAsD9fGhJk",
    "realm": "mpp.dev",
    "method": "tempo",
    "intent": "charge",
    "request": "eyJhbW91bnQiOiIxMDAwIi4uLn0",
    "expires": "2025-01-15T12:05:00Z"
  },
  "source": "did:pkh:eip155:4217:0x1234567890abcdef...",
  "payload": {
    "type": "transaction",
    "signature": "0xabc123..."
  }
}
```

**字段（文档表格摘要）：**

| 字段 | 含义 |
| --- | --- |
| `challenge` | 所回应的 [Challenge](https://mpp.dev/protocol/challenges) |
| `source` | 付款方身份（地址、DID、账户 ID 等） |
| `payload` | **随支付方式而变的**支付证明 |

**一次性：** 每个 Credential 仅对**一次**请求有效；处理时需校验 `challenge.id`、未过期、按方法校验支付、拒绝重放。

### 2.1 Tempo charge 的 `payload` 形态（文档表格）

| `payload.type` | 客户端使用场景 | 服务端验证内容 |
| --- | --- | --- |
| `transaction` | 非零扣款、pull 模式 | 广播前已签名的 Tempo 交易 |
| `hash` | 非零扣款、push 模式 | 已提交交易的链上回执 |
| `proof` | 零金额身份流 | 对 Challenge ID 的签名证明（无链上转账） |

零金额 Tempo Challenge 时，文档示例为：`{"type":"proof","signature":"0x..."}`，服务端对 `source` 身份验证明而非广播转账。

**规范原文还指向：** [IETF / paymentauth.org](https://paymentauth.org/)、[Payment Methods（方法相关 request schema）](https://mpp.dev/payment-methods)。

---

## 3. mpp.dev：Receipt（服务端确认已成功收款）

**文档：** [Payment receipts and verification | MPP](https://mpp.dev/protocol/receipts)

- **传输：** 成功响应头 **`Payment-Receipt`**。
- **编码：** **base64url 编码的 JSON**。

**JSON 结构（文档示例）：**

```json
{
  "challengeId": "qB3wErTyU7iOpAsD9fGhJk",
  "method": "tempo",
  "reference": "0xtx789abc...",
  "settlement": {
    "amount": "1000",
    "currency": "usd"
  },
  "status": "success",
  "timestamp": "2025-01-15T12:00:00Z"
}
```

**字段（文档表格摘要）：**

| 字段 | 含义 |
| --- | --- |
| `challengeId` | 对应的 challenge |
| `method` | 支付方式 |
| `reference` | 方法相关的支付引用（如交易哈希、发票 ID） |
| `settlement` | 实际结算金额与币种 |
| `status` | 结果（如 `success`） |
| `timestamp` | 处理时间 |

**按支付方式的 `reference` 格式（文档表格）：**

| 方式 | Reference 格式 |
| --- | --- |
| Tempo | 交易哈希（`0xtx789...`） |
| Stripe | PaymentIntent ID（`pi_1234...`） |

用途：审计、争议处理、对账等。

---

## 4. mpp.dev：Challenge（与凭证配套的 402 侧）

**文档：** [Challenges | MPP](https://mpp.dev/protocol/challenges)

- **传输：** `WWW-Authenticate: Payment id="...", realm="...", method="...", intent="...", request="..."`（可选 `expires`、`description`）。
- **`request`：** base64url 编码的 JSON，解码后常见字段包括 `amount`、`currency`、`recipient` 等（随支付方式变化）。

客户端选择其一 Challenge 后，构造 **Credential** 回应。

---

## 5. docs.tempo.xyz：流程与工具（非字段级「凭证规范」主站）

以下内容**不重复** Credential 的逐字段定义，但说明与 MPP 的关系：

| 页面 | URL | 与本主题的关系 |
| --- | --- | --- |
| Agentic Payments（MPP 总览） | [guide/machine-payments](https://docs.tempo.xyz/guide/machine-payments) | 描述 402 → Challenge → 支付 → `Authorization` 带 Credential → 200 + Receipt；并指向 [mpp.dev](https://mpp.dev)。 |
| `tempo request` | [cli/request](https://docs.tempo.xyz/cli/request) | 自动处理 402、读取 challenge、上链签名、重试并附带 [credential](https://mpp.dev/protocol/credentials)。 |
| 一次性支付（服务端示例） | [guide/machine-payments/one-time-payments](https://docs.tempo.xyz/guide/machine-payments/one-time-payments) | 使用 `mppx` + `tempo` 方法做服务端收费路由，**非**手写 Credential JSON 的教程。 |

---

## 6. 与本仓库 Redot API 的对照（便于集成时心不混）

- 标准 MPP over HTTP：**Credential** 走 `Authorization: Payment`；**Receipt** 走 `Payment-Receipt` 头（见 mpp.dev）。
- 仓库内 `agentic-mpp-requirements-and-api.md` 描述的是 **`POST /api/v1/agentic/mpp/pay`** 的 **Bearer OAuth** 与 **TypedResult 体**中的 `MppPaymentReceiptDto`（含嵌套 `settlement`），与「纯 HTTP 402 头绑定」的传输形态不同，但 **Receipt 字段语义可与 mpp.dev 对齐**。

集成时建议：**客户端支付证明**遵循 mpp.dev **Credential**；**Redot 免密支付**遵循仓库 OpenAPI/文档；两者不要混用同一套 HTTP 头字段名 unless 产品明确统一。

---

## 7. 参考链接（完整 URL）

- https://mpp.dev/protocol/credentials  
- https://mpp.dev/protocol/receipts  
- https://mpp.dev/protocol/challenges  
- https://mpp.dev/protocol/http-402  
- https://docs.tempo.xyz/guide/machine-payments  
- https://docs.tempo.xyz/cli/request  

*摘录日期：以用户本地拉取公开页面为准；若官网更新，请以线上版本为准。*
