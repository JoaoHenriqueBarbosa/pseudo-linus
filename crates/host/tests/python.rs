//! Python sobre o kernel de verdade (daemon, workers e `python3` da imagem). Estes casos precisam do que o kernel de
//! teste do `sysabi` não tem: `fcntl` e `ioctl` com travas entre processos, `posix_spawn`, `signal.set_wakeup_fd`
//! com entrega real de sinal, `pidfd_open` no `epoll` do asyncio e `fork_exec` concorrente. Eles saíram de
//! `ul-python/src/stdlib_tests.rs`, onde o filho rodava síncrono e o resultado era falso.

mod common;

use common::*;
use serde_json::json;

/// Roda `python3 -c src` num sandbox novo e devolve o stdout, exigindo stderr vazio e saída 0.
fn python(src: &str) -> String {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let r = argv(&c, &sb, &["python3", "-c", src]);
    assert_eq!(r["stderr"], "", "{r}");
    assert_eq!(r["exit_code"], 0, "{r}");
    r["stdout"].as_str().unwrap().to_string()
}

/// `create_autospec` de uma classe (o `getattr_static` do `_mock_add_spec` lê o `__dict__` de `function`) e o
/// `__class__` do `spec` do mock, que o `isinstance` consulta. O `unittest.mock` importa o `pkgutil` do disco.
#[test]
fn mock_create_autospec_and_spec_class() {
    let src = "\
class Svc:
    def greet(self): return 'x'
from unittest import mock
a = mock.create_autospec(Svc, instance=True)
a.greet.return_value = 'ok'
print(a.greet())
try: a.nope
except AttributeError: print('sem nope')
print(isinstance(a, Svc), a.__class__ is Svc, type(a) is Svc)
print(mock.Mock(spec=Svc).__class__ is Svc, isinstance(mock.Mock(spec=Svc), Svc), isinstance(mock.Mock(), Svc))
";
    assert_eq!(python(src), "ok\nsem nope\nTrue True False\nTrue True False\n");
}

/// `signal.set_wakeup_fd` e `siginterrupt` (mensagens e retorno do 3.13) e a entrega do byte do sinal no fd.
#[test]
fn signal_wakeup_fd_receives_the_signal_number() {
    let src = r##"
import os, signal, sys
r, w = os.pipe()
try:
    signal.set_wakeup_fd(w)
except ValueError as e:
    print(e)
os.set_blocking(w, False)
print(signal.set_wakeup_fd(w), signal.set_wakeup_fd(w, warn_on_full_buffer=False), signal.set_wakeup_fd(-1))
for bad in (1000, 'x'):
    try:
        signal.set_wakeup_fd(bad)
    except (OSError, TypeError) as e:
        print(type(e).__name__)
signal.signal(signal.SIGUSR1, lambda n, f: print('tratador', n))
signal.siginterrupt(signal.SIGUSR1, False)
signal.set_wakeup_fd(w)
os.kill(os.getpid(), signal.SIGUSR1)
signal.raise_signal(signal.SIGUSR1)
print(os.read(r, 10))
os.set_blocking(r, False)
try:
    os.read(r, 10)
except BlockingIOError:
    print('vazio')
os.close(r)
sys.unraisablehook = lambda a: print('unraisable', a.exc_type.__name__, a.err_msg)
os.kill(os.getpid(), signal.SIGUSR1)
for s in (signal.SIGKILL, 0):
    try:
        signal.siginterrupt(s, True)
    except (OSError, ValueError) as e:
        print(type(e).__name__, e)
"##;
    assert_eq!(
        python(src),
        r##"the fd 4 must be in non-blocking mode
-1 4 4
OSError
TypeError
tratador 10
tratador 10
b'\n\n'
vazio
unraisable BrokenPipeError Exception ignored when trying to write to the signal wakeup fd
tratador 10
OSError [Errno 22] Invalid argument
ValueError signal number out of range
"##
    );
}

