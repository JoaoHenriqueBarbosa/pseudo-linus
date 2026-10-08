# Threads do Python no ul-python: decisão técnica e fatias

## 1. O problema

`threading.py` e `_thread.py` rodavam as threads aninhadas: a thread nova era chamada (`_bootstrap()`) dentro da
espera de quem bloqueava, na mesma pilha. Consequências, todas visíveis ao agente e todas diferentes do CPython:

- `start()` só enfileirava; a thread não rodava até alguém bloquear;
- uma thread que bloqueava não podia ser retomada: ou rodava outras por cima dela (e só voltava quando elas
  acabavam) ou era "estacionada" (`_Parked`, a pilha desenrolada, a thread perdida);
- `acquire` bloqueante, `Condition.wait/notify`, `Event`, `Barrier` e `queue` com produtor e consumidor só
  funcionavam quando a ordem de bloqueio casava com a ordem aninhada;
- um deadlock real virava `RuntimeError('deadlock: ...')`, que o CPython nunca levanta.

O comportamento observável a reproduzir é o do CPython 3.13 com GIL: `start()` devolve depois que a thread nova
rodou até bloquear ou acabar; thread bloqueada fica suspensa e volta quando a condição vale; troca a cada
intervalo; `time.sleep` solta a GIL.

## 2. Opções consideradas

### A. Uma thread do host por thread Python, com GIL

Cada thread Python seria uma thread do host (e uma tarefa do kernel via `spawn_thread`/`clone`), com uma GIL
passando o bastão. É o modelo do CPython e daria `/proc/PID/task`, `ps -L` e `Threads:` fiéis de graça.

Descartada como base, por três razões medidas no código:

1. `Rc` não é `Send`. O estado todo da `Vm` (`globals`, `handled`, `frames_stack`, `modules`...) e todo `Value`
   são `Rc`. Passar a `Vm` para outra thread do host exige `unsafe impl Send`, e o projeto é `forbid(unsafe)`
   (a imagem do heap em `heapimage.rs` existe justamente para copiar o estado sem `unsafe`).
2. O estado por processo está em `thread_local!` da thread do interpretador: `vm::CURRENT`, `SIGNALS`
   (alarme, tratadores), `fork::REQUEST/MAIN/TAIL`, `tracing`, `frameobj`, `finalize`, `RECURSION_LIMIT`,
   `sysabi::sys::CURRENT` (o pseudo-processo). Uma segunda thread do host enxergaria tudo isso vazio. Mover
   cada um para um objeto compartilhado e protegido por lock é uma reescrita do interpretador inteiro.
3. O modelo do kernel é "uma thread do host por processo" com o pseudo-processo instalado nela; o
   `spawn_thread` do kernel cria tarefas com corpo próprio (`ThreadFn`), não um contexto de interpretador.

### B. Corrotinas da VM (escolhida)

Uma thread Python é um segmento da pilha explícita que a VM já tem: o quadro mais externo, os quadros que
esperam em `Vm::frames_stack` e o chamado em execução (`child` em `run_frames`), mais o estado por thread que a
VM guarda em campos (`handled`, `frames`, `depth`, `cur_line`, `globals`). É exatamente o dado que o
`os.fork` já captura (`VmImage::capture_fork`, `Resume`, `run_resumed`). Trocar de thread é trocar esse segmento
dentro do próprio laço, sem recursão Rust, sem thread do host, sem `Send`, sem `unsafe`, e sem mexer no modelo
do kernel.

Ela também é determinística por construção: não há relógio nem escalonador do host decidindo quem roda.

## 3. Desenho

### 3.1 Primitiva na VM (`crates/ul-python/src/gthread.rs`)

Modelo simétrico, como o `greenlet`. Nativas no módulo `_sys` (interno):

| nativa | efeito |
|---|---|
| `_gt_spawn(callable) -> tid` | cria a thread verde, ainda sem rodar |
| `_gt_switch(tid, valor=None) -> valor` | suspende a atual, retoma `tid` (ou a inicia); devolve o que quem a retomar passar |
| `_gt_finish(tid)` | descarta a pilha da atual e retoma `tid` (`-1` encerra o laço do programa) |
| `_gt_current()`, `_gt_can_switch()` | tid atual (principal é 0); se a troca é possível aqui |

