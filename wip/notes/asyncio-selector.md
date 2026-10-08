# asyncio sobre selectors: plano para tirar o asyncio_loopback

## Como o asyncio do sandbox é montado hoje (código feito, falta compilar e rodar)

- Todo o pacote vem verbatim do disco do Debian (`kernel/image/usr/lib/python3.13/asyncio/`, via `modules/pysrc.rs`): `base_events`,
  `selector_events`, `unix_events`, `sslproto`, `staggered`, `base_subprocess`, `events`, `futures`, `tasks`, `locks`, `streams` etc.
  Medido por `grep -vxFf`: as antigas cópias em `modules/py/asyncio_*.py` eram idênticas ao disco (salvo `events`, que trocava
  `socket.AF_UNSPEC`/`AI_PASSIVE` por 0/1 para não importar `socket`), então a troca não muda nada além do que `base_events`,
  `unix_events` e `selector_events` agora trazem. `asyncio.subprocess` também é o do disco (fatia 9).
- `_asyncio` é o shim `modules/py/_asyncio.py`: `Future` e `Task` nascem sob demanda (`__getattr__`) como subclasses de `_PyFuture` e
  `_PyTask` com `__module__ = '_asyncio'`, `Task(_PyTask, Future)`; os quadros seguem escondidos por `C_ACCELERATED` (`vm.rs`) e o
  arquivo está em `native_in_cpython`. As funções (`_register_task`, `get_running_loop`...) continuam as de Python (o `from _asyncio import`
  delas falha com ImportError, como no `except` do disco; a lista `C_ACCELERATED` as esconde).
- Sumiram `asyncio.loopback` (`NetworkMixin`, `_SelectorSocketTransport` à mão), `_net.watch`/`unwatch`/`_watches` e o `_io_ready` do
  laço; `_net._poller` só espera os fds de `wait_fd`. O laço é o `_UnixSelectorEventLoop` sobre `selectors.DefaultSelector`
  (`EpollSelector`, fatia 3 implícita) e o self-pipe é `socket.socketpair()`.
- Arquivos órfãos a remover com `git rm` (nada os inclui mais): `modules/py/asyncio_{base_events,base_futures,events,futures,locks,
  loopback,mixins,queues,runners,streams,taskgroups,tasks,threads,timeouts,unix_events,subprocess}.py`.
- Teste: `stdlib_tests.rs::asyncio_selector_loop_over_kernel_sockets` (servidor e cliente no mesmo processo, `sock_*`, recusa, `wait_for`);
  dois processos: caso `asyncio-two-processes` em `testbench/corpus/cases/python/agent.toml`.

## Bloqueios concretos

1. `select.py` agora é só o `poll(2)` do kernel (sem estado em processo: todo socket do Python é um fd do kernel, ver "Sockets no kernel"
   abaixo). Sem `select.epoll`/`poll`, então `selectors.DefaultSelector` cai em `SelectSelector`; o CPython real usa `EpollSelector`
   (ordem de eventos, `EPOLLIN|EPOLLOUT`, `modify`, `unregister` de fd fechado). Para conformidade: `select.epoll`
   (epoll_create1/ctl/wait no kernel, flags EPOLL*), `select.poll`, constantes `EPOLL*`, `PIPE_BUF`.
2. `socket.socketpair()` (self-pipe do `_make_self_pipe`) devolve pares do `_net`/`_os.unix_socketpair`: conferir `setblocking(False)`,
   `recv(4096)` e `send(b'\0')` com EAGAIN, e que o fd entre no epoll.
3. `signal.set_wakeup_fd` é stub `-1`; `signal.signal` não tem entrega por fd; `pthread_sigmask`/`sigpending` stubs. Necessários
   para `add_signal_handler` (unix_events usa `signal.set_wakeup_fd(self._csock.fileno())`, `siginterrupt`, e lê o número do sinal
   do socket).
4. `os.set_blocking`/`get_blocking`, `os.pipe`, `os.read/write` não bloqueantes com `BlockingIOError` (EAGAIN) reais, `os.set_inheritable`.
5. `_socket`: `connect` não bloqueante (EINPROGRESS), `getsockopt(SO_ERROR)`, `accept` não bloqueante, `recv_into`, `send` parcial,
   `sendmsg`/`recvmsg`, `shutdown(SHUT_WR)` `getpeername`; `SO_REUSEADDR`/`TCP_NODELAY` via `setsockopt` (`_set_nodelay`);
   `sock.sendfile`/`os.sendfile` (`loop.sock_sendfile`, opcional com fallback).
