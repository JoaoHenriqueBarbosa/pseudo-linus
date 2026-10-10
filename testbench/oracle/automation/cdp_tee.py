#!/usr/bin/python3
"""Invólucro do executável do Chromium para gravar o CDP que o Playwright fala pelo `--remote-debugging-pipe`.

O Playwright abre o navegador com dois fds extras (3: comandos, 4: respostas e eventos; mensagens JSON separadas por
NUL). Este programa fica no meio: cria dois pipes novos para o Chromium real, repassa os bytes sem alterar nada e
grava cada mensagem (`cdp_log`, `CDP_TEE_LOG`). Sem o `CDP_TEE_LOG`, só repassa. Uso: `executable_path` do `launch()`."""
import os, subprocess, sys, threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cdp_log

REAL = os.environ.get("CDP_TEE_REAL", "/usr/bin/chromium")


def pump(source, sink, direction):
    pending = b""
    while True:
        chunk = os.read(source, 65536)
        if not chunk:
            break
        view = memoryview(chunk)
        while view:
            written = os.write(sink, view)
            view = view[written:]
        pending += chunk
        while b"\0" in pending:
            raw, pending = pending.split(b"\0", 1)
            cdp_log.record(direction, raw)
    os.close(sink)


commands_read, commands_write = os.pipe()
events_read, events_write = os.pipe()


def attach():
    os.dup2(commands_read, 3)
    os.dup2(events_write, 4)


child = subprocess.Popen([REAL] + sys.argv[1:], pass_fds=(3, 4), preexec_fn=attach)
os.close(commands_read)
os.close(events_write)
threads = [
    threading.Thread(target=pump, args=(3, commands_write, "out"), daemon=True),
    threading.Thread(target=pump, args=(events_read, 4, "in"), daemon=True),
]
for thread in threads:
    thread.start()
code = child.wait()
for thread in threads:
    thread.join(timeout=2)
sys.exit(code)
