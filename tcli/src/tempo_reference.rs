/// Text for `tcli guide`: official tempo commands vs tcli.
pub fn guide_text() -> &'static str {
    r#"Official Tempo CLI (docs.tempo.xyz/cli/wallet):
  Install / update:  curl -fsSL https://tempo.xyz/install | bash   |   tempoup
  tempo wallet login | logout | whoami
  tempo wallet keys
  tempo wallet fund
  tempo wallet transfer <amount> <token> <to>
  tempo wallet services [--search <query>] [<id>]
  tempo wallet sessions list | sync | close [--all|--orphaned] [--dry-run]
  tempo wallet mpp-sign
  tempo request  — HTTP + MPP signing

tcli — same command names where applicable; implementation differs:
  tcli wallet login | logout — OAuth2 device flow (QR under ~/.openclaw/workspace/tcli-login/<session>/; stdout = path, MEDIA:path, VERIFICATION_CODE:…; same session dir gets result.json after background poll); token in ~/.tcli/wallet/oauth.json
  tcli wallet whoami | balance    — local token file + expiry only (no backend whoami API); use `tcli wallet login` when you need a new session
  tcli wallet keys|fund|transfer|services|sessions|mpp-sign  — stubs; need Tempo Wallet + `tempo`
  tcli request                    — curl-like; 402: Redot agentic/mpp/pay → `Authorization: Payment`
                                    retry (MPP), or legacy x402 body / problem JSON

Differences vs tempo request:
  • MPP / Agentic pay: `tcli agentic-mpp pay` (direct POST) or `tcli request <url>` (402→pay→retry);
    config `[auth].base` + `[agentic_mpp].pay_path`; raw Tempo chain signing: `tempo request`
  • Legacy x402: {"x402":…} → X-x402-Accept (when server sends that shape)

Configuration:
  ~/.tcli/config.toml [auth] base (default https://app.rp-2023app.com), client_id (default OpenClaw), paths, app_name, device_name; TCLI_AUTH_BASE overrides base; legacy `[payment_token]` in old configs is ignored
  Login: stdout = PNG path + MEDIA: + VERIFICATION_CODE: lines; stderr = plain-English steps (scan QR, timing from server) and path to result.json; read result.json once after (ok | error).

Docs: https://docs.tempo.xyz/cli/wallet
"#
}
