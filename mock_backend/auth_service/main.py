#!/usr/bin/env python3
"""
Minimal OAuth2 device-flow + POST /issue-token for local `tcli` demos.
Align AUTH_PUBLIC_BASE / TCLI_AUTH_BASE / [auth].base with this server's root URL.
"""
from __future__ import annotations

import json
import os
import secrets
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from html import escape
from urllib.parse import parse_qs, urlparse


PENDING: dict[str, dict] = {}


def _split_user_code_display(raw: str) -> tuple[str, str, str]:
    """Two groups of 4 (8 letters total) + separator, for centered hero display."""
    alnum = "".join(c for c in raw if c.isalnum())
    if len(alnum) >= 8:
        alnum = alnum[:8].upper()
        return alnum[:4], alnum[4:], "-"
    if "-" in raw:
        left, _, right = raw.partition("-")
        a = "".join(c for c in left if c.isalnum())[:4].upper()
        b = "".join(c for c in right if c.isalnum())[:4].upper()
        if len(a) == 4 and len(b) == 4:
            return a, b, "-"
    dot = "\u00b7" * 4
    return dot, dot, "-"


def _verify_page_html(part_a: str, sep: str, part_b: str) -> str:
    a = escape(part_a)
    b = escape(part_b)
    s = escape(sep)
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>tcli — verify device</title>
  <style>
    :root {{
      --bg: #0d1117;
      --surface: #161b22;
      --text: #e6edf3;
      --muted: #8b949e;
      --accent: #58a6ff;
      --glow: rgba(88, 166, 255, 0.35);
    }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      min-height: 100vh;
      background: radial-gradient(ellipse 120% 80% at 50% -20%, #1f2937 0%, var(--bg) 55%);
      color: var(--text);
      font-family: ui-sans-serif, system-ui, "Segoe UI", Roboto, sans-serif;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      padding: 2rem;
    }}
    .card {{
      background: var(--surface);
      border: 1px solid #30363d;
      border-radius: 16px;
      padding: 2.5rem 2rem 2rem;
      max-width: 420px;
      width: 100%;
      text-align: center;
      box-shadow: 0 24px 48px rgba(0,0,0,0.45);
    }}
    .label {{
      font-size: 0.75rem;
      letter-spacing: 0.2em;
      text-transform: uppercase;
      color: var(--muted);
      margin-bottom: 1.25rem;
    }}
    .code {{
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 0.35rem;
      flex-wrap: wrap;
      margin-bottom: 1.75rem;
    }}
    .code span.part {{
      font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
      font-size: clamp(2rem, 8vw, 2.75rem);
      font-weight: 600;
      letter-spacing: 0.12em;
      color: var(--accent);
      text-shadow: 0 0 32px var(--glow);
      padding: 0.35rem 0.5rem;
      border-radius: 8px;
      background: rgba(88, 166, 255, 0.08);
      border: 1px solid rgba(88, 166, 255, 0.25);
    }}
    .code span.sep {{
      font-size: clamp(1.5rem, 5vw, 2rem);
      font-weight: 300;
      color: var(--muted);
      user-select: none;
    }}
    p.hint {{
      font-size: 0.9rem;
      line-height: 1.6;
      color: var(--muted);
    }}
    p.hint code {{
      color: #79c0ff;
      background: #21262d;
      padding: 0.15rem 0.4rem;
      border-radius: 6px;
      font-size: 0.85em;
    }}
  </style>
</head>
<body>
  <div class="card">
    <div class="label">Verification code</div>
    <div class="code" aria-label="User code">
      <span class="part">{a}</span><span class="sep">{s}</span><span class="part">{b}</span>
    </div>
    <p class="hint">Mock server: session auto-approves. You can close this tab — <code>tcli wallet login</code> will finish.</p>
  </div>