6. Processos filhos: `unix_events` real usa `_PidfdChildWatcher` (3.13: `os.pidfd_open`, ou `ThreadedChildWatcher` com `os.waitpid` em
   thread, `threading.Thread`), `subprocess.Popen` com `stdin/out/err=PIPE` entregando fds reais de pipe, `base_subprocess.py`,
   `_UnixSubprocessTransport`, `_UnixReadPipeTransport`/`_UnixWritePipeTransport` (`stat.S_ISFIFO/S_ISSOCK/S_ISCHR`,
   `os.fstat`), `socket.socketpair` para stdio. `_asyncio` ausente: rodar o verbatim Python puro.
7. `ssl`: `sslproto.py` verbatim sobre `_ssl.MemoryBIO`/`SSLObject` (o `_ssl` do sandbox hoje tem cliente TLS 1.3 próprio).
8. `unix_events`: `create_unix_connection/server` (`socket.AF_UNIX`, `os.stat` para remover socket obsoleto, `os.chmod`).
9. `staggered`/happy eyeballs em `create_connection`: `socket.getaddrinfo` real, `loop.getaddrinfo` via `run_in_executor` (threads).
10. `threading`/`concurrent.futures` para executores e `call_soon_threadsafe` (`_write_to_self` de outra thread).
11. Mensagens e `repr` (`<_SelectorSocketTransport fd=7 read=polling write=<idle, bufsize=0>>`) saem sozinhas com o verbatim;
    o custo é desempenho do interpretador do sandbox no `_run_once`.

## Fatias (cada uma com caso no oráculo)

1. FEITA (código; falta compilar e conferir no oráculo): `select.select` sobre o kernel. `_os.poll(pares (fd, events), timeout)` é a nativa
   (`osnative.rs`, só em `_os`, que é INTERNAL); `select.py` usa o `poll(2)` para todo fd (fd inválido: EBADF; sem "sempre pronto"; sem
   estado de socket em processo; sem trava de "espera eterna": lista vazia sem prazo bloqueia, como no Linux). Máscaras do `select(2)`:
   leitura `IN|HUP|ERR`, escrita `OUT|ERR`, exceção `PRI`. Validações na ordem do CPython: timeout (NaN, tipo, overflow, negativo) antes
   das listas; fd >= 1024 `ValueError`. A espera usa o poll do kernel em fatias (50 ms), rodando as threads cooperativas entre elas.
   Pendente: casos no oráculo (pipe, tty, arquivo, fd fechado, fd 1024, timeout nan/-1/str/1e30, socket com SO_ERROR de connect recusado).