O pedido segue o caminho do `os.fork`: a nativa devolve a exceção de marca (`fork::suspend`) com
`SuspendRequest::Green(..)` e só o ramo de erro da instrução de chamada de `Vm::run_frames` a reconhece
(custo zero no caminho comum). Ali `Vm::green_switch`:

1. valida o alvo e o laço (antes de qualquer troca, então um erro deixa a thread atual intacta);
2. `take_thread`: tira da VM o segmento atual (`outer` vira um quadro marcador, `frames_stack` inteiro,
   `child`, `handled`, `frames`, `depth`, `cur_line`, `globals`);
3. `install_thread` do alvo suspenso, ou, se é nova, `call_or_enter(callable)` sobre o estado limpo;
4. devolve `Landing::Resume(valor)` (o valor entra na pilha do alvo como resultado do `_gt_switch` dele),
   `Started` (thread nova, nada a empilhar) ou `Exit`.

O quadro marcador reaproveita o código do programa com `pc` no fim: se uma thread verde devolvesse para ele,
`run_frames` acusa `SystemError` (a função da thread sempre termina em `_gt_finish`).

### 3.2 Restrição: só no laço mais externo

Igual ao `os.fork` (`rust_nest == 1`): uma chamada Rust ativa (`sorted(key=)`, `import`, tratador de sinal,
finalizador, `iter(callable, sentinel)`, `throw`/`close` de gerador, `list(gen)`) guarda estado que não é
dado. `_gt_can_switch()` informa; nesses pontos o `threading` cai no escalonador aninhado antigo (que continua
no arquivo, atrás de `if _gsched.can_switch()`), sem regressão. Fechar isso é o mesmo trabalho das fatias G1 a
G5 de `python-fork.md`: cada uma que tira um caso de `rust_nest > 1` ensina ao mesmo tempo o fork e as threads.
G2 (feita, ver `python-fork.md`): geradores e correntes retomados pelo `for`, por `next(g)`, `g.send(v)`,
`g.__next__()` e por `yield from`/`await` rodam como quadro da pilha explícita (`CallLink::resuming`), então
a troca de thread dentro deles (inclusive `contextlib.contextmanager` em `__enter__` e `__exit__` sem
exceção, que chamam `next(self.gen)`) funciona com `rust_nest == 1`. Casos: `nest_suspend.toml`.

### 3.3 Política em Python (`modules/py/_gsched.py`)

Toda a política mora em Python; a VM só troca. Regras (as do CPython com GIL, de forma determinística):

- `start()` troca para a thread nova na hora e põe quem iniciou no fim da fila de prontos;
- bloquear (`wait(cond, timeout)`, `sleep`) enfileira a thread com a condição e o prazo e passa a vez;
- quem roda a seguir é a elegível que espera há mais tempo (condição verdadeira, ou prazo vencido), em ordem de
  chegada, avaliando as condições (lambdas pequenas) na pilha de quem escolhe;
- sem elegível, o processo espera de verdade: as fontes externas (`_pollers`, sockets de outros processos), ou
  o prazo mais próximo; sem prazo nem fonte, bloqueia para sempre (`_os.sleep`), como um `acquire` sem saída
  no CPython. Só um sinal interrompe. O `RuntimeError('deadlock')` antigo some;
- o contexto por thread do lado Python (`threading._state['current']`, `_thread._idents`, ganchos de
  `settrace`/`setprofile` via `_sys._swap_hooks`) é instalado por quem troca, antes de trocar;
- a thread acaba em `_gsched._finish`, que escolhe o próximo e chama `_gt_finish`.

Primitivas (`lock`, `RLock`, `Condition`, `Event`, `Semaphore`, `Barrier`, `queue`) continuam as do CPython:
só dependem de `_thread._block`, que desemboca em `_gsched.wait`.

