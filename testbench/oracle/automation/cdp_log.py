"""Gravação das mensagens CDP em JSONL, compartilhada pelo invólucro do pipe (`cdp_tee`) e pelo gancho do pyppeteer.
Cada linha: `{"t": segundos desde o início, "out" | "in": mensagem}`; "out" vai do cliente ao navegador. Strings acima de
2 KB (o script injetado do Playwright, corpos de resposta) viram `<sha256:HEX len=N>` e o texto fica uma vez em
`CDP_TEE_BLOBS/HEX.txt`, para as transcrições não repetirem centenas de KB a cada mensagem."""
import hashlib, json, os, threading, time

LONG = 2048
START = time.time()
_lock = threading.Lock()
_file = None


def _open():
    global _file
    path = os.environ.get("CDP_TEE_LOG")
    if _file is None and path:
        _file = open(path, "a")
    return _file


def shrink(value):
    if isinstance(value, str) and len(value) > LONG:
        digest = hashlib.sha256(value.encode("utf-8", "surrogatepass")).hexdigest()
        blobs = os.environ.get("CDP_TEE_BLOBS")
        if blobs:
            os.makedirs(blobs, exist_ok=True)
            path = os.path.join(blobs, digest + ".txt")
            if not os.path.exists(path):
                with open(path, "w", encoding="utf-8", errors="surrogatepass") as blob:
                    blob.write(value)
        return f"<sha256:{digest} len={len(value)}>"
    if isinstance(value, list):
        return [shrink(item) for item in value]
    if isinstance(value, dict):
        return {key: shrink(item) for key, item in value.items()}
    return value


def record(direction, raw):
    """`raw` é o texto (ou bytes) de uma mensagem JSON do protocolo."""
    file = _open()
    if file is None:
        return
    try:
        message = shrink(json.loads(raw))
    except ValueError:
        message = {"raw": raw.decode("utf-8", "replace") if isinstance(raw, bytes) else raw}
    with _lock:
        file.write(json.dumps({"t": round(time.time() - START, 3), direction: message}, ensure_ascii=False) + "\n")
        file.flush()