2. FEITA (código; falta compilar e conferir no oráculo): `select.poll` e `select.epoll`. O módulo mora em `modules/py/_select.py`
   (INTERNAL) e `select.py` só reexporta os nomes do `builtin-dir.tsv`. Constantes `POLL*`, `EPOLL*`, `EPOLL_CLOEXEC`, `PIPE_BUF`; sem
   `kqueue`/`devpoll`. `select.poll()` (função embutida que devolve o objeto `select.poll`): `register`/`modify`/`unregister`/`poll`
   sobre o `_os.poll`, `modify` de fd ausente é `FileNotFoundError` (ENOENT), `unregister` ausente é `KeyError(fd)`, timeout em ms
   arredondado para cima, `RuntimeError('concurrent poll() invocation')`. `select.epoll` é um fd real: `epoll_create1`/`epoll_ctl`/
   `epoll_wait` em `sysabi` (`Syscalls`, com `EpollEvent` e o módulo `sysabi::epoll`) e no kernel (`crates/kernel/src/epoll.rs`,
   `FileObj::Epoll`, `Task::poll_ofd` compartilhado com o `poll(2)`). `/proc/self/fd` mostra `anon_inode:[eventpoll]`, `fstat` dá
   modo `0600` sem bits de tipo, `read`/`write` dão EINVAL, `lseek`/`pread` ESPIPE; `epoll_ctl` confere na ordem do `do_epoll_ctl` (EBADF, EPERM em
   arquivo regular, EINVAL em si mesmo ou fd que não é epoll, EEXIST/ENOENT, `EPOLLEXCLUSIVE`, ELOOP em ciclo); `epoll_wait` valida
   `maxevents` antes do fd. Nível, `EPOLLONESHOT` (MOD rearma), entradas de nível giram para o fim da fila, entrada some quando a
   última referência à descrição fecha, epoll dentro de epoll. Testes: `crates/kernel/tests/epoll.rs`. No Python: `epoll(sizehint,
   flags)` com as mensagens do Argument Clinic, `close`/`closed`/`fileno`/`fromfd`/`register`/`modify`/`unregister`/`poll(timeout s,
   maxevents)`/`__enter__`/`__exit__`, `ValueError('I/O operation on closed epoll object')`; o fd fecha na coleta do objeto.
   Fechado (código; falta compilar): (a) `EPOLLET` como o `ep_poll_callback`: cada entrada tem um `Parker` próprio (`Item::wake`), registrado
   nas filas do arquivo no mesmo `poll` de cada passada de `epoll_wait` e consultado depois dele (`Parker::take_notified`); chegada nova
   com o fd já pronto reentrega, ler parte sem chegada não, `EPOLL_CTL_MOD` zera; o despertar filtra pela chave como o Linux (código; falta compilar):
   `WaitList::take_key(key)` (`park.rs`, módulo `park::key` com `READ`, `WRITE`, `PIPE_READ`, `PIPE_WRITE`) só acorda os parkers cujo
   `Parker::filter` cruza a chave e deixa os outros na fila; `take()` é a chave 0 (close, HUP, shutdown: acorda todos). O filtro do parker
   de uma entrada é `events | ERR | HUP` (sem filtro se o alvo é outro epoll); o parker do `poll(2)` e o da espera do `epoll_wait` não têm
   filtro, só ganham despertares a mais. Chaves: `pipe_read` acorda escritores com `OUT|WRNORM`, `pipe_write` leitores com `IN|RDNORM`,
   entrega em socket (stream, dgram, seqpacket, udp, fila do listener) com `READ`, o leitor de seqpacket libera o escritor com `WRITE`;
   tty e o resto seguem sem chave. Testes `edge_other_direction` e `edge_hup`. (b) `fdinfo` do epoll
   com as linhas `tfd:` (`Epoll::fdinfo_lines`, `FdInfo::extra`; `st_dev`/`st_ino` do alvo por `sys::ofd_stat_in`, `sdev` no `dev_t` do
   kernel; a ordem é a da rbtree do kernel, por ponteiro, aqui por descrição e fd). (c) medido no oráculo: `st_dev` 16, `st_ino` 58, `mnt_id`
   17, modo `0600` sem bit de tipo (não é `S_IFREG`), `nlink` 1, `blksize` 4096. (d) mensagens medidas: `sizehint` 0 ou menor que -1 é
   `ValueError('negative sizehint')`, aridade do `epoll()` é `epoll() takes at most 2 arguments (3 given)`, docstring do 3.13. (e) o parker de
   cada entrada sai das filas do arquivo como no `ep_remove`/`ep_unregister_pollwait` (código; falta compilar): `EPOLL_CTL_DEL` (`Epoll::delete`,
   sob a trava da lista, então despertar concorrente ou chega antes ou não acha mais a entrada; o parker da espera do `epoll_wait` é outro e
   fica), close do epoll (`impl Drop for Epoll`, o `ep_free`) e close da descrição alvo (`Ofd::watchers`, o `f_ep`: `Drop for Ofd` chama
   `Epoll::release`, o `eventpoll_release`). `WaitList::unregister` remove por identidade e para no primeiro (a ordem de despertar fica);
   `sys::unregister_ofd(ofd, parker)` virou função livre que recebe o parker. `Epoll::scan` segura os `Arc<Ofd>` até soltar a trava da lista
   (um `Drop` que fosse o último travaria `items` de novo) e não descarta mais a entrada de descrição morta: quem tira é o `release`. Testes
   `removed_entries_leave_the_wait_queues` (`epoll.rs`) e `unregister_removes_only_that_parker_and_keeps_order` (`park.rs`).
   Pendente: casos no oráculo (`/proc/self/fd`, `selectors.DefaultSelector` virando `EpollSelector`, `poll`/`epoll` com pipe, socket, fd
   fechado, `maxevents`, timeouts, `fdinfo` com `tfd:` de socket e arquivo); mensagens de aridade dos métodos (`fromfd`, `unregister`,
   `register`) ainda de memória; testes sem kernel em `stdlib_tests.rs` (`select_argument_errors_match_cpython`).