/// `loop.add_signal_handler` e `remove_signal_handler` do `unix_events` real, sobre `set_wakeup_fd` e o self-pipe.
#[test]
fn asyncio_add_signal_handler_delivers_through_the_wakeup_fd() {
    let src = r##"
import asyncio, os, signal

async def main():
    loop = asyncio.get_running_loop()
    got = loop.create_future()
    loop.add_signal_handler(signal.SIGUSR1, got.set_result, 'usr1')
    print(signal.getsignal(signal.SIGUSR1).__name__)
    os.kill(os.getpid(), signal.SIGUSR1)
    print(await asyncio.wait_for(got, 2))
    print(loop.remove_signal_handler(signal.SIGUSR1), loop.remove_signal_handler(signal.SIGUSR1))
    print(signal.getsignal(signal.SIGUSR1) == signal.SIG_DFL, signal.set_wakeup_fd(-1))
    try:
        loop.add_signal_handler(signal.SIGKILL, print)
    except RuntimeError as e:
        print(e)

asyncio.run(main())
"##;
    assert_eq!(
        python(src),
        r##"_sighandler_noop
usr1
True False
True -1
sig 9 cannot be caught
"##
    );
}

/// `asyncio.subprocess` do disco sobre `_UnixSubprocessTransport` e `PidfdChildWatcher` (o `os.pidfd_open` do kernel).
#[test]
fn asyncio_subprocess_over_pidfd_child_watcher() {
    let src = r##"
import asyncio, os
from asyncio import unix_events

async def main():
    print(unix_events.can_use_pidfd(), type(asyncio.get_event_loop_policy()._watcher).__name__)
    p = await asyncio.create_subprocess_exec('/bin/sh', '-c', 'echo oi; echo erro >&2; exit 3',
                                              stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    print(p.returncode, type(p).__name__, p.pid > 0)
    out, err = await p.communicate()
    print(out, err, p.returncode, await p.wait())
    p = await asyncio.create_subprocess_exec('/bin/cat', stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE)
    out, _ = await p.communicate(b'um\ndois\n')
    print(out, p.returncode)
    p = await asyncio.create_subprocess_shell('exit 7')
    print(await p.wait(), p.returncode)
    p = await asyncio.create_subprocess_exec('/bin/sleep', '30')
    p.terminate()
    print(await p.wait())

asyncio.run(main())
"##;
    assert_eq!(
        python(src),
        r##"True PidfdChildWatcher
None Process True
b'oi\n' b'erro\n' 3 3
b'um\ndois\n' 0
7 7
-15
"##
    );
}

/// O `asyncio` do disco do Debian (`base_events`, `selector_events`, `unix_events`) sobre `select.epoll` e sockets
/// do kernel: servidor e cliente no mesmo processo, `sock_*`, conexão recusada e `wait_for` com prazo. Os dois
/// processos têm caso na bancada (`asyncio-two-processes`).
#[test]
fn asyncio_selector_loop_over_kernel_sockets() {
    let src = r##"
import asyncio, selectors, socket
print(asyncio.Future.__module__, asyncio.Task.__module__, issubclass(asyncio.Task, asyncio.Future), selectors.DefaultSelector.__name__)

async def handle(r, w):
    while line := await r.readline():
        w.write(b'eco: ' + line.upper())
        await w.drain()
    w.close()
    await w.wait_closed()

async def main():
    loop = asyncio.get_running_loop()
    print(type(loop).__name__, type(loop._selector).__name__)
    srv = await asyncio.start_server(handle, '127.0.0.1', 0)
    port = srv.sockets[0].getsockname()[1]
    r, w = await asyncio.open_connection('127.0.0.1', port)
    for word in ('um', 'dois'):
        w.write(f'{word}\n'.encode())
        await w.drain()
        print((await r.readline()).decode().strip())
    print(type(w.transport).__name__, w.get_extra_info('peername')[1] == port)
    r2, w2 = await asyncio.open_connection('127.0.0.1', port)
    try:
        await asyncio.wait_for(r2.readline(), 0.1)
    except TimeoutError:
        print('wait_for: timeout')
    w2.close()
    await w2.wait_closed()
    w.close()
    await w.wait_closed()
    srv.close()
    await srv.wait_closed()

    ls = socket.socket()
    ls.bind(('127.0.0.1', 0))
    ls.listen()
    ls.setblocking(False)
    cs = socket.socket()
    cs.setblocking(False)
    await loop.sock_connect(cs, ls.getsockname())
    conn, addr = await loop.sock_accept(ls)
    await loop.sock_sendall(cs, b'ping')
    print(await loop.sock_recv(conn, 10), addr[0])
    dead = ls.getsockname()
    for s in (conn, cs, ls):
        s.close()
    s = socket.socket()
    s.setblocking(False)
    try:
        await loop.sock_connect(s, dead)
    except ConnectionRefusedError as e:
        print('recusada', e.errno)
    s.close()
    try:
        await asyncio.wait_for(asyncio.sleep(10), 0.05)
    except TimeoutError:
        print('sleep: timeout')

asyncio.run(main())
"##;
    assert_eq!(
        python(src),
        r##"_asyncio _asyncio True EpollSelector
_UnixSelectorEventLoop EpollSelector
eco: UM
eco: DOIS
_SelectorSocketTransport True
wait_for: timeout
b'ping' 127.0.0.1
recusada 111
sleep: timeout
"##
    );
}

/// `BaseHTTPRequestHandler` num `ThreadingTCPServer` com `serve_forever` numa thread, e os clientes do mesmo processo:
/// `urllib` (GET, POST JSON, redirecionamento, 404, conexão recusada), `http.client` e um `socket` cru em HTTP/1.0.
#[test]
fn http_server_forever_in_thread() {
    let src = r##"
import http.server, threading, json, urllib.request, urllib.error, urllib.parse, socketserver, http.client, socket
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _send(self, code, obj, headers=()):
        body = json.dumps(obj).encode(); self.send_response(code); self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        for k, v in headers: self.send_header(k, v)
        self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        u = urllib.parse.urlsplit(self.path)
        if u.path == '/items': self._send(200, {'q': urllib.parse.parse_qs(u.query), 'ua': self.headers.get('User-Agent', '')[:6]})
        elif u.path == '/redir': self.send_response(302); self.send_header('Location', '/items?r=1'); self.end_headers()
        else: self._send(404, {'err': 'nf'})
    def do_POST(self):
        n = int(self.headers['Content-Length']); data = json.loads(self.rfile.read(n))
        self._send(201, {'got': data, 'ct': self.headers.get_content_type()}, [('X-Id', '7')])
srv = socketserver.ThreadingTCPServer(('127.0.0.1', 0), H); srv.daemon_threads = True
port = srv.server_address[1]; t = threading.Thread(target=srv.serve_forever, daemon=True); t.start()
base = f'http://127.0.0.1:{port}'
with urllib.request.urlopen(base + '/items?a=1&a=2') as r: print(r.status, r.headers['Content-Type'], json.load(r))
req = urllib.request.Request(base + '/items', data=json.dumps({'x': [1, 2]}).encode(), headers={'Content-Type': 'application/json'}, method='POST')
with urllib.request.urlopen(req, timeout=5) as r: print(r.status, r.getheader('X-Id'), json.loads(r.read()))
with urllib.request.urlopen(base + '/redir') as r: print(r.status, r.url.endswith('/items?r=1'), json.load(r)['q'])
try: urllib.request.urlopen(base + '/nada')
except urllib.error.HTTPError as e: print('HTTPError', e.code, e.reason, json.loads(e.read()))
try: urllib.request.urlopen('http://127.0.0.1:1/x', timeout=2)
except urllib.error.URLError as e: print('URLError', type(e.reason).__name__)
c = http.client.HTTPConnection('127.0.0.1', port, timeout=5); c.request('GET', '/items?z=9', headers={'User-Agent': 'agente/1'}); resp = c.getresponse()
print(resp.status, resp.reason, json.loads(resp.read())); c.close()
s = socket.create_connection(('127.0.0.1', port)); s.sendall(b'GET /items HTTP/1.0\r\nHost: x\r\n\r\n'); data = b''
while (chunk := s.recv(4096)): data += chunk
s.close(); print(data.split(b'\r\n')[0], data.endswith(b'}'))
srv.shutdown(); srv.server_close(); print('fim')
"##;
    assert_eq!(
        python(src),
        r##"200 application/json {'q': {'a': ['1', '2']}, 'ua': 'Python'}
201 7 {'got': {'x': [1, 2]}, 'ct': 'application/json'}
200 True {'r': ['1']}
HTTPError 404 Not Found {'err': 'nf'}
URLError ConnectionRefusedError
200 OK {'q': {'z': ['9']}, 'ua': 'agente'}
b'HTTP/1.0 200 OK' True
fim
"##
    );
}

/// `fcntl.fcntl`/`ioctl`/`flock`/`lockf` sobre o kernel (flags, fds, tamanho do pipe, `FIONREAD`, travas entre
/// processos) e o `os.posix_spawn` pelo qual o `subprocess.py` do disco passa quando as condições dele valem.
#[test]
fn fcntl_ioctl_locks_and_posix_spawn_over_the_kernel() {
    let src = r##"
import errno, fcntl, os, signal, struct, subprocess, sys

r, w = os.pipe()
print(fcntl.fcntl(r, fcntl.F_GETFD), fcntl.fcntl(r, fcntl.F_GETFL), fcntl.fcntl(w, fcntl.F_GETFL))
fcntl.fcntl(r, fcntl.F_SETFD, 0)
print(fcntl.fcntl(r, fcntl.F_GETFD), os.get_inheritable(r))
print(fcntl.fcntl(w, fcntl.F_SETFL, os.O_NONBLOCK), os.get_blocking(w), fcntl.fcntl(w, fcntl.F_GETFL))
d = fcntl.fcntl(r, fcntl.F_DUPFD, 30)
e = fcntl.fcntl(r, fcntl.F_DUPFD_CLOEXEC, 40)
print(d, e, fcntl.fcntl(d, fcntl.F_GETFD), fcntl.fcntl(e, fcntl.F_GETFD))
print(fcntl.fcntl(w, fcntl.F_GETPIPE_SZ), fcntl.fcntl(w, fcntl.F_SETPIPE_SZ, 100000), fcntl.fcntl(w, fcntl.F_GETPIPE_SZ), fcntl.fcntl(w, fcntl.F_SETPIPE_SZ, 1))
plain = open('/tmp/plain', 'w+')
for call in (lambda: fcntl.fcntl(1000, fcntl.F_GETFD), lambda: fcntl.fcntl(r, 12345), lambda: fcntl.fcntl(plain, fcntl.F_GETPIPE_SZ),
             lambda: fcntl.fcntl(w, fcntl.F_SETPIPE_SZ, 2 ** 31 + 1), lambda: fcntl.fcntl(r, fcntl.F_GETFD, b'x' * 2000),
             lambda: fcntl.fcntl(r, fcntl.F_GETFD, 1.5), lambda: fcntl.fcntl(-1, 1), lambda: fcntl.fcntl('x', 1)):
    try:
        call()
    except (OSError, ValueError, TypeError) as x:
        print(type(x).__name__, x)
print(fcntl.fcntl(r, fcntl.F_GETFD, b'ab'))
os.write(w, b'12345')
buf = bytearray(4)
print(fcntl.ioctl(r, 0x541B, buf), int.from_bytes(buf, 'little'), fcntl.ioctl(r, 0x541B, b'\0\0\0\0'), fcntl.ioctl(r, 0x541B, bytearray(4), False))
codes = []
for args in ((r, 0x541B, 0), (r, 0x5413, bytes(8)), (r, 0xdead, bytes(4)), (r, 0x541B)):
    try:
        fcntl.ioctl(*args)
    except OSError as x:
        codes.append(x.errno)
print(*codes)
fcntl.ioctl(w, 0x5421, b'\0\0\0\0')
print(os.get_blocking(w))
fcntl.ioctl(w, 0x5421, struct.pack('i', 1))
print(os.get_blocking(w))

f = open('/tmp/lk', 'w+')
f.write('0123456789')
f.flush()
fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
g = open('/tmp/lk')
try:
    fcntl.flock(g, fcntl.LOCK_SH | fcntl.LOCK_NB)
except BlockingIOError as x:
    print('flock', x.errno)
fcntl.flock(f, fcntl.LOCK_UN)
print(fcntl.flock(g, fcntl.LOCK_SH | fcntl.LOCK_NB))
try:
    fcntl.lockf(f, 12)
except ValueError as x:
    print(x)
fcntl.lockf(f, fcntl.LOCK_EX | fcntl.LOCK_NB, 5, 2)
rr, ww = os.pipe()
pid = os.fork()
if pid == 0:
    h = open('/tmp/lk', 'r+')
    try:
        fcntl.lockf(h, fcntl.LOCK_EX | fcntl.LOCK_NB, 5, 2)
        res = 'locked'
    except OSError as x:
        res = x.errno
    probe = fcntl.fcntl(h, fcntl.F_GETLK, struct.pack('hhqqi', fcntl.F_WRLCK, 0, 0, 3, 0))
    kind, whence, start, length, owner = struct.unpack('hhqqi', probe)
    os.write(ww, repr((res, kind, whence, start, length, owner == os.getppid())).encode())
    os._exit(0)
os.close(ww)
print(os.read(rr, 100).decode())
os.waitpid(pid, 0)

p = subprocess.Popen(['/bin/cat'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, pipesize=8192)
print(fcntl.fcntl(p.stdin.fileno(), fcntl.F_GETPIPE_SZ), fcntl.fcntl(p.stdout.fileno(), fcntl.F_GETPIPE_SZ))
print(p.communicate(b'hi'))

real_spawn = os.posix_spawn
calls = []
def counting(*a, **k):
    calls.append(a[0])
    return real_spawn(*a, **k)
os.posix_spawn = counting
print(subprocess._USE_POSIX_SPAWN, subprocess._HAVE_POSIX_SPAWN_CLOSEFROM)
print(subprocess.run(['/bin/sh', '-c', 'echo ok'], capture_output=True).stdout, calls)
calls.clear()
print(subprocess.run('echo hi; exit 2', shell=True, capture_output=True, text=True), calls)
calls.clear()
subprocess.run(['echo', 'x'], capture_output=True)
print(calls)
subprocess.run(['/bin/true'], cwd='/tmp')
print(calls)
subprocess.run(['/bin/true'], close_fds=False)
print(calls)
for args in (['/nonexistent'], ['/tmp/plain']):
    try:
        subprocess.run(args)
    except OSError as x:
        print(type(x).__name__, x)
os.posix_spawn = real_spawn

out = '/tmp/spawn.out'
act = [(os.POSIX_SPAWN_OPEN, 1, out, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)]
pid = os.posix_spawn('/bin/echo', ['echo', 'spawned'], os.environ, file_actions=act)
print(os.waitpid(pid, 0)[1], repr(open(out).read()))
pid = os.posix_spawnp('echo', ['echo', 'path'], os.environ, file_actions=act)
print(os.waitpid(pid, 0)[1], repr(open(out).read()))
pid = os.posix_spawn('/bin/sh', ['sh', '-c', 'echo $FOO'], {'FOO': 'bar'}, file_actions=act, setsigmask=[signal.SIGINT], setsigdef=[signal.SIGPIPE])
print(os.waitpid(pid, 0)[1], repr(open(out).read()))
r2, w2 = os.pipe()
pid = os.posix_spawn('/bin/echo', ['echo', 'piped'], os.environ, file_actions=[(os.POSIX_SPAWN_DUP2, w2, 1), (os.POSIX_SPAWN_CLOSE, r2)])
os.close(w2)
print(os.read(r2, 100), os.waitpid(pid, 0)[1])
for kw in ({'setsid': True}, {'setpgroup': 0}):
    pid = os.posix_spawn('/bin/sleep', ['sleep', '5'], os.environ, **kw)
    print(os.getsid(pid) == pid if 'setsid' in kw else os.getpgid(pid) == pid)
    os.kill(pid, signal.SIGTERM)
    print(os.waitpid(pid, 0)[1])
for call in (lambda: os.posix_spawn('/nonexistent', ['x'], {}),
             lambda: os.posix_spawn('/bin/true', [], {}),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, file_actions=[(99, 1)]),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, file_actions=[(os.POSIX_SPAWN_CLOSE,)]),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, file_actions=[(os.POSIX_SPAWN_DUP2, 1)]),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, file_actions=[(os.POSIX_SPAWN_CLOSE, -1)]),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, setsigdef=[0]),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, scheduler=(os.SCHED_FIFO, os.sched_param(1))),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, scheduler=(None, 5)),
             lambda: os.posix_spawn('/bin/true', ['true'], {}, file_actions=[(os.POSIX_SPAWN_OPEN, 3, '/nonexistent/x', os.O_RDONLY, 0)]),
             lambda: os.posix_spawnp('nonexistent-prog', ['x'], {})):
    try:
        call()
    except Exception as x:
        print(type(x).__name__, x)
