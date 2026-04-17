# Open WebUI × tcli 二维码展示（简化对齐规范）

**读者：** tcli 开发、OpenClaw Skill/Agent 开发。

**目标：** Open WebUI 里能用 Markdown 看到可扫码图片；OpenClaw 原生 UI 仍能用 `MEDIA:`。**Sidecar 只由 tcli 调用，Skill 不再调 Sidecar。**

---

## 1. 为什么需要两行输出

| 客户端 | 需要什么 |
|--------|-----------|
| OpenClaw 自带 UI | **`MEDIA:/绝对路径/...png`** |
| Open WebUI | **`![](http://127.0.0.1:<port>/m/...)`**（浏览器只能拉 HTTP，不认本地路径） |

---

## 2. 数据流（核心）

```text
tcli 写 login_qr.png
    → tcli POST Sidecar /register { path }
    → Sidecar 返回 markdown 一行
    → tcli  stdout 同时输出：MEDIA 行 + Markdown 行（+ 验证码等原有字段）
    → Skill 把 tcli 原文（至少含 Markdown 行）放进助手回复 → Open WebUI 渲染图片
```

**分工：**

- **tcli：** 生成图 → **调 Sidecar** → **打包输出**（`MEDIA:` + `![](...)`）。
- **Skill：** **不要**再请求 Sidecar；**原样或按约定解析** tcli 输出，把 **Markdown 图片行** 交给 Open WebUI 可见内容即可。

---

## 3. Sidecar（不变，仅由 tcli 使用）

**参考实现：** `demo/openclaw_media_sidecar.py`

- 监听 **`127.0.0.1`**，端口默认 **`OPENCLAW_MEDIA_PORT` / `18790`**。
- **`POST /register`**，`{"path":"/abs/.../login_qr.png"}` → 响应里的 **`markdown`** 字段即 `![](http://127.0.0.1:.../m/<id>.png)` 整行。

Sidecar 需常驻（或与 tcli 同机、在登录前已启动）。若未启动，tcli 应对 `register` 失败做明确报错或降级说明。

---

## 4. tcli 输出约定（重点）

生成二维码并落盘后 **立即**：

1. `POST http://127.0.0.1:<port>/register`，body 为刚生成的 **`login_qr.png` 绝对路径**。
2. 将结果与原有字段 **一起打印**（stdout 或 JSON，二选一在 tcli 内统一即可），至少包含：

| 内容 | 含义 |
|------|------|
| **`MEDIA:/绝对路径/login_qr.png`** | 给 OpenClaw 原生 UI（与现约定一致）。 |
| **`![](http://127.0.0.1:<port>/m/....png)`** 一行 | Sidecar 返回的 **`markdown`**，给 Open WebUI。 |
| **`VERIFICATION_CODE:...`** 等 | 保持现有格式。 |

**建议：** 固定行首标记，方便 Skill 解析，例如：

```text
MEDIA:/Users/.../session/login_qr.png
OPENWEBUI_IMAGE:![](http://127.0.0.1:18790/m/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx.png)
VERIFICATION_CODE:6DC7-28SP
```

若用 JSON，可增加字段如 `openWebUiImageMarkdown` / `mediaLine`，语义与上表一致。

**失败：** Sidecar 不可用时，仍输出 `MEDIA:` 与验证码；`OPENWEBUI_IMAGE:` 可省略或输出错误说明行，勿静默。

---

## 5. Skill 约定（极简）

1. 运行 `tcli ...`，读取 **完整输出**。
2. 组装助手回复时：**必须包含** tcli 给出的 **`![](http://127.0.0.1:...)` 那一行**（或 `OPENWEBUI_IMAGE:` 后缀内容），使 Open WebUI 能渲染图。
3. **不要**在 Skill 里再次调用 Sidecar。
4. 若仍兼容 OpenClaw 控制端，**保留** `MEDIA:` 行（与 tcli 输出一致即可）。

---

## 6. 备选：无 Sidecar

可用 `demo/local_media_data_uri.py` 生成 data URI 的一行 Markdown（仅小图、且注意消息长度上限）。**默认仍以 Sidecar + tcli 内调用为准。**

---

## 7. Docker / 浏览器注意

Open WebUI 在 Docker、Sidecar 在宿主机时，Markdown 里的主机可能是 **`host.docker.internal:<port>`**，需与 tcli/Sidecar 配置一致，保证 **用户浏览器** 能访问该 URL。

---

## 8. 检查清单

- [ ] tcli：落盘 → **调 Sidecar** → 同时输出 **`MEDIA:`** + **Markdown 图片行**。
- [ ] Skill：**只转发/拼接** tcli 输出，**不调** Sidecar。
- [ ] Open WebUI：对话里能看到 **`![](http://...)`** 并扫码成功。
- [ ] Sidecar：**127.0.0.1**、端口与 tcli 一致。

---

## 9. 参考

- [Open WebUI — Connect OpenClaw](https://docs.openwebui.com/getting-started/quick-start/connect-an-agent/openclaw/)
- [OpenAI Chat Completions (HTTP)](https://docs.openclaw.ai/gateway/openai-http-api)

**文档版本：** 1.1