3. `selectors.DefaultSelector` passa a ser `EpollSelector` (nada a mudar no verbatim; caso de `selectors` completo).
4. `os.set_blocking/get_blocking`, `os.pipe` não bloqueante com EAGAIN; `socketpair` não bloqueante; casos de `BlockingIOError`.
5. FEITA (código; falta compilar): `signal.set_wakeup_fd(fd, /, *, warn_on_full_buffer=True)` em `modules/py/signal.py`: `TypeError` se o
   fd não é inteiro, `ValueError('set_wakeup_fd only works in main thread of the main interpreter')` fora da thread principal,
   `ValueError('the fd N must be in non-blocking mode')` com fd bloqueante (`os.get_blocking`, novo: `_os.get_blocking` sobre
   `get_status_flags`), `OSError` (EBADF) com fd inválido, devolve o fd anterior (-1 no início). O byte do número do sinal é escrito
   na chegada (`_write_wakeup`, o `trip_signal`): `_dispatch` escreve todos os sinais da leva antes de rodar qualquer tratador, e
   `raise_signal` escreve antes do tratador. Falha de escrita vai ao `sys.unraisablehook` com `Exception ignored when trying to write
   to the signal wakeup fd` (o `EAGAIN` só com `warn_on_full_buffer`). `signal.siginterrupt(sig, flag, /)`: valida o sinal e o tipo do
   `flag`, `OSError(22)` para SIGKILL/SIGSTOP, guarda o `SA_RESTART` em `_restart` (o kernel do sandbox não distingue; `signal.signal` o
   apaga). Com isso `loop.add_signal_handler`/`remove_signal_handler` do `unix_events` real funcionam sobre o self-pipe. Falta:
   `pthread_sigmask`/`sigpending` (stubs), `signal.signal` fora da thread principal (`ValueError`), caso no oráculo de
   `set_wakeup_fd` (mensagens, fd de socket, `warn_on_full_buffer` com pipe cheio: o testkit não devolve EAGAIN ao pipe cheio, só o kernel).
6. FEITA (código; falta compilar): os verbatim entram de uma vez (as reescritas eram cópias), `_asyncio` como shim.
7. FEITA (código; falta compilar): `SelectorEventLoop` real ligado. Pendente de rodar: `create_connection`/`create_server`/streams/
   `sock_*` (teste novo), `start_unix_server`, `add_signal_handler` (precisa da fatia 5), `loop.getaddrinfo` por nome (executor),
   `asyncio.run` com `shutdown_asyncgens` (os ganchos `sys.set_asyncgen_hooks` agora são chamados pelo `base_events` real: conferir se o VM
   chama `firstiter`/`finalizer` e se o gerador assíncrono aceita referência fraca).
8. FEITA: `asyncio_loopback.py`, `INTERNAL` e `NetworkMixin` fora do código; sobra o `git rm` dos arquivos órfãos.
9. FEITA (código; falta compilar): `asyncio.subprocess` é o `subprocess.py` do disco (`pysrc.rs` aponta o arquivo da imagem); o laço usa
   `_UnixSubprocessTransport` e o `PidfdChildWatcher`, porque `can_use_pidfd()` agora é verdadeiro. `os.pidfd_open(pid, flags=0)`
   (`os.py` -> `_os.pidfd_open` -> `Syscalls::pidfd_open`) é um fd real do kernel: `FileObj::Pidfd` (`crates/kernel/src/pidfd.rs`), o
   `anon_inode:[pidfd]` de `/proc/<pid>/fd` (fdinfo com `Pid:` e `NSpid:`), mesmo inode/dev de anon do epoll, `read`/`write` EINVAL, `lseek`
   ESPIPE. Legível (`IN`) quando o processo vira zumbi, `IN|HUP` quando o pai o colhe (`pidfd_poll`), pollável e usável em epoll; o
   despertar vem de `Proc::death` (`Death`: `exit()` em `finish_process`, `reap()` em `Table::reap`, sob uma trava só). Erros: pid <= 0 ou
   flags desconhecidas EINVAL, pid inexistente ESRCH, tid de thread que não é líder ENOENT (com `PIDFD_THREAD`, `0o200`, vale). Testes:
   `crates/kernel/tests/pidfd.rs` (`lifecycle`, `epoll`, `errors`, `zombie`) e `stdlib_tests.rs::asyncio_subprocess_over_pidfd_child_watcher`.
   Arquivo órfão a remover com `git rm`: `crates/ul-python/src/modules/py/asyncio_subprocess.py`. Falta: `pidfd_send_signal`/`os.waitid(P_PIDFD)`,
   `PIDFD_GET_INFO`, casos no oráculo (`/proc/self/fdinfo` do pidfd, `poll` antes e depois do `waitpid`), e o `Popen` do sandbox
   (`py/subprocess.py`) ainda é próprio, não o do disco (`_posixsubprocess`/`_fork_exec`).
