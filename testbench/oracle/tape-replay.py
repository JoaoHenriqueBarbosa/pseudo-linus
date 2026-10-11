#!/usr/bin/env python3
"""Servidor de replay de uma fita: responde nas portas 80 e 443 com o que o `tape-record.py` gravou, escolhendo a
entrada por método, `Host` e caminho com consulta. O que a fita não tem responde 404. O chromium do oráculo roda
com `--host-resolver-rules="MAP * 127.0.0.1"` e `--ignore-certificate-errors`, então qualquer origem cai aqui.

Uso: tape-replay.py <diretório da fita>   (fica em primeiro plano)
"""
import http.server, json, os, ssl, subprocess, sys, threading
from urllib.parse import urlsplit

tape_dir = sys.argv[1]
tape = json.load(open(os.path.join(tape_dir, "tape.json")))
table = {}
for entry in tape["entries"]:
    parts = urlsplit(entry["url"])
    location = dict(entry["headers"]).get("location")
    if location:
        target = urlsplit(location)
        # Redirecionamento que só troca o esquema (upgrade http->https do chromium) faria laço: a chave ignora o esquema.
        if (target.netloc, target.path or "/", target.query) == (parts.netloc, parts.path or "/", parts.query):
            continue
    key = (entry["method"], parts.netloc.lower(), parts.path or "/", parts.query)
    table.setdefault(key, entry)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def reply(self, method):
        parts = urlsplit(self.path)
        host = (self.headers.get("Host") or "").lower()
        host = host[:-4] if host.endswith(":443") else host[:-3] if host.endswith(":80") else host
        entry = table.get((method, host, parts.path or "/", parts.query))
        if entry is None and method == "HEAD":
            entry = table.get(("GET", host, parts.path or "/", parts.query))
        if entry is None:
            self.send_response(404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        body = b""
        if entry.get("body"):
            body = open(os.path.join(tape_dir, "bodies", entry["body"]), "rb").read()
        self.send_response(entry["status"])
        for name, value in entry["headers"]:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if method != "HEAD":
            self.wfile.write(body)

    def do_GET(self):
        self.reply("GET")

    def do_HEAD(self):
        self.reply("HEAD")

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length") or 0))
        self.reply("POST")

    def do_OPTIONS(self):
        # Preflight de CORS: libera o que foi pedido (a fita não grava preflights).
        self.send_response(204)
        origin = self.headers.get("Origin") or "*"
        self.send_header("Access-Control-Allow-Origin", origin)
        self.send_header("Access-Control-Allow-Methods", self.headers.get("Access-Control-Request-Method") or "GET")
        self.send_header("Access-Control-Allow-Headers", self.headers.get("Access-Control-Request-Headers") or "*")
        self.send_header("Content-Length", "0")
        self.end_headers()


def serve(port, context=None):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
    if context is not None:
        server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()


subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=replay",
                "-keyout", "/tmp/replay.key", "-out", "/tmp/replay.crt"], check=True, capture_output=True)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain("/tmp/replay.crt", "/tmp/replay.key")
threading.Thread(target=serve, args=(443, context), daemon=True).start()
serve(80)