"##;
    assert_eq!(
        python(src),
        r##"1 0 1
0 True
0 False 2049
30 40 0 1
65536 131072 131072 4096
OSError [Errno 9] Bad file descriptor
OSError [Errno 22] Invalid argument
OSError [Errno 9] Bad file descriptor
OSError [Errno 22] Invalid argument
ValueError fcntl argument 3 is too long
TypeError fcntl requires a file or file descriptor, an integer and optionally a third integer or a string
ValueError file descriptor cannot be a negative integer (-1)
TypeError argument must be an int, or have a fileno() method.
b'ab'
0 5 b'\x05\x00\x00\x00' b'\x05\x00\x00\x00'
14 25 25 14
True
False
flock 11
None
unrecognized lockf argument
(11, 1, 0, 2, 5, True)
8192 8192
(b'hi', None)
True True
b'ok\n' ['/bin/sh']
CompletedProcess(args='echo hi; exit 2', returncode=2, stdout='hi\n', stderr='') ['/bin/sh']
[]
[]
['/bin/true']
FileNotFoundError [Errno 2] No such file or directory: '/nonexistent'
PermissionError [Errno 13] Permission denied: '/tmp/plain'
0 'spawned\n'
0 'path\n'
0 'bar\n'
b'piped\n' 0
True
15
True
15
FileNotFoundError [Errno 2] No such file or directory: '/nonexistent'
ValueError posix_spawn: argv must not be empty
TypeError Unknown file_actions identifier
TypeError Each file_actions element must be a non-empty tuple
TypeError A dup2 file_action tuple must have 3 elements
OSError [Errno 9] Bad file descriptor
ValueError signal number 0 out of range [1; 64]
PermissionError [Errno 1] Operation not permitted: '/bin/true'
TypeError must have a sched_param object
FileNotFoundError [Errno 2] No such file or directory: '/bin/true'
FileNotFoundError [Errno 2] No such file or directory: 'nonexistent-prog'
"##
    );
}