10. `sslproto` verbatim e `_ssl.MemoryBIO`.

## Sockets no kernel (fatia feita; código sem compilar, falta rodar e conferir no oráculo)

Todo socket do Python (`AF_INET`/`AF_INET6` TCP e UDP, `AF_UNIX` stream/dgram/seqpacket, `socketpair`) é um fd real do kernel desde o
`socket()`: `_os.tcp_socket`, `_os.udp_socket`, `_os.unix_socket`, `_os.unix_socketpair`. Sumiram `_socket._fds`, os fds falsos a partir
de 100, `_held`, `_State`, os `Endpoint`/`Listener`/`Datagram`/`KernelEndpoint` do `_net` e o `_kpoll`. O que mora no kernel:
`Listener` TCP sem endereço (`Ports::socket`, `bind_socket`), `connect` não bloqueante (`tcp_connect_fd`: EINPROGRESS, recusa em
`SO_ERROR`/poll/`connect` seguinte), `SO_ERROR` (`sock_error`), opções (`sock_setopt`/`sock_getopt`, no `Ofd.sock`), `recv` e `send` de
fluxo com flags (`sock_recv`/`sock_send`: `MsgFlags` `PEEK`, `DONTWAIT`, `WAITALL`, `NOSIGNAL`), nomes (`tcp_names`), `sock_info`
(SO_DOMAIN/TYPE/PROTOCOL/ACCEPTCONN). O Python guarda só família, tipo, protocolo e
timeout (como o CPython), liga `O_NONBLOCK` pelo timeout e espera por `poll` cooperativo (`_net.wait_fd`) antes de chamar o kernel.
O asyncio é o do disco, sobre `select.epoll` (ver o início).

Semântica de erro de fluxo (net/ipv4/tcp.c e af_unix.c do 6.12; código sem compilar, testes em `integration.rs`: `tcp-send-unconnected`,
`tcp-recv-unconnected`, `tcp-refused`, `tcp-rst`, `tcp-peer-closed`, `unix-stream-epipe`, `tcp-recv-flags`, `sock-info-opts`):
- `send`/`write` em TCP em CLOSE ou LISTEN: o `sk_err` pendente uma vez, senão EPIPE com SIGPIPE (salvo `MSG_NOSIGNAL`); nunca ENOTCONN.
- `recv` em TCP: LISTEN e nunca conectado, ENOTCONN; depois de `shutdown(SHUT_RD)` o que já chegou sai e a fila vazia dá 0 (o par continua
  escrevendo; `Pipe::shutdown_read`; no TCP o par segue escrevendo). `connect` não bloqueante recusado: `recv` dá ECONNREFUSED uma vez e depois 0, `send` EPIPE, poll
  `IN|OUT|HUP|RDHUP` (+`ERR` até o `SO_ERROR`); o `connect` bloqueante recusado deixa o socket como novo (`tcp_disconnect`).
- Estado de RST da `Conn` agora tem `ECONNRESET` pendente (par fechou com dado não lido) e `EPIPE` pendente (o par que já fechou respondeu à
  escrita com RST, ponta em CLOSE_WAIT); `take_error` serve `SO_ERROR`, `send` e o fim da fila no `recv` (só o `ECONNRESET`: com o `SOCK_DONE`
  do FIN o `tcp_recvmsg` devolve 0 sem olhar o `sk_err`). Os dados já na fila saem antes do erro. Antes dava ECONNRESET na escrita seguinte ao par
  fechado; o Linux dá EPIPE.
