"""_net: apoio do `_socket` e do laço asyncio sobre os sockets do kernel do sandbox.

Todo socket do Python é um fd do kernel (`_os.tcp_socket`, `_os.unix_socket`, `_os.udp_socket`): o estado dele
(conexão, erro pendente, opções, fila de datagramas) vive lá, e nada daqui o duplica. Este módulo só guarda o
que não é estado de socket: os nomes locais (`hostname`, `is_local`), a forma dos endereços, e a espera
cooperativa por prontidão de fd, que roda as threads do interpretador enquanto o `poll(2)` do kernel não acusa
nada."""

import _os
import _sys

AF_UNIX = 1
AF_INET = 2
AF_INET6 = 10

POLLIN = 1
POLLOUT = 4

_hostname = [None]


def hostname():
    if _hostname[0] is None:
        try:
            with open('/etc/hostname') as f:
                _hostname[0] = f.read().strip() or 'localhost'
        except OSError:
            _hostname[0] = 'localhost'
    return _hostname[0]


def is_local(host):
    if isinstance(host, bytes):
        host = host.decode()
    return host in ('', '0.0.0.0', '127.0.0.1', 'localhost', '::', '::1', '<broadcast>') or \
        host.startswith('127.') or host == hostname()


def loopback_ip(family):
    return '::1' if family == AF_INET6 else '127.0.0.1'


def address(family, host, port):
    return (host, port, 0, 0) if family == AF_INET6 else (host, port)


def unix_name(raw):
    """O endereço que o Python mostra para um nome do kernel: str no sistema de arquivos, bytes no espaço
    abstrato, '' sem nome."""
    if raw is None:
        return ''
    if raw[:1] == b'\0':
        return raw
    import os
    return os.fsdecode(raw)


def unix_encode(addr):
    if isinstance(addr, str):
        import os
        return os.fsencode(addr)
    return bytes(addr)


# ---- espera por prontidão ---------------------------------------------------------------------------
# `_waits` são os fds em que alguma espera cooperativa está parada. Todos entram num só `poll(2)` do kernel,
# que a `threading._wait_for` chama quando nenhuma thread pendente tem o que fazer.

_waits = []


def _poller(timeout):
    """Espera até `timeout` segundos (`None`, sem limite) por prontidão em algum fd esperado."""
    if _waits:
        _os.poll(list(_waits), timeout)


def _sync_poller():
    import threading
    if _waits:
        if _poller not in threading._pollers:
            threading._pollers.append(_poller)
    elif _poller in threading._pollers:
        threading._pollers.remove(_poller)


def wait_fds(entries, cond, timeout, what):
    """Espera `cond()` valer por até `timeout` segundos (`None`, sem limite), rodando as outras threads, com os
    pares `(fd, events)` de `entries` entre os que o `poll(2)` do kernel vigia quando ninguém mais pode rodar.
    Devolve `cond()` ao fim."""
    import threading
    entries = list(entries)
    _waits.extend(entries)
    _sync_poller()
    try:
        return bool(threading._wait_for(cond, timeout, what))
    finally:
        for entry in entries:
            _waits.remove(entry)
        _sync_poller()


def wait_fd(fd, events, timeout, what):
    """Espera `events` em `fd` por até `timeout` segundos (`None`, sem limite; 0, só sonda), rodando as threads
    pendentes. Devolve verdadeiro se há o que fazer (inclusive `POLLERR`, `POLLHUP` e `POLLNVAL`, que a
    chamada seguinte transforma em erro)."""
    def ready():
        return _os.poll([(fd, events)], 0.0)[0] != 0

    if ready():
        return True
    if timeout == 0.0:
        return False
    return wait_fds([(fd, events)], ready, timeout, what)


def cooperative():
    """O `threading` quando há outra coisa que precisa rodar enquanto o processo espera um filho ou um
    descritor (threads pendentes ou verdes vivas, serviços, sockets ligados a outros processos); `None` quando
    esperar bloqueado no kernel não atrasa ninguém.
    No CPython o `waitpid` e o `read` soltam a GIL, e as outras threads seguem rodando durante a espera."""
    import sys
    threading = sys.modules.get('threading')
    if threading is not None and (threading._pending or threading._services or threading._pollers
                                  or threading._gsched.others()):
        return threading
    return None


