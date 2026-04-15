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
  tcli wallet login | logout — OAuth2 device flow (QR under ~/.openclaw/workspace/tcli-login/<session>/; stdout = PNG path only; same session dir gets result.json after background poll); token in ~/.tcli/wallet/oauth.json
  tcli wallet whoami | balance    — OAuth session / readiness (not on-chain balances)
  tcli wallet keys|fund|transfer|services|sessions|mpp-sign  — stubs; need Tempo Wallet + `tempo`
  tcli request                    — curl-like; demo x402 + payment-token; MPP not signed here

Differences vs tempo request:
  • MPP (WWW-Authenticate: Payment + on-chain): not implemented — use `tempo request` or mpp.dev
  • Demo paths: POST {auth}/issue-token → X-Payment-Token; legacy {"x402":…} → X-x402-Accept

Configuration:
  ~/.tcli/config.toml [auth] base (default https://app.rp-2023app.com), client_id, paths, app_name, device_name; TCLI_AUTH_BASE overrides base; [payment_token] url / disable
  Login: stdout = QR PNG path only; stderr lists verification_code, auth_url, and path to result.json; after the command returns, read that result.json once (ok | error) — see PAY_AUTHORIZATION_AND_OAUTH_DEVICE_API.md.

Docs: https://docs.tempo.xyz/cli/wallet
"#
}