- Poll de TCP estabelecido no `tcp_poll`: FIN do par é `IN|RDHUP` (sem HUP), HUP só com `SHUTDOWN_MASK` (RST, ou FIN mais `SHUT_WR`), `ERR` com `sk_err`.
- AF_UNIX de fluxo: escrita com o par fechado é EPIPE na hora (antes contava como feita), sem tocar no erro pendente da leitura.
- `MSG_WAITALL`: insiste até o tamanho pedido, para no fim ou erro com dado já copiado, e com `O_NONBLOCK`/`MSG_DONTWAIT` leva o que tem
  (EAGAIN se vazio); `MSG_PEEK|MSG_WAITALL` espera a fila ter o tamanho pedido. `_socket._call` só liga `O_NONBLOCK` por `MSG_DONTWAIT` sem timeout
  (com timeout o CPython espera o prazo e depois recebe).
- Opções: `SO_RCVBUF`/`SO_SNDBUF` dobrados, mínimos e padrão 131072/16384 ficam no `_socket.py` (`_OPTIONS`); o kernel só guarda os bytes.
- `SO_SNDBUF` de um TCP estabelecido (`tcp_sndbuf_expand` no `tcp_init_buffer_space`): no loopback vale 2626560 (2 * 10 segmentos *
  (roundup_pow_of_two(65495 + 256 + 320) + 256), teto `tcp_wmem[2]` 4194304) no `connect` e no `accept`. `SockMeta.sndbuf_expanded` o faz
  aparecer em `sock_getopt` quando ninguém definiu a opção; `setsockopt` antes trava o buffer, e o aceito herda o valor do que escuta.
- `shutdown(SHUT_RD)` agora é `RCV_SHUTDOWN` no `Pipe` (`shutdown_read`, sob a trava dele, sem perder o aviso): TCP e AF_UNIX de fluxo
  entregam o que há na fila e depois 0; no Unix o par recebe EPIPE (`wr_closed`, acorda escritores bloqueados); `shutdown(SHUT_WR)` acorda
  o escritor bloqueado do próprio lado. Antes o Unix soltava a ponta de leitura e a fila sumia.
- Acordar quem espera: o `recv` bloqueado acorda com `shutdown(SHUT_RD)`; `shutdown(SHUT_RD)` num listener TCP desfaz a escuta
  (`stop_listening`: `accept` bloqueado dá EINVAL, `connect` ECONNREFUSED, poll `OUT|HUP`; o teste de `listening` do `try_accept` passou
  para dentro da trava da fila); o `connect` AF_UNIX esperando vaga na fila não segura mais o listener (`Weak`), então o `Drop` o acorda e
  ele dá ECONNREFUSED. Dado, FIN e RST já acordavam pelo `PipeEnd::drop`. Testes: `tcp_sndbuf_grows_after_the_connection_is_established`,
  `shutdown_wakes_a_blocked_recv_and_a_blocked_accept`, `unix_stream_shutdown_rd_keeps_the_queue_and_refuses_the_peer`,
  `unix_connect_waiting_for_the_queue_wakes_when_the_listener_closes` (código sem compilar).

Ficou para a próxima:
- Casos no oráculo para: fd do primeiro socket (3), `/proc/self/fd`, `os.close`/`os.fstat`/`os.set_blocking` no `fileno()`, `getsockopt`
  de opções após `setsockopt`, `connect` não bloqueante recusado, `recv(MSG_PEEK)`, `socketpair` com `SOCK_NONBLOCK`; e os cenários de erro acima
  (recv/send não conectado, RST, escrita após o par fechar, `MSG_WAITALL`, `shutdown(SHUT_RD)` com dado na fila).
- As opções só são guardadas: `SO_RCVTIMEO`/`TCP_NODELAY` não mudam o comportamento do kernel (a mensagem do seqpacket maior que o
  `SO_SNDBUF` ainda é conferida no Python); a normalização do `setsockopt` segue em Python. O `SO_RCVBUF` não cresce no Linux (medido no
  oráculo: 131072 antes e depois do `connect`); o kernel não guarda valor e o padrão 131072 vem do `_socket.py`, então não há pendência.