### 3.4 Troca por tempo (fatia seguinte)

O CPython troca a cada 5 ms. Aqui, a cada N instruções (contador no laço, ligado só com mais de uma thread
verde, como `signals_armed`): o laço empilha uma chamada ao `_gsched.tick` como se fosse um `spawn` e o tick
reenfileira a atual como pronta e passa a vez. É por contagem, não por relógio, para a saída seguir
determinística. Sem isso, um laço `while not flag: pass` numa thread espera para sempre (hoje só `sleep`,
travão, condição e `join` cedem a vez).

### 3.5 Tarefas do kernel

O pid continua com uma tarefa só; `get_native_id` é `pid + slot` como antes. Diferença observável restante:
`/proc/PID/task`, `ps -L`, `Threads:` mostram 1. Fatia futura: criar com `spawn_thread` uma tarefa do kernel
por thread verde (corpo parado num futex, vida atrelada à thread verde) e usar o tid dela em `get_native_id`.
Não depende da VM e não muda o escalonamento.

## 4. Estado depois desta fatia (nada compilado nem executado ainda)

Feito (por leitura):

- `gthread.rs` (novo), `SuspendRequest::Green` em `fork.rs`, ramo de troca em `Vm::run_frames` e guarda do
  quadro marcador (`vm.rs`), `call_or_enter` agora `pub(crate)`, nativas registradas em `_sys` (`pysys.rs`),
  `pub mod gthread` em `lib.rs`;
- `_gsched.py` (novo; registrado em `pysrc.rs`, em `INTERNAL` e na lista de arquivos "nativos no CPython" de
  `vm.rs` para ficar fora dos tracebacks);
- `threading.py`: `start()` (e `_RawThread.start`), `_wait_for`, `_run_one`, `_serve`, `_would_park`, `_shutdown`,
  `_before_sleep` e `_after_fork` usam o escalonador verde quando `can_switch()`; números de linha das classes
  do CPython preservados (Condition 269, Thread 858, start 953, `_bootstrap_inner` 1026, join 1061...);
- `_thread.py`: `_block` sem o `RuntimeError` de deadlock.

Pendências, em ordem:

1. Compilar e rodar `threading.toml` (golden em `testbench/golden/python/threading.json`): os casos alvo são
   `threading-thread-attributes`, `thread-lock-blocking-acquire-waits-for-worker`,
   `threading-condition-semantics` e `threading-excepthook-*`. Testes de unidade no `stdlib_tests.rs`:
   ordem de `start`, `Condition` entre threads, `Event`, `Barrier`, `queue` com produtor e consumidor.
2. Troca por tempo (3.4).
3. (feita) `os.py::_cooperative` virou `_net.cooperative()` e já conta as threads verdes (`_gsched.others()`).
4. `fork` feito numa thread secundária: o `outer` capturado é o marcador e o filho acaba no `_gt_finish(-1)`
   em vez de seguir como `MainThread` (CPython: a thread que forka vira a principal do filho).
5. `threading.setprofile` ativo com thread nova: `reports_native_python_call` pode devolver `Entered::Done` para
   a função de entrada (`_gsched.py` conta como C no CPython); a entrada precisa ficar fora dessa regra.
6. Tarefas do kernel por thread (3.5) e a restrição `rust_nest == 1` (3.2).
7. `heapimage`: as threads suspensas ficam fora da imagem do fork (correto: o filho só leva a que chamou),
   mas o registro Python (`_gsched._queue`) é limpo por `after_fork`, conferir no teste.

## 5. Esperas bloqueantes do processo (achado por leitura, nada compilado nem executado)

Regra: toda espera bloqueante do processo que uma thread dele possa satisfazer cede o turno antes de entrar no
kernel (`_net.cooperative()` diz se há alguém a rodar; sem ninguém, o bloqueio direto no kernel segue como era).