def cooperative_fd(fd):
    """Esperar `fd` bloqueado no kernel atrasaria alguém: há outra coisa a rodar e o fd é bloqueante. Um fd
    não bloqueante segue direto para o kernel (`EAGAIN` ou `None`, como no Linux)."""
    return cooperative() is not None and _os.get_blocking(fd)


def wait_readable(fd):
    """Espera `POLLIN` em `fd` (que só outra thread satisfaz) rodando as outras threads; sem ninguém para
    rodar, ou num fd não bloqueante, volta de imediato e a leitura bloqueia (ou dá `EAGAIN`) no kernel. Num
    terminal em modo canônico o `POLLIN` só acende com a linha inteira, como o `read(2)` que o segue."""
    if cooperative_fd(fd):
        wait_fd(fd, POLLIN, None, 'read()')


def wait_stdin(fd):
    """A espera de uma leitura do stdin que saiu da nativa (`SuspendRequest::Wait`): roda as outras threads até
    `fd` ter o que ler e marca a repetição da instrução que parou, para ela ir direto ao kernel."""
    wait_readable(fd)
    _sys._stdin_resume()


def wait_then_call(fd, func, args, kwargs):
    """`wait_stdin` e a repetição da chamada de leitura que parou (o valor é o resultado da instrução)."""
    wait_stdin(fd)
    return func(*args, **kwargs)


def read(fd, n):
    """`read(2)` que cede às outras threads: o processo não pode ficar bloqueado no kernel esperando um pipe
    ou terminal que só uma thread dele satisfaz (o servidor HTTP numa thread, o cliente num filho que o
    `subprocess` lê). Com `n < 0` lê até o fim do arquivo, esperando a prontidão a cada pedaço. Um fd
    não bloqueante segue direto para o kernel (`EAGAIN` ou `None`, como no Linux)."""
    if n == 0 or not cooperative_fd(fd):
        return _os.read(fd, n)
    if n > 0:
        wait_fd(fd, POLLIN, None, 'read()')
        return _os.read(fd, n)
    chunks = []
    while True:
        wait_fd(fd, POLLIN, None, 'read()')
        chunk = _os.read(fd, 65536)
        if not chunk:
            return b''.join(chunks)
        chunks.append(chunk)


_PIPE_BUF = 4096
_S_IFMT = 0o170000
_S_IFIFO = 0o010000


def wait_writable(fd):
    """Espera `POLLOUT` em `fd` (que só outra thread esvazia) rodando as threads pendentes; sem ninguém para
    rodar, ou num fd não bloqueante, não espera e a chamada seguinte bloqueia (ou dá `EAGAIN`) no kernel."""
    if cooperative_fd(fd):
        wait_fd(fd, POLLOUT, None, 'write()')


def write(fd, data):
    """`write(2)` que cede às outras threads num pipe cheio. No Linux a escrita bloqueante num pipe só volta
    com tudo escrito, e um pipe cujo leitor é outra thread do processo nunca esvazia se o interpretador
    ficar parado no kernel. A espera é por `POLLOUT` (lugar para `PIPE_BUF` bytes), e cada pedaço escrito
    cabe, então o kernel não bloqueia (a descrição de arquivo, compartilhada com outros processos, não troca
    de modo). Até `PIPE_BUF` a escrita segue atômica (um pedaço só). Um fd não bloqueante, fora de um pipe
    ou sem outra coisa a rodar vai direto ao kernel."""
    if not isinstance(data, (bytes, bytearray, memoryview)) or not cooperative_fd(fd):
        return _os.write(fd, data)
    import os
    data = bytes(data)
    if not data or os.fstat(fd).st_mode & _S_IFMT != _S_IFIFO:
        return _os.write(fd, data)
    total = 0
    while True:
        wait_fd(fd, POLLOUT, None, 'write()')
        try:
            total += _os.write(fd, data[total:total + _PIPE_BUF])
        except BrokenPipeError:
            # O Linux devolve o que já escreveu; o erro só sobe quando nada foi.
            if total:
                return total
            raise
        if total >= len(data):
            return total


def wait_retry(attempt, what, step=0.001):
    """Espera uma trava que pode estar com outra thread ou com outro processo: `attempt()` tenta sem bloquear
    (verdadeiro quando conseguiu) e, entre as tentativas, as outras threads rodam por até `step` segundos."""
    while not wait_fds((), attempt, step, what):
        pass