- `SO_ERROR` do `err` do UDP, que o `UdpSock` já guarda; `unix_poll` ainda usa o poll de pipe (só `IN|OUT|RDHUP|HUP` do `SHUT_RD`/`SHUT_WR`
  foram ajustados). Feitos: `shutdown` num AF_UNIX não conectado devolve 0 como o `unix_shutdown`; fechar um listener (TCP ou Unix) com
  conexão na fila dá ECONNRESET ao cliente (RST do `inet_csk_listen_stop`, `embrion` do `unix_release_sock`).
- `shutdown` em UDP conectado.
- `getpeername`/`recvfrom` de seqpacket sem nome, `AF_UNIX` com caminho de 108 bytes e `OSError('AF_UNIX path too long')`.

## Descritores e dados auxiliares (fechado em código; falta compilar e conferir no oráculo)

- `os.dup`, `os.dup2(fd, fd2, inheritable=True)`, `os.get_inheritable`/`set_inheritable` e `os.set_blocking`/`get_blocking` moram em `os.py`
  sobre `_os.dup`/`dup2`/`get_inheritable`/`set_inheritable`/`set_blocking` (`osnative.rs`) e sobre `dup_min`/`dup3`/`get_cloexec`/
  `set_cloexec`/`get_status_flags`/`set_status_flags` do kernel. Como no CPython: `dup` e `dup2(inheritable=False)` dão fd com `FD_CLOEXEC`
  (o `dup2` herdável com `fd == fd2` só confere o fd, o `dup3` com iguais é EINVAL), fd fora do `int` de C é `OverflowError`, fd ruim EBADF.
  `set_blocking` agora mexe só no `O_NONBLOCK` (antes zerava o `O_APPEND`, porque o `set_status_flags` troca os três bits de uma vez).
  Mensagens de aridade dos wrappers (`dup expected 1 argument, got 0`, `dup2() missing required argument 'fd2' (pos 2)`) ainda são as do
  `def` do Python, não as do Argument Clinic.
- `socket.dup()`, `socket.fromfd`, `detach` e `socket(fileno=...)` já eram coerentes com `_socket.dup` (`dup_min` com CLOEXEC, a descrição é
  compartilhada, então `settimeout` de um vale para o outro, como no Linux); `socket.py` do disco faz o resto. Caso `socket-dup-detach-fromfd`.
- `sendmsg`/`recvmsg`/`recvmsg_into`, `CMSG_LEN`, `CMSG_SPACE`, `SCM_RIGHTS`, `SCM_CREDENTIALS`, `SO_PASSCRED`, `SO_PEERCRED` e as `MSG_*`
  que faltavam (`CTRUNC`, `TRUNC`, `CMSG_CLOEXEC`, `EOR`, `MORE`, `CONFIRM`, `ERRQUEUE`, `FASTOPEN`) no `_socket.py`; `socket.send_fds` e
  `recv_fds` são as do disco (existem porque `_socket.socket` tem `sendmsg`/`recvmsg`); `socket.getpeereid` não existe (Linux).
  `SO_PASSSEC` e `SO_PEERSEC` entram só como constante (o primeiro guarda o bool, o segundo dá ENOPROTOOPT, como sem LSM).