- Causa do `subprocess.run`/`check_output` pendurado com o servidor HTTP numa thread do processo: o `communicate`
  do disco, com só o `stdout` em pipe, faz `self.stdout.read()`, que cai em `io.FileIO.read(-1)` e este chamava
  `_os.read(fd, -1)` direto, bloqueado no kernel até o EOF do pipe. O filho (`curl`, `python3 -c`) esperava a resposta
  da thread do servidor, que nunca rodava (a thread do host do processo estava parada no `read`). Com `stderr` também
  em pipe o caminho era o do `selectors` (cooperativo), por isso `capture_output=True` funcionava e `stdout=PIPE` não.
- Correção: `_net.read(fd, n)` (espera `POLLIN` por `wait_fd` e só então lê; `n < 0` lê até o EOF por pedaços; fd não
  bloqueante vai direto ao kernel) usado por `os.read` e por `FileIO.read`/`readall`/`readline`. `_cooperative` saiu de
  `os.py` e virou `_net.cooperative`. `os.waitid` bloqueante passou pelo `_wait_child` (fatias de `WNOHANG`), como `waitpid`.
- Ainda bloqueia o processo inteiro: `eventfd_write` de valor maior que a folga do contador com `POLLOUT` ainda aceso
  (o kernel decide por soma, o `poll` por folga > 1).
- Leitura do `sys.stdin` (por leitura, nada compilado nem executado): `stdin.rs` deixou de ler dentro das operações.
  Cada uma (`text_line`, `text_chars`, `text_all`, `bytes_read`, `bytes_read1`, `bytes_line`) é função pura sobre o
  buffer que devolve `Stall` quando falta byte e o fd 0 não acabou; o `drive` larga o empréstimo do arquivo, chama
  `_net.wait_readable(0)` (via `vm::current()`, só se `threading` já foi importado) e então faz o `sys::read` do kernel,
  repetindo até a operação bastar. Sem o empréstimo preso, outra thread pode usar o `sys.stdin` durante a espera.
  `_net.wait_readable` = `wait_fd(POLLIN)` quando `cooperative_fd(fd)` (há quem rodar e o fd é bloqueante); fora disso
  volta e o kernel bloqueia como antes (EOF, `EAGAIN` de fd não bloqueante e linha do tty canônico seguem do kernel, o
  `POLLIN` do tty só acende com a linha inteira). Chamado de nativa, o `wait_fd` roda aninhado (`rust_nest > 1`), então
  vale o escalonador aninhado do `threading`, não a troca de pilha. Os pontos de entrada são `file_readline`, o
  `read(n)` em `vm.rs`, `b_input` e `stdbuf.rs`; `_net.cooperative_fd` agora serve também a `read` e `write`.
  Em aberto: o erro do kernel no `fill` ainda vira EOF (como antes), então `sys.stdin.readline()` em fd não bloqueante
  vazio devolve `''` em vez do `None`/`TypeError` do CPython (o caso `threading-stdin-nonblocking-pipe-with-live-thread`
  mostra a diferença assim que o golden do oráculo existir).
- Casos novos em `threading.toml` (golden do oráculo): `threading-stdin-readline-fed-by-thread-via-dup2`,
  `threading-input-fed-by-thread-via-dup2`, `threading-stdin-read-and-iteration-fed-by-thread-via-dup2`,
  `threading-stdin-buffer-read-read1-readline-fed-by-thread-via-dup2`, `threading-stdin-nonblocking-pipe-with-live-thread`.
- Fatia seguinte (por leitura, nada compilado nem executado): `_net.write(fd, data)` espera `POLLOUT` por `wait_fd` e
  escreve pedaços de `PIPE_BUF` (cada um cabe, o kernel não bloqueia; escrita até `PIPE_BUF` continua atômica, um
  pedaço só; `BrokenPipeError` com algo já escrito devolve o total, como o Linux). Só vale para FIFO bloqueante com
  alguém para rodar: fd não bloqueante, arquivo comum, tty e fora de pipe vão direto ao kernel (`EAGAIN` e escrita
  parcial intactos). Sem `O_NONBLOCK` temporário, para a descrição compartilhada não mudar de modo. Usado por
  `os.write`, `FileIO.write` (logo `BufferedWriter.flush`), `os.writev` e o `_transfer` do `sendfile`/`copy_file_range`;
  `os.readv`, `eventfd_read` e o `_transfer` leem por `_net.read`; `eventfd_write` espera `POLLOUT` antes.
