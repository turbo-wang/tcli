#!/usr/bin/env python3
"""
Open WebUI media sidecar (reference): register absolute paths to PNGs, serve them at /m/<uuid>.png.

- Listen on 127.0.0.1, port OPENCLAW_MEDIA_PORT (default 18790).
- POST /register with JSON {"path": "/abs/.../login_qr.png"} → {"markdown": "![](http://127.0.0.1:.../m/<id>.png)"}.
- GET /m/<uuid>.png → image/png for registered files only.

See openwebui-tcli-media-bridge.md in the repo root.
"""
from __future__ import annotations

import json
import os
import sys
import uuid
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

HOST = "127.0.0.1"
PORT = int(os.environ.get("OPENCLAW_MEDIA_PORT", "18790"))

# uuid (no extension) -> absolute path on disk
_registry: dict[str, str] = {}


class MediaHandler(BaseHTTPRequestHandler):
    server_version = "OpenClawMediaSidecar/1.0"

    def log_message(self, fmt: str, *args: object) -> None:
        sys.stderr.write("%s - - [%s] %s\n" % (self.address_string(), self.log_date_time_string(), fmt % args))

    def do_POST(self) -> None:
        if urlparse(self.path).path != "/register":
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            data = json.loads(raw.decode("utf-8"))
        except json.JSONDecodeError:
            self.send_error(HTTPStatus.BAD_REQUEST, "invalid JSON")
            return
        path = data.get("path")
        if not path or not isinstance(path, str):
            self.send_error(HTTPStatus.BAD_REQUEST, "missing or invalid 'path'")
            return
        abs_path = os.path.abspath(os.path.expanduser(path))
        if not os.path.isfile(abs_path):
            self.send_error(HTTPStatus.BAD_REQUEST, "path is not a file")
            return
        uid = str(uuid.uuid4())
        _registry[uid] = abs_path
        markdown = f"![](http://127.0.0.1:{PORT}/m/{uid}.png)"
        body = json.dumps({"markdown": markdown}).encode("utf-8")
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        parts = [p for p in parsed.path.strip("/").split("/") if p]
        if len(parts) != 2 or parts[0] != "m":
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        fname = parts[1]
        if not fname.endswith(".png"):
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        uid = fname[:-4]
        disk = _registry.get(uid)
        if not disk or not os.path.isfile(disk):
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        try:
            with open(disk, "rb") as f:
                data = f.read()
        except OSError:
            self.send_error(HTTPStatus.INTERNAL_SERVER_ERROR)
            return
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", "image/png")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def main() -> None:
    server = ThreadingHTTPServer((HOST, PORT), MediaHandler)
    print(f"openclaw_media_sidecar listening on http://{HOST}:{PORT}", file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nshutdown", file=sys.stderr)
        server.shutdown()


if __name__ == "__main__":
    main()