/// O `subprocess.py` do disco do Debian sobre o `_posixsubprocess.fork_exec` do sandbox: as falhas de `exec` e de
/// `chdir` chegam pelo `errpipe`, `umask` e `preexec_fn` valem só para o filho.
#[test]
fn subprocess_from_disk_over_fork_exec() {
    let src = r##"
import os, subprocess, _posixsubprocess

print(subprocess.run(['/bin/sh', '-c', 'echo oi; echo erro >&2; exit 3'], capture_output=True))
print(repr(subprocess.check_output(['/bin/echo', 'a b'], text=True)))
try:
    subprocess.run(['/bin/sh', '-c', 'exit 5'], check=True)
except subprocess.CalledProcessError as e:
    print(e, e.returncode, e.cmd)
print(subprocess.CalledProcessError(-9, 'x'))
for args, kw in ((['/nonexistent'], {}), (['nonexistent-cmd'], {}), (['/bin/true'], {'cwd': '/nonexistent'})):
    try:
        subprocess.run(args, **kw)
    except OSError as e:
        print(type(e).__name__, e)
try:
    subprocess.run(['/bin/sleep', '30'], timeout=0.2)
except subprocess.TimeoutExpired as e:
    print(e)
print(subprocess.getstatusoutput('echo hi; exit 4'))
print(subprocess.run(['/bin/cat'], input=b'x\ny\n', capture_output=True).stdout)
print(repr(subprocess.run(['/bin/sh', '-c', 'echo $A'], env={'A': '1'}, capture_output=True, text=True).stdout))
p = subprocess.Popen(['/bin/true'])
print(p.wait(), repr(p))
before = os.umask(0o22)
print(subprocess.run(['/bin/sh', '-c', 'umask'], umask=0o27, capture_output=True).stdout, os.umask(before) == 0o22)
print(subprocess.run(['/bin/sh', '-c', 'umask'], preexec_fn=lambda: os.umask(0o77), capture_output=True).stdout)
def boom():
    raise ValueError('x')
try:
    subprocess.run(['/bin/true'], preexec_fn=boom)
except subprocess.SubprocessError as e:
    print(type(e).__name__, e)
try:
    _posixsubprocess.fork_exec(1, 2)
except TypeError as e:
    print(e)
"##;
    assert_eq!(
        python(src),
        r##"CompletedProcess(args=['/bin/sh', '-c', 'echo oi; echo erro >&2; exit 3'], returncode=3, stdout=b'oi\n', stderr=b'erro\n')
'a b\n'
Command '['/bin/sh', '-c', 'exit 5']' returned non-zero exit status 5. 5 ['/bin/sh', '-c', 'exit 5']
Command 'x' died with <Signals.SIGKILL: 9>.
FileNotFoundError [Errno 2] No such file or directory: '/nonexistent'
FileNotFoundError [Errno 2] No such file or directory: 'nonexistent-cmd'
FileNotFoundError [Errno 2] No such file or directory: '/nonexistent'
Command '['/bin/sleep', '30']' timed out after 0.2 seconds
(4, 'hi')
b'x\ny\n'
'1\n'
0 <Popen: returncode: 0 args: ['/bin/true']>
b'0027\n' True
b'0077\n'
SubprocessError Exception occurred in preexec_fn.
fork_exec expected 23 arguments, got 2
"##
    );
}