- `fcntl.flock` bloqueante (`LOCK_SH`/`LOCK_EX` sem `LOCK_NB`) com alguém para rodar tenta com `LOCK_NB` e, no
  `EAGAIN`, `_net.wait_retry` cede o turno em fatias de 1 ms (serve também a um dono em outro processo). `lockf` fica
  como estava: a trava POSIX é por processo, as threads dele nunca conflitam, e `F_SETLKW` bloqueante só espera outro
  processo (repetir com `F_SETLK` perderia o `EDEADLK`). `os.pread`/`pwrite` não bloqueiam (pipe dá `ESPIPE`);
  `time.sleep` já cede (`_sleep_hooks`); `socket.send/sendall/recv/connect/accept` já passam por `_call`/`_send_call`.
- Casos novos em `threading.toml` (golden do oráculo): `threading-os-write-full-pipe-reader-thread`,
  `threading-fileio-write-and-flush-full-pipe-reader-thread`, `threading-os-writev-readv-full-pipe-threads`,
  `threading-os-write-nonblocking-pipe-full-eagain`, `threading-eventfd-read-write-between-threads`,
  `threading-flock-two-fds-blocking-between-threads`.
- `http-server-bg-client` (servidor `python3 -m http.server` num processo, `curl`/`wget`/`python3 -c` em outros): neste
  caso não há espera do servidor por filho, e a leitura do escalonador (`_gsched.wait`, `_idle`, `_net._poller`,
  `_socket._call`, `accept` do kernel) não achou thread que deixe de ser escalonada. Hipótese verificada por leitura do
  kernel e corrigida (nada compilado nem executado): o `connect` de loopback recebia ECONNREFUSED na hora quando a fila do
  `listen` enchia (`net.rs`, `queue.len() >= backlog`), e o `curl` 000 podia vir daí. No Linux 6.12 é diferente:
  - `sk_acceptq_is_full` usa `sk_ack_backlog > sk_max_ack_backlog`, então a fila aceita `backlog + 1` conexões completas
    (`listen(fd, 0)` aceita 1; antes o `backlog` era forçado a pelo menos 1). O `backlog` é limitado por
    `net.core.somaxconn` (4096, antes `clamp(1, 4096)`); `listen` de novo num socket que escuta só ajusta o `backlog`.
  - Com a fila cheia `tcp_conn_request` descarta o SYN (`LISTENOVERFLOWS`) sem RST e sem criar SYN_RECV: o cliente fica
    em SYN_SENT e retransmite com RTO inicial de 1 s que dobra (1, 3, 7, 15, 31 e 63 s; `tcp_syn_retries` = 6, ETIMEDOUT
    em 127 s). O `connect` bloqueante espera isso, o não bloqueante dá EINPROGRESS (depois EALREADY até acabar) e ganha
    `POLLOUT` na retransmissão que achar vaga (sair vaga com `accept` não antecipa o SYN). Se o ouvinte fechou, o SYN
    seguinte leva RST (ECONNREFUSED, com `ERR` no poll). Porta sem ouvinte continua recusando na hora.
  - Implementação: `Ports::connect` devolve `Connect::Dropped` em vez de erro; `Listener::connect` põe o socket em
    SYN_SENT (`SynState`) e uma thread do host (`syn_loop`, nos instantes de `syn_at`, com o relógio real do kernel
    como o `nanosleep`) retransmite, acorda quem espera (`try_syn`, `poll`) e guarda o desfecho. O erro final de um
    `connect` não bloqueante passa ao `SockMeta` em `Task::settle_syn` (poll, `connect`, `SO_ERROR`, `recv`).
  - `/proc/net/tcp`: o cliente aparece em SYN_SENT (`02`, `tx_queue` 1, timer 1 com o tempo até a próxima
    retransmissão, `retrnsmt` e RTO crescentes) e o ouvinte com `rx_queue` igual às conexões na fila de aceite (não há
    SYN_RECV: o SYN descartado não cria `request_sock`). `ss` não existe no sandbox.
  - Fora desta fatia: `recv`/`send` num socket em SYN_SENT (o Linux dá EAGAIN ou espera o `connect`; aqui seguem como em
    socket sem conexão) e o teto de 4096 do `somaxconn` não tem teste (exigiria 4098 sockets).
  - Testes: `crates/kernel/tests/tcp_backlog.rs` (fila cheia com backlog 0, 1 e 2; `connect` não bloqueante
    EINPROGRESS e POLLOUT na retransmissão; bloqueante aceito depois do `accept`; RTO que dobra; ouvinte que fecha;
    `/proc/net/tcp`). O curl 000 do `http-server-bg-client` pode ainda ter a causa do `sleep 1`; medir no oráculo.