</body>
</html>"""


# Unambiguous chars for display (no I, O, 0, 1).
_USER_CODE_ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"


def random_user_code() -> str:
    """RFC 8628-style human-readable code, e.g. XXXX-XXXX (new on each /oauth/device)."""
    def chunk(n: int) -> str:
        return "".join(secrets.choice(_USER_CODE_ALPHABET) for _ in range(n))
    return f"{chunk(4)}-{chunk(4)}"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt: str, *args: object) -> None:
        return

    def _json(self, code: int, body: object) -> None:
        raw = json.dumps(body).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def _html(self, code: int, body: str) -> None:
        raw = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_POST(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else b""

        if path == "/oauth/device":
            device_code = secrets.token_urlsafe(24)
            user_code = random_user_code()
            PENDING[device_code] = {"until": time.time() + 600.0}
            self._json(
                200,
                {
                    "device_code": device_code,
                    "user_code": user_code,
                    "verification_uri": f"http://{self.headers.get('Host', '127.0.0.1:8000')}/verify",
                    "verification_uri_complete": f"http://{self.headers.get('Host', '127.0.0.1:8000')}/verify?code={user_code}",
                    "expires_in": 600,
                    "interval": 1,
                },
            )
            return

        if path == "/oauth/token":
            form = parse_qs(body.decode("utf-8"))
            grant = (form.get("grant_type") or [""])[0]
            if grant != "urn:ietf:params:oauth:grant-type:device_code":
                self._json(400, {"error": "unsupported_grant_type"})
                return
            device_code = (form.get("device_code") or [""])[0]
            slot = PENDING.get(device_code)
            if not slot:
                self._json(400, {"error": "invalid_grant"})
                return
            # Auto-approve for demo (no separate browser step).
            del PENDING[device_code]
            self._json(
                200,
                {
                    "access_token": "demo-access-token",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                },
            )
            return

        if path == "/issue-token":
            try:
                payload = json.loads(body.decode("utf-8") or "{}")
            except json.JSONDecodeError:
                self._json(400, {"error": "invalid_json"})
                return
            _ = payload.get("original_url")
            self._json(
                200,
                {
                    "payment_token": "demo-payment-token",
                    "issuer_base": f"http://{self.headers.get('Host', '127.0.0.1:8000')}",
                },
            )
            return

        self.send_error(404)

    def do_GET(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0].rstrip("/") or "/"

        # Routes for `cargo test` (see tcli/tests/common/mod.rs — TCLI_TEST_MOCK_BASE).
        if path == "/test/mpp":
            raw = b"{}"
            self.send_response(402)
            self.send_header("WWW-Authenticate", 'Payment realm="x"')
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
            return
        if path == "/test/x402":
            if self.headers.get("X-x402-Accept"):
                raw = b"ok-body"
                self.send_response(200)
                self.send_header("Content-Type", "text/plain; charset=utf-8")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
                return
            self._json(402, {"x402": {"n": 1}})
            return
        if path == "/test/paid":
            if self.headers.get("X-Payment-Token"):
                raw = b"after-pay"
                self.send_response(200)
                self.send_header("Content-Type", "text/plain; charset=utf-8")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
                return
            raw = b"paywall"
            self.send_response(402)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
            return

        # Device flow: verification_uri / verification_uri_complete; browsers GET this path.
        if path == "/verify":
            qs = parse_qs(urlparse(self.path).query)
            raw_code = (qs.get("code") or [""])[0].strip().upper()
            part_a, part_b, sep = _split_user_code_display(raw_code)
            page = _verify_page_html(part_a, sep, part_b)
            self._html(200, page)
            return
        self.send_error(404)


def _listen_tuple():
    host = os.environ.get("MOCK_AUTH_HOST", "127.0.0.1")
    port = int(os.environ.get("MOCK_AUTH_PORT", "8000"))
    return host, port


def main() -> None:
    host, port = _listen_tuple()
    server = HTTPServer((host, port), Handler)
    display_host = host if host not in ("0.0.0.0", "::") else "127.0.0.1"
    print(f"auth_service listening on http://{display_host}:{port} (MOCK_AUTH_HOST/MOCK_AUTH_PORT to change)")
    server.serve_forever()


if __name__ == "__main__":
    main()