- Kernel: `Syscalls::unix_sendmsg(fd, dados, nome, controle, flags)` e `unix_recvmsg(fd, max, tamanho do controle, flags) -> RecvMsg`
  (`sysabi::cmsg` tem o layout do `struct cmsghdr`, o `parse` do `__scm_send`/`CMSG_OK` e o `Builder` do `put_cmsg`/`scm_detach_fds`).
  Só `AF_UNIX`; no TCP/UDP o `_socket.py` chama `send`/`recv` e recusa `SCM_RIGHTS` com EINVAL (o `__scm_send` só o aceita em `PF_UNIX`).
  - Descritores em trânsito: `kernel/src/scm.rs`. Datagrama e seqpacket levam um `Scm { fds: Vec<Arc<Ofd>>, cred }` por mensagem (a fila
    do `Dgram` virou `Datagram`, a do seqpacket `(Vec<u8>, Scm)`); o fluxo é uma fila de bytes (par de pipes), então as marcas ficam
    no `Pipe` (`Marks`, com os contadores absolutos de escrita e leitura): `try_write_with` deixa uma marca no primeiro byte de um envio com
    descritores (`fd_end` = `min(tamanho, 36544)`, o primeiro `sk_buff` do `unix_stream_sendmsg`) ou com credenciais diferentes das
    anteriores, e `try_read_scm`/`try_peek` calculam a janela da leitura como o `unix_stream_read_generic`: cola dados de vários envios,
    mas encerra no fim do `sk_buff` com descritores, que saem na primeira leitura que o toca (mesmo parcial); com `SO_PASSCRED` para onde
    o remetente muda. `MSG_PEEK` instala cópias e deixa os descritores na fila (`unix_peek_fds`); `read`/`recv` sem `msg_control` os fecham.
  - `recvmsg`: `SCM_CREDENTIALS` (se o receptor tem `SO_PASSCRED`) antes de `SCM_RIGHTS`; cada fd cabe segundo `scm_max_fds`
    (`(controllen - 16) / 4`), o que não coube fecha e marca `MSG_CTRUNC`; sem `msg_control` (tamanho 0), `MSG_CTRUNC` se havia fds ou
    `SO_PASSCRED`; `MSG_CMSG_CLOEXEC` põe `FD_CLOEXEC` nos novos; `MSG_TRUNC` na saída para datagrama e seqpacket cortados.
  - `sendmsg`: controle de 20480 bytes ou mais é ENOBUFS (`optmem_max`); item com `cmsg_len` fora do buffer é EINVAL; nível diferente de
    `SOL_SOCKET` é ignorado; tipo desconhecido é EINVAL; mais de 253 fds é EINVAL; fd negativo ou fechado EBADF; `SCM_CREDENTIALS` confere
    pid, uid e gid como o `scm_check_creds` (root passa; uid/gid -1 é EINVAL). Fluxo com 0 bytes não envia os descritores
    (o laço do `unix_stream_sendmsg` não roda). Toda escrita Unix leva as credenciais reais (`getpid`, `getuid`, `getgid`) de quem escreveu.
  - `SO_PEERCRED` (`sock_getopt`): `struct ucred` com o euid/egid de quem chamou `socketpair` (nas duas pontas), de quem chamou `listen`
    (o `connect` copia para o cliente) e de quem conectou (o embrião aceito); socket sem par, e qualquer TCP/UDP, devolve pid 0 e uid/gid -1.
    `SO_PASSCRED` é um flag do `UnixSock` (`set_passcred`), guardado também em `opts` para o `getsockopt`.
  - Decisões a conferir no oráculo: o 6.12 pode só anexar credenciais ao `sk_buff` se o remetente ou o receptor tinha `SO_PASSCRED` na hora do envio
    (`unix_maybe_add_creds`); aqui sempre se anexa, então a diferença só aparece se o receptor liga a opção depois do envio. Sem coleta de lixo
    de descritores em ciclo (`unix_gc`): um fd de socket que viaja dentro de si mesmo fica retido até o sandbox acabar. `SO_PASSPIDFD`/`SCM_PIDFD`
    e `MSG_OOB` não existem. `recvmsg` em UDP/TCP devolve `msg_flags` 0 (sem `MSG_TRUNC` de datagrama UDP). As mensagens de `TypeError`
    dos itens de `ancdata` malformados (`[sendmsg() ancillary data items]() argument must be ...`) estão de memória.
  - Testes: `crates/kernel/tests/integration.rs` (`scm_rights_*`, `scm_send_*`, `so_passcred_*`, `so_peercred_*`), `crates/sysabi/src/cmsg.rs`,
    `crates/kernel/src/scm.rs`, `stdlib_tests.rs` (`os_dup_dup2_inheritable_and_blocking_follow_cpython`,
    `socket_ancillary_data_helpers_match_cpython`) e, contra o oráculo, `testbench/corpus/cases/python/socket-fds.toml` (`socket-scm-rights-fork`:
    um processo passa um fd de pipe pelo socketpair e o filho lê; `socket-ancdata`, `socket-credentials`, `os-dup-inheritable-blocking`,
    `socket-dup-detach-fromfd`). O `stdlib_tests.rs` roda no kernel de teste, que não tem sockets: os casos com socket só podem ser da bancada.