- Casos novos em `threading.toml` (o golden vem do oráculo): `threading-server-thread-subprocess-run-stdout-only`,
  `threading-server-thread-python-client-subprocess`, `threading-os-read-pipe-and-waitid-with-live-thread`.
- Espera do stdin fora da nativa (nada compilado nem executado, só leitura): `stdin.rs` esperava por
  `_net.wait_readable` dentro da nativa (`rust_nest` 2, `can_switch()` falso), então a thread que escreve no pipe nunca
  rodava. Agora, num `Stall` com `threading` em uso, instrução repetível e `rust_nest == 1`, `drive` devolve
  `suspend(SuspendRequest::Wait(fd))`. O laço atende: na chamada (`Op::Call*` cujo callee `stdin::is_reader`, com cópia
  de callee, args e kwargs) abre `_net.wait_then_call` (espera o fd, marca `_sys._stdin_resume()` e repete a chamada; o
  valor dela é o da instrução); no `FOR_ITER` sobre `PyIter::Native` abre `_net.wait_stdin` com `Dunder::Signal` e
  repete a instrução no mesmo `pc` (o iterador segue na pilha). `RESUMED` faz a repetição ir direto ao `read` do kernel
  (sem ele a espera se repetiria) e o `input()` não reimprime o prompt. A repetição só é segura enquanto nada foi
  consumido: `drive` desliga `REPLAYABLE` no primeiro sucesso, e o que sobra (`readlines`, `list(sys.stdin)`) cai na
  espera aninhada antiga. `sys.stdin.read()` sem tamanho passou a usar `text_all` (uma operação só, antes eram várias
  `readline`). Aberto: `readlines()` e as consumidoras compostas continuam com o deadlock; confirmar os quatro casos
  `*-fed-by-thread-via-dup2`.
- `eventfd-id` (`os_extra.toml`, `os-xattr-memfd-eventfd-timerfd`): no Linux 6.12 é o `eventfd_ida` global (`ida_alloc`:
  menor número livre do sistema, `ida_free` ao fechar), e o valor depende de quantos eventfds o host já mantém (um
  Debian em contêiner de outra máquina dá outro número). Não é determinístico, então o caso normaliza (`eventfd-id: N`) e
  o golden foi editado à mão para essa linha. O kernel (`anon.rs`) deixou de usar contador fixo: aloca o menor id livre
  de um `Ida` (base 343 modela os do host, que nunca voltam) e devolve no `Drop` do `Eventfd`.
- `FuncObj::c_owner` (`object/mod.rs`): `C_EXTENSION_MODULES` virou tabela `(módulo daqui, módulo do CPython)` com os
  módulos Python embutidos que no CPython são C e sem `.py` no Debian (termios, fcntl, `_select`, time, grp, pwd,
  resource, gc, atexit, faulthandler, marshal, zlib, cmath, `_posixsubprocess`, `_imp`, `_string`, `_thread`, `_socket`);
  o `os` segue por `code_is_posix_builtin`. `__code__`, `__globals__`, `__defaults__`, `__kwdefaults__`,
  `__annotations__`, `__dict__` e `__closure__` de uma função de C caem no `AttributeError` (`vm.rs`, `load_attr`).
