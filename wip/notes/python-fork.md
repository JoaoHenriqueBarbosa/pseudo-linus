# os.fork no ul-python: levantamento e desenho (fatia 7)

Alvo: `os.fork()`, `os.forkpty()`, `pty.fork()`, `os.register_at_fork()` e o aviso do 3.12+ iguais ao
CPython 3.13 no Debian 13. `multiprocessing` com `fork`, `socketserver.ForkingMixIn`,
`http.server.CGIHTTPRequestHandler` e o `asyncio` (`on_fork` em `asyncio_events.py`) dependem disso.

## 1. O que existe hoje

### Kernel e sysabi
- `Sys::spawn_fn(attrs, name, body: ProcessFn)` (`crates/sysabi/src/sys.rs:288`, impl em
  `crates/kernel/src/sys.rs:2095`) é o fork do sandbox. `ProcessFn = Box<dyn FnOnce() -> i32 + Send>`
  (`sysabi/src/types.rs:725`). Ele chama `spawn::fork_state` (`kernel/src/spawn.rs:77`): copia cred, cwd, root,
  umask, argv, env, comm, exe, rlimits, nice, disposições de sinal (`SigState::forked`: nada pendente) e a
  tabela de fds (descrições compartilhadas, FD_CLOEXEC por fd). Depois `apply_attrs`, `insert_child` (pid novo,
  `ppid` = pai, `children` do pai), `inherit_tune` (sched, afinidade, personality) e `start_process`, que cria
  UMA thread do SO por processo (`task_main`) e marca `fork_noexec` (`PF_FORKNOEXEC`).
- `name` vazio mantém o `comm` do pai (é o que o fork de verdade faz; o shell passa `b"bash"`). O python deve
  passar vazio.
- `wait4`, `kill`, `take_caught_signals`, `spawn_thread`, `tiocsctty` (`sysabi/src/sys.rs:255`) e `/dev/ptmx`
  já existem. Nada no kernel precisa mudar para o `os.fork` básico.
- O shell faz fork assim (`crates/shell/src/exec.rs:~330`): clona o estado do interpretador na thread do
  pai (`self.subshell_clone()`), move o clone para dentro do closure `Box::new(move || child.run_...())` e
  passa para `spawn_fn`. Isso só é possível porque o estado do shell é `Send`. Nada de `Rc`.

### ul-python
- `os.fork`, `os.forkpty`, `os.register_at_fork` constam no `__all__` de `modules/py/posix.py` (linha 31 e 40)
  mas NÃO têm implementação (`grep` não acha `def fork` nem função nativa). `os.waitpid`/`wait`/`wait3`/`wait4`,
  `WIFEXITED`, `WEXITSTATUS`, `waitstatus_to_exitcode` já existem (`osnative.rs::wait`, `os.py:109..`); `wait`
  nativo devolve `(pid, código)` com código negativo para sinal e o `os.py` recompõe o status cru.
  `os.getpid`/`getppid` chamam o kernel, então valem no filho sem trabalho.
- O interpretador roda numa thread do SO própria (1 GiB de pilha, `lib.rs::run_main`), filha da thread do
  pseudo-processo, que instala o pseudo-processo nela (`sys::install`) e dá `join`. Os desvios do kernel
  (`ExitUnwind` do `_exit`, `execve`, morte por sinal) atravessam o `join` por `resume_unwind`.
- A `Vm` (`vm.rs:968`) é feita de `Rc<RefCell<..>>` por toda parte (globals, modules, frames, frames_stack,
  std_files, handled...). `Value` tem `Rc` em todo variant composto. **`Rc` não é `Send`, e o workspace
  tem `unsafe_code = "forbid"`** (`Cargo.toml:17`). Portanto a `Vm` do pai NÃO pode ser movida para a
  thread do filho, nem um clone dela. Esta é a restrição central do desenho.
- Threads Python são cooperativas e aninhadas na mesma thread do SO (`_thread.py`: "as threads rodam uma de
  cada vez, aninhadas"). Nenhuma thread do kernel por thread Python. O pid tem 1 tarefa.
- Estado global fora da `Vm` que o filho precisa ter próprio: `thread_local!` (SEEN, TEXT_ERROR*, SOURCES,
  CURRENT, RECURSION_LIMIT, frameobj, tracing, typeattrs, builtins_ext, globalsview) e três estáticos de
  processo: `SIGNALS_ARMED`, `SIGNAL_THREAD`, `ALARM_AT_NS` (`vm.rs:1024..1030`). Estes três são do processo
  Rust inteiro, logo compartilhados entre pseudo-processos; hoje só um python roda por vez, com fork deixa de
  valer (ver fatia F2).

## 2. Desenho

### 2.1 Imagem do heap (a única forma sem `unsafe`)
O filho nasce numa thread do SO nova; tudo que cruza para lá tem de ser `Send`. Logo o pai produz uma
**imagem** `Send` do estado da `Vm` e o filho reconstrói um grafo `Rc` novo dela:

1. `HeapImage`: arena `Vec<Node>` com índices (`u32`) no lugar dos ponteiros. `Node` espelha cada variant de
   `Value`/`Object` (listas, dicts com ordem, sets, instâncias com `__dict__`, classes com MRO e metaclasse,
   funções com defaults/closure/globals, módulos, bytes, bytearray, ints grandes, `Native::*`, geradores
   suspensos com o `Frame`, `Env`, `Cell`s de closure, exceções com traceback, iteradores com posição).
2. Percurso do pai: pilha de trabalho + `HashMap<*const (), u32>` (endereço do `Rc` para índice) garante
   que o compartilhamento e os ciclos sobrevivem (dois nomes para o mesmo objeto continuam sendo um
   objeto; `is`/`id` coerentes). Imutáveis internados (None, True, False, ints pequenos, `str` interna,
   `Rc<Code>`) viram um nó por endereço também; `Code` é serializado como as `Op`s (se `Op` é só dado `Copy`,
   vai direto; constantes com `Value` passam pelo mesmo percurso).
3. Reconstrução no filho em três passadas: (1) folhas imutáveis (`str`, `bytes`, `int` grande) e as cascas
   vazias dos mutáveis (`list`, `dict`, `set`); (2) tuplas e `frozenset`s, filhos antes dos pais (um
   `Rc<[Value]>` não tem casca: nasce pronto), por pilha explícita, o que basta porque todo ciclo passa por
   um mutável e entre imutáveis o grafo é acíclico; (3) o conteúdo dos mutáveis, agora que todo alvo existe.
   O percurso do pai reserva o índice na descoberta (não na conclusão), senão `t = ([], 5); t[0].append(t)`
   não terminaria. O `set` leva a tabela de posições inteira (`Set::export_table`/`import_table`: vazias,
   removidas, hashes), porque a ordem de iteração depende do histórico de inserções e remoções e reinserir
   mudaria o que `print(s)` mostra; o `dict` basta na ordem de inserção. O endereço do `Rc` só vale como
   chave enquanto o grafo está vivo, e está: o capturador roda inteiro na thread do pai, com as raízes em mãos.
   `complex` NÃO é variant de `Value` (é a classe de `_complex.py`, logo `Instance`): entra em H2.
4. `Weak` (weakref, `FrameObj.env`): imagem guarda o índice do alvo; alvo ausente na imagem vira morto.
5. O que NÃO é dado do heap e tem tratamento próprio: `Native::File` (buffer de leitura e escrita incluídos,
   copiados: o buffer não descarregado do stdout duplica nos dois processos, como no CPython com stdout em
   pipe), sockets e `ssl` (fd + estado do cliente TLS 1.3 copiados), `sqlite3` (conexão aberta: o filho
   reabre o banco pelo caminho; o CPython documenta isso como inseguro, então pode ser `ProgrammingError`
   igual ao real no uso cruzado), módulos de extensão em Rust (`markupsafe._speedups`: estado `Send` próprio),
   `threading.Lock`/`RLock` em Python puro (são objetos comuns, entram no percurso).
6. Os `thread_local!` da seção 1 são reinicializados no filho (cache `SEEN` vazio; `SOURCES` copiado porque
   alimenta traceback; `RECURSION_LIMIT` copiado; `TEXT_ERROR*` limpos). `CURRENT` recebe a `Vm` nova.

Uma tabela de `Value`s e `Native` que o percurso não conheça deve **falhar o teste de cobertura** (match
exaustivo sem `_ =>` no percurso), para um tipo novo no futuro não ser esquecido em silêncio. Teste de
regressão: `fork()` com um exemplar de cada variant de `Value`, filho imprime `repr` e muta, pai confere
que não mudou.

### 2.2 Retomada no ponto do fork
Com a pilha explícita, o estado de execução é DADO: `frames_stack` (quadros suspensos), o `Frame` em execução
(pilha de valores, blocos protegidos, `pc`), `depth`, `frames` (para `sys._getframe`), `cur_line`, `handled`,
`globals` atual. Mas o quadro em execução vive como `&mut Frame` local de `run_frames`, fora do alcance de uma
função nativa. Mecanismo:

- `os.fork` nativo NÃO chama o kernel. Devolve `Err(PyErr::Suspend(SuspendRequest::Fork))`, uma variante nova
  tratada só no caminho de erro (custo zero no caminho comum).
- Em `run_frames`, no ramo de erro do `Op::Call` (e de `CallKw`, `CallMethod`...: todo op que chama
  nativa), um helper `complete_suspend(frame, req) -> PyResult<Value>` recebe o `&mut Frame`, monta a
  imagem (`frames_stack` + `frame` + resto da `Vm`), roda os ganchos `before`, chama
  `sys::current().spawn_fn(ProcAttrs::default(), vec![], body)`, roda `after_in_parent` e devolve
  `Ok(Value::Int(pid))` ao op, que segue como se a nativa tivesse devolvido. Como o op já consumiu os
  argumentos, nada precisa ser refeito.
- `body` (o `ProcessFn`, `Send`) carrega só a `HeapImage` e o índice do quadro corrente. No filho ele faz o
  que `lib.rs::run_main` faz (thread do interpretador com pilha de 1 GiB, `sys::install`, ignorar
  SIGPIPE/SIGXFSZ NÃO: o filho herda as disposições do pai, que já as ignorou), reconstrói a `Vm`, roda
  `after_in_child` e entra em `run_frames` com o `Frame` reconstruído e `Value::Int(0)` empilhado onde o
  op empilharia o resultado. Na saída, repete o tratamento de `run_main` (`resume_unwind` dos desvios do
  kernel; status de saída normal do `Outcome`; traceback não capturado no stderr do filho com status 1,
  idêntico a um programa que termina por exceção). `os._exit` no filho vira `ExitUnwind` como hoje.
- Falha de `spawn_fn` (EAGAIN por limite de processos): a nativa levanta `BlockingIOError`/`OSError` com
  errno e mensagem do Linux (`[Errno 11] Resource temporarily unavailable`), como o CPython.

### 2.3 Recursão Rust ativa no momento do fork
A imagem só captura estado que está em dados. Qualquer chamada Rust ativa na pilha da thread do interpretador
(um `run_loop` aninhado) carrega estado que não é copiável. Caminhos que ainda recursam, e portanto podem
estar na pilha quando `os.fork` roda:

- `vm.rs` `run_frame`, usado por `call_function` e por toda chamada vinda de Rust:
  `self.call(...)` em ~25 sítios do `vm.rs`, 48 de `classes.rs` (dunders despachados por nativas:
  `__getattr__`, `__repr__`, `__eq__`, `__hash__`, `__len__`, `__iter__`...), `json.rs` (`default=`,
  hooks), `builtins.rs`/`builtins_ext.rs` (`sorted`/`sort` com `key=`, `map`, `filter`, `functools`),
  `typeattrs.rs`, `generic.rs`, `weakrefmod.rs` (callbacks), `re.rs` (`re.sub` com função), `textwrap.rs`.
- `generator.rs` (chamada de `run_loop`) por retomada de gerador, de corrotina e de `async`: o quadro do gerador é
  `Frame` próprio resolvido por recursão a cada `next`/`send`/`throw`.
- `import`: o corpo de um módulo importado roda em `run_frame` aninhado; `fork` no nível do módulo importado
  (raro) ou em código que roda durante o `import`.
- `deliver_signals` → `signal._dispatch` → tratador Python (`vm.rs:1724`): fork dentro de um tratador de
  sinal (comum em daemons: `SIGCHLD`/`SIGTERM`).
- Threads Python (`Thread.run` chamado aninhado por `threading.py`), `atexit`, `__del__`/finalizadores.
- `with`/`__enter__`/`__exit__`, `StoreAttr` e dunders unários: outro agente os está movendo para a pilha
  explícita; fica fora desta fatia, mas entra na lista de pendências de "pode estar na pilha".

Podem estar na pilha ao chamar `os.fork` (casos reais): (a) o fluxo comum `pid = os.fork()` em função ou
módulo `__main__`, chamadas Python simples encadeadas (`Process.start` → `Popen._launch` → `os.fork`):
**não recursam, OK**; (b) fork dentro de gerador (`yield` no meio de um laço que faz fork: `socketserver`
usa método comum, mas `contextlib.contextmanager` + `fork` dentro do `with` roda o corpo no quadro do
chamador, OK; fork DENTRO do corpo do gerador recursa); (c) tratador de sinal; (d) callback de `sort`/`map`;
(e) corpo de módulo em import; (f) thread cooperativa.

Contabilidade (P1, feita): `Vm::rust_nest: Rc<Cell<usize>>` (`Rc` porque a `Vm` é clonada e as cópias
enxergam o mesmo contador). Só `run_loop` o move, por uma guarda (`NestGuard`) que sobe na entrada e desce
no `Drop`, inclusive quando um desvio do kernel (`_exit`) desempilha a thread; `run_frame` e
`generator.rs` passam por `run_loop`, então não há outro ponto. Chamada Python simples não conta (o quadro
do chamado vira o em execução, sem recursar): `os.getpid()` direto do `__main__` e dentro de função comum
vê 1; dentro de callback despachado por nativa (`sorted(key=)`) vê 2. `complete_suspend` só aceita o fork quando `rust_nest == 1` (só o `run_loop` mais externo da thread
do interpretador). Com `> 1`, o estado que falta é a pilha Rust e a cópia fiel não existe: levanta
`RuntimeError` interno marcado como lacuna (bug de prioridade máxima por vazar a costura, ver CLAUDE.md), e
cada caminho acima ganha sua fatia para virar dado (seção 3, G1 a G5). Não há atalho aceitável: a emulação
por "reexecução" ou "thread dormindo" mostraria comportamento diferente do Linux.

### 2.4 Threads
- Threads Python cooperativas não são threads do kernel; o pid tem 1 tarefa, `spawn_thread` não é usado.
- Imagem do heap inclui `threading._active`, `_limbo`, locks: no filho `threading._after_fork` (já registrada
  por `threading.py` via `os.register_at_fork(after_in_child=_after_fork)`) marca a thread corrente como
  `MainThread` e as demais como paradas, exatamente o que o CPython faz. Os locks de módulos (`logging`,
  `random`, `import lock`) seguem pelos mesmos `register_at_fork` da stdlib.
- Threads do kernel eventuais (`ul-sqlite` com thread auxiliar, `host_tracked`) não são copiadas: o fork
  só leva a thread que chamou, como no Linux. Teste: pid com thread secundária do kernel, o filho tem
  `Threads: 1` em `/proc/self/status`.

### 2.5 DeprecationWarning do 3.12+
`os.fork` do CPython 3.13, em `warn_about_fork_with_threads`, conta as threads do processo (lê
`/proc/self/stat` campo 20 no Linux; se falhar, conta as de `threading._active`) e, se `> 1`, emite
`DeprecationWarning("This process (pid=%d) is multi-threaded, use of fork() may lead to deadlocks in the
child.")`. Atribuído ao chamador do `os.fork` (`stacklevel=1` visto de C = quadro Python que chamou), filtrado
pelos filtros padrão (DeprecationWarning só aparece em `__main__` e com `-W`/`PYTHONWARNINGS`). Implementação:
em `posix.fork` (Python, dentro do módulo de wrappers de `posix.py`), antes de pedir o `Suspend`, ler o número
de threads do pid via o mesmo arquivo `/proc/self/stat` que o kernel já expõe MAIS as `threading._active`
vivas não principais (cada thread Python viva é uma thread no CPython real, então `n = 1 + vivas`); se
`n > 1`, `warnings.warn(msg, DeprecationWarning, stacklevel=2)`. O texto exato e o `stacklevel` são conferidos
no oráculo (`python3 -W always::DeprecationWarning`). `forkpty` emite o mesmo aviso com `forkpty()` no lugar de `fork()`
(`use of forkpty() may lead to deadlocks in the child.`); o texto é fixado no oráculo na fatia P2.

### 2.6 register_at_fork
`os.register_at_fork(*, before=None, after_in_parent=None, after_in_child=None)`: sem nenhum argumento
`TypeError: At least one argument is required.`; argumento não chamável `TypeError: 'before' must be
callable, not int` (nome do kw); posicional `TypeError: register_at_fork() takes no positional arguments`.
Três listas na `Vm` (imagem as copia). `before` roda em ordem INVERSA de registro, `after_in_parent` e
`after_in_child` em ordem de registro. Exceção num gancho não aborta o fork: vai ao `sys.unraisablehook`
(`Exception ignored in: <function ...>` + traceback no stderr) e segue. Ordem do fork: `before` no pai →
`spawn_fn` → `after_in_parent` no pai / `after_in_child` no filho (no filho antes de qualquer código do
usuário). Ganchos também rodam em `forkpty`/`pty.fork`. Não rodam em `subprocess` (`_posixsubprocess` é
posix_spawn aqui) nem `os.system`.

### 2.7 forkpty / pty.fork
`os.forkpty()` → `(pid, master_fd)`: `openpty` (abre `/dev/ptmx`, `unlockpt`, abre o escravo) + fork como
acima; no filho: `setsid`, `tiocsctty(slave)`, `dup2(slave, 0/1/2)`, fecha master e slave extra, devolve
`(0, master_fd)`; no pai fecha o escravo e devolve `(pid, master)`. É `login_tty` da glibc. `pty.py` (Python
puro) usa `os.forkpty` quando existe e senão `os.openpty`+`os.fork`+`os.setsid`+`fcntl.ioctl(TIOCSCTTY)`;
conferir que o `pty.py` do Debian roda sem mudança. Mesmo aviso de threads.

### 2.8 waitpid e saída do filho
Já funcionam: o filho é um processo do kernel, `wait4` o enxerga, o status cru (`WIFEXITED`,
`WEXITSTATUS`, `WIFSIGNALED`) sai de `Exited`/`Signaled`. Falta só: (a) `os._exit(n)` no filho encerra o
processo sem rodar `atexit` nem descarregar buffers (já assim); (b) saída normal do filho roda `atexit`,
descarrega `sys.stdout`/`stderr` e finaliza (a mesma rotina de fim de `run_main`); (c) o código de saída do
`SystemExit` não capturado e o traceback no stderr do filho; (d) `SIGCHLD` chegando ao pai dispara o
tratador Python pelo `deliver_signals` normal.

## 3. Fatias de implementação (cada uma pequena)

Pré-requisitos (podem ir antes, independentes):
- P1 (feita). Contador `rust_nest` em `run_loop`, com teste (`lib.rs`, `rust_nest_tests`: 1 no `__main__` e
  em função comum, 2 dentro de callback de `sorted(key=)`, 0 ao fim). Sem comportamento novo.
- P2. Conferir no oráculo (Debian 13 em Docker) os textos: aviso de threads (fork e forkpty),
  `register_at_fork` (3 erros), saída do traceback de gancho. Fixar em casos `testbench` antes de codar.

Imagem do heap (sem fork ainda; testada por ida e volta `Value → HeapImage → Value` na mesma thread):
- H1 (feita, `crates/ul-python/src/heapimage.rs`). `HeapImage` + percurso/reconstrução de `None`, `bool`,
  `int`, `Big`, `float`, `range`, `str`, `bytes`, `bytearray`, `list`, `tuple`, `dict`, `set`, `frozenset`
  com identidade e ciclos (testes: `a = []; a.append(a)`, ciclo por tupla, 200 mil níveis, ordem do `set`).
  Os demais variants de `Value` casam explicitamente e devolvem `ImageError::Unsupported` até H2 a H4.
- H2 (feita, `crates/ul-python/src/heapimage.rs`, mais `ExtObject::image`/`ExtImage` em `object/mod.rs` e
  `classes::ext_from_image`). Cobre `Function` (`defaults`, `kwdefaults`, `attrs`, `closure`, globals do
  módulo), `Code`, escopos de closure (`Env`, as "células" deste interpretador, com `parent` e ordem de
  nomes), tabela de globais (`Rc<RefCell<VarMap>>`, um nó compartilhado por todas as funções do módulo),
  `Class` (bases, MRO derivado das bases, metaclasse, `dict`, `__slots__` por estar no `dict`,
  `__subclasses__`), `Instance` (`dict`, `__dict__` vivo, `payload` de subclasse de embutido, o que
  inclui `complex` de `_complex.py`), `Bound`, `BoundFn`, `Slice`, `NativeFn`, `Builtin`, `Module`,
  exceções (`args`, `__cause__`, `__context__`, `__suppress_context__`, atributos extras) e os `Ext` com
  `image()`: `staticmethod`, `classmethod`, `property` (com `__doc__` próprio), o `getter/setter/deleter`
  de subclasse de `property`, `object()` e a célula `__classcell__`. Três passadas: cascas sem
  dependência, nós com campo imutável em ordem de dependência (pilha explícita, teto contra ciclo
  imutável), conteúdo mutável. Decisão sobre o `Code`: vai por cópia dos dados, não por `Arc`, porque ele
  guarda `Value` nas constantes e `Rc<str>` nos nomes e por isso não é `Send`; o `Rc<Code>` compartilhado
  continua um só no filho e os nomes são internados. Testes de ida e volta por tipo no módulo, rodando
  fonte Python na `Vm`, capturando as globais e chamando as funções e métodos refeitos.
  Fica para depois (anotado no cabeçalho do módulo): `id()`/hash por identidade e a ordem de um `set` de
  objetos sem `__hash__` dependem do endereço do `Rc`, que o grafo novo não preserva (tabela de
  endereços do filho, junto da H5); `__subclasses__` só refaz as subclasses que estão na imagem.
  `traceback` da exceção capturada, geradores e iteradores são H3; `Native`, `getset_descriptor` e os
  demais `Ext` são H4 (continuam `ImageError::Unsupported`, com o nome do tipo).
- H3. Iteradores, geradores suspensos (`Frame` completo), exceções com traceback, `weakref`.
- H4. `Native::*`: arquivos com buffer, sockets, `ssl`, `sqlite3`, extensões Rust, regex compilada, `re`,
  `array`, `memoryview`, `decimal`/`datetime` se forem nativos. Match exaustivo.
- H5. Resto da `Vm`: `modules`, `foreign_modules`, `module_globals`, `std_files`, `stdout` pendente,
  `handled`, `argv`, e reinício dos `thread_local!` e dos estáticos por-`Vm` (F2).

Fork (F1 a F3 implementadas; o que ficou para a compilação e a bancada está em "Estado" abaixo):
- F1 (feita, `crates/ul-python/src/fork.rs`). O `PyErr::Suspend` é a exceção de marca `fork::SUSPEND_KIND`
  (`PyException` é uma `struct`, não um `enum`) com o pedido numa variável da thread (`SuspendRequest::Echo`
  e `::Fork`). Só o ramo de erro do `Op::Call`/`CallMethod`/`CallEx` de `run_frames` a reconhece e,
  depois do `match` da instrução (o empréstimo do quadro já acabou), chama `Vm::complete_suspend`; o `pc`
  já aponta a instrução seguinte e o resultado entra na pilha do quadro em execução.
- F2 (feita). `SIGNALS_ARMED`, `SIGNAL_THREAD` e `ALARM_AT_NS` viraram `SignalState` por thread do
  interpretador (`vm::signals_armed`, `arm_signals`, `alarm_at_ns`, `set_alarm_at_ns`). O `SIGNAL_THREAD`
  saiu: com um estado por thread ele não tem mais função. O filho herda o "armado" pela imagem e nasce sem
  alarme (o `fork(2)` zera os alarmes do filho).
- F3 (feita). `_os.fork` (nativa), `os.fork`/`os.forkpty` com os ganchos e o aviso, `_os._exit` de verdade
  (`ExitUnwind`, sem `atexit`, sem finalizadores e sem descarregar o stdout, como o `_exit(2)`; antes era
  um `SystemExit` que descarregava), `os.waitpid`/`os.wait` sobre o `_os.wait`. `Vm::run` marca o laço mais
  externo (`fork::MainGuard`); o filho refaz a `Vm` (`VmImage::restore_fork`), retoma por `Vm::run_resumed`
  com `0` empilhado e termina como o pai (`lib.rs::conclude_run` e `conclude`, com `Finish` guardando se
  é arquivo, `-c` ou `-m`).
- F4. `os.register_at_fork` com validação e ordem (feita em `os.py`, junto da F3); `threading._after_fork` e
  `random` reseed no filho dependem dos módulos do Debian registrarem os ganchos: conferir na bancada.
- F5. Aviso `DeprecationWarning` de threads: feito em `os._fork_with_hooks` (`threading.active_count() > 1`,
  `stacklevel=1`: os quadros de `os.py` são invisíveis ao `_getframe`); falta conferir o texto no oráculo (P2).
- F6. `os.forkpty` (feita: `openpty` + `fork` + `login_tty` no filho, `os.openpty`, `os.login_tty`); falta
  rodar `pty.fork`/`pty.spawn` do `pty.py` do Debian.
- F7. Integração: `multiprocessing` start method `fork` (`Process`, `Pool`, `Queue`), `socketserver`
  `ForkingTCPServer`, `http.server` CGI, `asyncio` com `on_fork`.
  - `multiprocessing` (feito, sem compilar): o módulo embutido que emulava `Process` com thread saiu do registro
    (`py/multiprocessing.py` deve ser removido do repositório); roda o pacote do CPython que está em
    `/usr/lib/python3.13`, com `fork` de verdade. O que falta ao pacote é o `_multiprocessing`, agora em
    `py/_multiprocessing.py`: `SemLock` com o valor numa caixa de correio (um `pipe` de uma mensagem de 8 bytes,
    herdado pelo `fork`) no lugar do `sem_t` compartilhado. Desvio conhecido da fidelidade: cada lock ocupa dois
    fds visíveis em `/proc/self/fd` (no Debian o semáforo é uma página compartilhada, sem fd), e nome de semáforo
    (`spawn`, `forkserver`) não existe. Fecha de verdade quando o kernel ganhar semáforo (ou `futex` + memória
    compartilhada).
  - Causas achadas por leitura dos casos `fork-multiprocessing-*` e `fork-*threads*` (nada compilado nem
    executado; confirmar na bancada):
    - HANG de `Process` e `Pool`: `header + buf` em `Connection._send_bytes` soma `bytes` com `memoryview` (o
      `getbuffer()` do `ForkingPickler`). O `binary_native` de `vm.rs` só aceitava `bytes`/`bytearray` do lado
      direito e dava `TypeError`; o feeder do `Queue` (e o `put` do worker do `Pool`) morria sem escrever, e o
      `q.get()` do pai bloqueava para sempre. Agora qualquer objeto com buffer (`bytes_like`) concatena, com o
      tipo da esquerda (`bytes + mv` é `bytes`, `bytearray + mv` e `+=` ficam `bytearray`), como
      `bytes_concat`/`bytearray_concat`.
    - HANG do `Pool`, segunda causa: as threads verdes só cedem a vez em `_gsched.wait`/`sleep`. O
      `_select._await` esperava em fatias de `poll(2)` chamando `_wait_for(cond, 0)`, que não cede (timeout zero
      volta na hora), então a thread de `_handle_workers` (`selectors`, sem prazo) prendia o processo e a thread
      principal nunca rodava. Agora `_await` usa `_net.wait_fds` (novo, e `wait_fd` passou a usá-lo): a thread
      suspende e o `poll` do kernel vigia os fds quando ninguém mais pode rodar. `_gsched.wait` também tira a
      thread da fila se a espera levanta (tratador de sinal).
    - HANG do `Pool`, terceira causa: `_handle_results` lê o pipe com `Connection.recv`, ou seja, `os.read`
      bloqueante, que prendia as outras threads (inclusive a que despacha as tarefas). `os.read` agora espera o
      fd ficar legível pelo escalonador quando `_cooperative()` diz que há outra coisa a rodar, e
      `_cooperative()` passou a contar as threads verdes vivas (`_gsched.others()`), o que também torna o
      `waitpid` de uma thread cooperativo. Fica fora: `os.write` em pipe cheio e `SemLock._take` com a caixa
      de correio na mão de outro processo (janela curta, sem troca de thread entre `take` e `put`).
    - `fork-with-threads-warning`: parte do que o golden fixa é a ordem filho primeiro (a linha do filho antes do
      aviso do pai, as duas na mesma pipe). O aviso é trabalho pesado no pai (lê o fonte), então no Linux a
      primeira linha do filho chega antes; já os ganchos `after_in_parent` e o `write` do pai no mestre do
      `pty.fork` chegam ANTES da primeira linha do filho. O portão (`fork::Gate`, um `Arc` com `Mutex` e
      `Condvar`, sem fd; a primeira versão era um pipe e fechava o pai até o filho estar pronto) vale só para o
      aviso: `fork_process` deixa o portão pendente, o filho o solta depois de `restore_fork` (ou ao morrer, por
      `Release::drop`), e `os._fork_with_hooks` chama `_os._fork_settle()` apenas no ramo do aviso de threads,
      depois dos `after_in_parent`. A versão anterior (esperar sempre, antes de devolver o pid) deu a regressão
      de `fork-at-fork-hooks-order`, `fork-at-fork-hook-raises` (blocos de traceback do pai e do filho
      intercalados) e `fork-pty-fork-echo` (o filho vencia o eco do `abc`).
    - Causa comum de `fork-with-threads-warning`, `-catch` (HANG), `fork-threads-after-fork-state`,
      `fork-multiprocessing-process` (HANG), `-process-traceback` e `-pool` (HANG): o filho começava com
      `AttributeError: module '_thread' has no attribute '_slots'` em `threading._after_fork`, antes de
      `_gsched.after_fork()`. Os módulos embutidos filtrados por `builtin_dir` (como `_thread`) guardam as
      globais completas em `pysrc::PRIVATE` (um `thread_local!`), que o código embutido lê por `private_attr`; a
      thread do filho nascia com a tabela vazia. Agora a imagem as leva (`VmImage::private_names`, raízes
      depois das de `module_globals`, `pysrc::private_snapshot`/`private_install`). Sem o `_gsched.after_fork()`
      o filho herdava a fila e as threads vivas do pai: o `threading._shutdown` do `_bootstrap` esperava
      threads fantasmas para sempre.
    - `fork-threads-after-fork-state`: o stdout já devia bater; o golden guardava o `pid=154` do stderr, que
      depende do que rodou antes no contêiner. O caso agora normaliza (`pid=N`) por `sed` em arquivo e o
      golden foi ajustado igual.
    - `fork-multiprocessing-process-traceback`: a diferença vista era só o `Exception ignored` do `_after_fork`
      (mesma causa acima). O `threading._shutdown` antes das funções do `atexit` está certo: o `Py_FinalizeEx`
      chama `wait_for_thread_shutdown` antes de `_PyAtExit_Call`.
  - `pty.fork`: o `import pty` cai em `tty` e `termios`, e o sandbox não tem `termios` (só o `.so` em
    `lib-dynload`, que o importador trata como ausente). Falta portar `termios` (`tcgetattr`/`tcsetattr` sobre o
    `Syscalls::tcgetattr`, constantes, `error`).
- `KeyboardInterrupt` sem tratamento (no filho e no programa): `lib.rs::finish` devolve `UNHANDLED_INTERRUPT` e
  `die_by_sigint` restaura SIGINT e envia a si mesmo, como o `exit_sigint` do `Py_RunMain` (o shell vê 130).

### Estado depois de F1 a F3 (nada compilado nem executado ainda)
- Pendências do H5 fechadas em `heapimage.rs`: `FrameObj` (`ExtImage::Frame`, com `f_back` e `f_trace` na
  terceira passada e o registro por thread refeito), `CodeObject` e `CodeSource` (`ExtImage::CodeObject`,
  `::CodeSource`), `Code::cpy` (o `Emitted` do `cpybc`), tabela de `id()` (`HeapImage::ids` e
  `object::install_inherited_ids`: `py_addr`/`py_type_addr` devolvem no filho o id que o objeto tinha no pai,
  então `id()` e o hash por identidade, inclusive a ordem de um `set` de objetos, não mudam) e os
  rastreadores de `sys.settrace`/`sys.setprofile` (raízes da imagem).
- A imagem de fork leva o quadro do programa, os `Callee` de `frames_stack` e o em execução, com o `CallLink`
  (função, globais do chamador, `instance`, `on_stop`, `Dunder` de operador em curso). `CallLink`, `Callee`,
  `Dunder`, `Chain`, `Attempt`, `ChainKind` e `TruthUse` ficaram `pub(crate)`.
- Lacuna sem atalho: `rust_nest > 1` (callback de `sorted(key=)`, gerador, import, tratador de sinal) ou
  objeto sem imagem (`ImageError::Unsupported`) levanta `RuntimeError: fork() is not supported ...`. É
  vazamento da costura e some quando G1 a G5 fecharem; o teste `fork_inside_a_callback_is_a_gap_not_a_wrong_copy`
  (hoje com `list.sort(key=)`, já que o `key=` de `sorted` ganhou quadro) sai no mesmo commit da G4.
- Ainda não coberto: `Native::File` de arquivo aberto pelo programa (cópia do buffer e da posição entra pela
  H4, fd pela tabela herdada), sockets/ssl/sqlite (H4), `Threads: 1` em `/proc/self/status` no filho.
- Testes novos em `stdlib_tests.rs` (rodam no testkit, que executa o filho até o fim dentro do `spawn_fn`):
  estado independente, retomada com buffer duplicado e `atexit`, status e traceback do filho, fork em método
  e laço, ids e hash preservados, ordem do `register_at_fork` e gancho que levanta, erros de argumento,
  aviso de threads, lacuna em callback, `forkpty`. O `ul-python` ganhou `sysabi` com `testkit` em
  `[dev-dependencies]`.

Eliminar recursão (G, na ordem de frequência de uso; cada uma tira um caso de `rust_nest > 1`):
- G1 (feita, sem compilar). Tratador de sinal e `atexit` deixam de recursar.
  - Tratador: o laço consulta os sinais capturados entre duas instruções (a cada 8192 voltas, ou na próxima, se
    uma nativa pediu por `vm::request_signal_check`) antes do evento `line`. `Vm::signal_frame` abre o quadro do
    `signal._dispatch` (`enter_callable`) com `CallLink::then = Dunder::Signal` e o laço o empilha como um `spawn`
    qualquer: o quadro em execução espera em `frames_stack` com o `pc` na instrução interrompida, que roda quando
    o tratador volta (`Ok(Some(*pc))` pula a execução da volta em que o quadro nasceu). Valor devolvido é
    descartado; exceção do tratador (`KeyboardInterrupt` de `default_int_handler` incluída) vira `pending` no
    chamador SEM recuar o `pc` (nas outras chamadas o `pc` já tinha avançado), então sobe no ponto interrompido
    com o traceback do tratador. `Vm::pending_dispatch` é a parte comum com `deliver_signals` (que segue
    recursivo para as nativas que esperam). A imagem do heap leva `DunderNode::Signal`.
  - `os.kill`/`killpg`/`kill_many` para si mesmos não chamam mais `deliver_signals`: pedem a consulta imediata, e o
    tratador roda logo depois da chamada, no quadro do laço (o ponto de verificação do `CALL` do CPython).
  - `atexit`: `conclude_run` calcula o desfecho (`fork::Verdict`: código e stderr, o traceback sai antes do
    `atexit` como no CPython), publica-o em `RunTail::verdict` (`fork::enter_exit_phase`) e roda
    `Vm::run_exit_hooks` sob um `MainGuard` próprio (`rust_nest == 1`, a chamada de `_run_exitfuncs` é o laço mais
    externo da fase). Um `os.fork` dentro de uma função de `atexit` copia o estado; o filho retoma o quadro,
    e como `tail.verdict` está preenchido termina por `lib.rs::finish_exit` (`finalize_at_exit`, stdout, desfecho
    do pai) sem refazer `conclude_run`. Troca de thread dentro do `atexit` (daemon que acorda no `Event`) já
    funcionava com `rust_nest == 1`, o caso novo está em `nest_suspend.toml`.
  - Resta recursivo: tratador entregue por nativa que espera e retoma (`time.sleep`, `signal.pause`, `wait4`,
    `fcntl` com EINTR, PEP 475: o tratador roda e a espera recomeça; precisa de continuação, família G4);
    `signal.raise_signal` e `_thread.interrupt_main` chamam o tratador direto em Python (sem `frame`, sem passar
    pelo laço, mas já em quadro por serem chamada Python simples). Com thread secundária rodando, o tratador roda
    nela, e não na principal (o CPython o roda na principal): trocar de volta para a principal exige continuar
    a espera dela no escalonador Python, fica para a fatia de threads. O evento `line` do `settrace` é disparado
    uma vez só pela instrução interrompida (o tratador roda antes dele).
  - Casos: `nest-fork-in-signal-handler`, `nest-fork-signal-handler-in-loop-and-function`,
    `nest-signal-handler-exception-propagates`, `nest-thread-block-in-signal-handler`, `nest-fork-in-atexit` e
    `nest-thread-block-in-atexit` em `nest_suspend.toml` (goldens ainda do oráculo).
- G2 (feita, sem compilar). Geradores e correntes retomados pelo laço deixam de recursar. `GenCore::resume`
  virou `prepare` + `begin` + `end` (`generator.rs`); o laço usa as mesmas `begin`/`end` por `Vm::enter_resume`:
  o quadro do gerador sai do `GenCore` e vira um `Callee` sem `func` (`CallLink::func` agora é `Option`) com
  `CallLink::resuming` (`Resuming { tail, use_ }`). `Op::Yield` num filho `resuming` fecha o quadro como
  `Exit::Yield`; o `finish` chama `GenCore::end` (devolve o quadro ao gerador, desfaz `frames`/`handled`/globais)
  e `Vm::apply_resumed` aplica o desfecho no chamador conforme o `ResumeUse`: `ForIter` (`for` e compreensão
  inline: valor vira o item, `return` descarta o iterador e salta), `Next` (`next(g[, d])`, `g.send(v)`,
  `g.__next__()` por `call_or_enter`: valor ou `StopIteration(retorno)`/`d`) e `Delegate` (`yield from`/`await`:
  `Op::Delegate` sobre gerador/corrente no topo). A imagem do heap leva o `ResumingNode` (núcleo, `base`, linha,
  globais, uso). `throw`/`close` (feitas, sem compilar): a exceção injetada vai em `Resuming::inject`, o laço a toma
  ao empilhar o quadro (`pending`), e `ResumeUse::Close` passa o desfecho por `GenCore::settle_close` (yield vira
  `RuntimeError`, `GeneratorExit`/`StopIteration` viram `None`); `close_if_plain` decide o `close` que não
  retoma. Com isso o `__exit__` de `contextlib.contextmanager` com exceção (`gen.throw(value)`) não recursa.
  Consumidores nativos que esgotam antes de usar, como o CPython: `list(g)`, `tuple(g)`, `sorted(g, ...)` e
  `s.join(g)` por `ResumeUse::Collect` (o laço retoma o gerador em quadro até o fim, acumulando, e então chama
  a função com a lista).
  Consumidores por item (G4, fatia dos consumidores, feita, sem compilar): `sum`, `set`, `frozenset`, `dict`,
  `min`, `max`, `any`, `all` e `list.extend` sobre gerador, e `enumerate`, `zip`, `map` e `filter` sobre gerador
  (`for`, `next(it)`, `list(...)` e os consumidores acima sobre a cadeia). O mecanismo reaproveita o `Collect`:
  - `fold.rs`: `Fold` é a conta de cada consumidor como dado (`Sum` com fase `Int`/`Float` compensada/`Generic`,
    `Set`, `Dict`, `MinMax`, `Any`, `All`, `Extend`), com `feed` por item e `finish`. `Collect::fold` a guarda;
    com ela o item entra na conta na hora e `any`/`all` devolvem no primeiro que decide (o gerador fica parado,
    sem esgotar), `extend` deixa os itens parciais na lista se o gerador levanta, e `sum`/`set`/`dict`/`min`/`max`
    chamam `__add__`/`__hash__`/`key`/comparação na ordem do CPython. O caminho síncrono (iterável que não é
    gerador) passa pela mesma `Fold` (`fold::run`), então `b_sum`, `minmax`, `b_any`, `b_all` e o `dict(...)`
    (`builtins::dict_item`) têm uma só lógica. `fold::for_call` reconhece a chamada (`Builtin` ou `NativeFn` da
    tabela de `builtins`; as formas mal escritas caem no caminho comum, que dá o erro do CPython).
  - Cadeia preguiçosa: `generator::Pull { root, layers, then }` e `Layer` (`Enumerate`, `Filter`, `Many` para
    `map`/`zip`, com os itens já juntados e a fonte `at` em curso). `Vm::drive` é a máquina (`Want`/`Got`/
    `Ended`): desce pelas fontes (`lazy::source_ext`/`source_next`/`reaches_generator`), empilha o quadro do
    gerador com o `Pull` no `Resuming::use_`, e o item sobe pelas camadas (índice do `enumerate`, predicado do
    `filter`, função do `map`, tupla do `zip`, conferência do `zip(strict=True)`) até o consumidor `then`
    (`ForIter`, `Next` ou `Collect`). `Vm::deliver` entrega e, na coleta, pede o item seguinte à mesma fonte
    (`Vm::fetch`); `Next::Exit` é o fim do `for` (descarta o iterador e salta). A coleta de gerador sem
    camadas também passa por `Pull` (o `Collect` direto no quadro saiu junto com `collect_step`).
  - Imagem: `UseNode::Pull`, `LayerNode` e `FoldNode` em `heapimage.rs` (tudo `Value`, índice e número).
  - Corrigido de passagem: `list`, `tuple`, `sorted` e `next` são `NativeFn` da tabela (não `Builtin`), então os
    braços de `collect_source` e do `next` do G2 não casavam; `fold::builtin_name` aceita as duas formas.
  - Callbacks Python de consumidores nativos (G4, fatia dos callbacks, feita, sem compilar): a função do `map`, o
    predicado do `filter`, o `key=` de `min`/`max`/`sorted`, a conferência final do `zip(strict=True)` e os dunders
    de classe de usuário que um consumidor nativo chama (`__iter__`, `__next__`, `__getitem__` do protocolo antigo
    de sequência) deixam de recursar. O mecanismo é o mesmo do `Pull`: o callback ganha quadro
    (`Vm::enter_callable`, o trecho de `call_or_enter` que abre função, método ligado, classe com `__init__` e
    instância com `__call__` em Python) com `CallLink::then = Dunder::Callback`, que guarda o consumidor parado
    (`generator::Callback { pull, what }`) e o que fazer com o valor (`CallbackKind`: `Mapped`, `Kept`, `Keyed`,
    `Advance`, `Start`). Ao fechar o quadro, `finish_dunder` chama `Vm::resume_callback`, que devolve o valor à
    máquina (`drive` ou `collect_run`). Erro do callback chega ao chamador na instrução que abriu o passo, como o do
    quadro de gerador.
    - Fontes que precisam de quadro: `lazy::needs_frames` (gerador; fonte folha; cadeia com callback Python ou fonte
      dessas) substitui o `reaches_generator`. A fonte folha é o `IterBox` (iterador vivo de qualquer iterável, usado
      quando o `__next__` é de usuário ou o `key=` roda Python) e o `OldSeqIter` (protocolo antigo de sequência,
      agora preguiçoso como no CPython: `get_iter` não esgota mais o `__getitem__` na criação); `lazy::leaf` diz o
      método Python a chamar e `Callback::Advance` trata o fim (`StopIteration`, no antigo também `IndexError`).
    - `zip(strict=True)`: `Layer::Check` puxa as fontes seguintes depois que a primeira acaba (item a mais é o
      `ValueError` de "longer"), pelas mesmas camadas, então um gerador que faz `fork` ali tem quadro.
    - `Fold` ganhou `Sorted` (itens esgotados primeiro, depois cada chave em ordem, como o `list.sort` do CPython; o
      `sort_items` também passou a chamar as chaves na ordem natural antes de inverter) e `Flow::Key`: a conta pede a
      chave de um item, o laço abre o quadro e devolve por `Fold::keyed`. O caminho síncrono (`fold::run`) atende o
      mesmo `Flow::Key` chamando a função na hora.
    - Raiz da coleta: `frame_root` aceita gerador, cadeia, instância com `__iter__` em Python (`Root::Iterable`, o
      `__iter__` roda em quadro com `CallbackKind::Start`), instância do protocolo antigo e, com `key=` em Python,
      qualquer iterável. `dict(x)` fica de fora para instâncias (o mapeamento decide primeiro).
    - Imagem: `DunderNode::Callback`, `PullNode`, `LayerNode::Check`, `FoldNode::Sorted`, `LazyNode::Boxed` e
      `::OldSeq` em `heapimage.rs`.
  - Resta recursivo: o `key=` de `list.sort`, `functools.reduce`/`partial`, o `__iter__` que um construtor de
    `map`/`zip`/`enumerate` chama na criação, as comparações de `sorted`/`min`/`max` (`__lt__`/`__gt__` de classe de
    usuário, família dos dunders de comparação), `*g` de gerador assíncrono, `__anext__`/`asend` e `throw`/`close`
    com o gerador parado numa delegação (`yield from`/`await`, `GenCore::delegating`). Nada de compilar nem de
    golden ainda: os casos `nest-fork-generator-eager-consumers`, `nest-fork-generator-lazy-iterators`,
    `nest-thread-block-in-native-consumers`, `nest-fork-callback-map-filter`, `nest-fork-callback-key`,
    `nest-fork-zip-strict-final-check`, `nest-fork-user-class-dunders`, `nest-thread-block-in-callbacks` e
    `nest-thread-block-in-user-class-dunders` de `nest_suspend.toml` ainda precisam do oráculo.
  Casos: `testbench/corpus/cases/python/nest_suspend.toml`.
- G3 (feita, sem compilar). Corpo de módulo em `import`, `exec`, `eval` e `__import__` rodam como quadro do laço
  (`crates/ul-python/src/modrun.rs`), então `rust_nest == 1` dentro deles e `fork`/troca de thread funcionam.
  - Módulo: `userimport::exec_file` só prepara (`modrun::Body`: módulo já em `sys.modules` e em `module_globals`,
    globais vivas, código). `Op::Import`/`ImportRel` e `__import__` chamam `modules::begin_import`, que monta um
    `ImportPlan` (os prefixos `a`, `a.b`, `a.b.c` e o nome entregue no fim) e `Vm::continue_import` abre o quadro do
    primeiro corpo que falta (`Vm::open_body`: troca `Vm::globals`, `Dunder::Import(ImportRun)` com a guarda
    `Initializing`). Ao fechar (`close_import`): sucesso liga o módulo ao pai e segue a cadeia; exceção remove de
    `sys.modules` e `module_globals` e a exceção sobe no quadro do `import`. Módulo parcial em importação circular
    é o mesmo de antes (registrado antes de rodar). `import_checked` (recursivo, usado pelas nativas) passa pelo
    mesmo `load_one` + `open_body` + `Vm::run_module_callee`, então há uma só lógica.
  - O quadro de corpo acaba sem `Return`: `run_frames` empilha `None` e executa um `Return` sintético (`ended`).
  - `exec`/`eval`: `builtins_ext::enter_exec` (a `call_or_enter` o escolhe, salvo com perfil que pede `c_call`)
    prepara os espaços de nomes e `Vm::enter_nested` abre o quadro (entra em `frames`, `bind_globals`, `tracing`);
    `Dunder::Exec(ExecRun)` leva globais, `was`/`lwas`, `locals` separado, cópia de segurança das globais vivas e o
    mapeamento de `locals` que não é `dict`; `finish_exec` devolve o valor do `eval` e escreve de volta nos dicts.
    `run_nested` saiu (era a mesma lógica).
  - Imagem: `DunderNode::Import` e `DunderNode::Exec` em `heapimage.rs`; a restauração refaz a guarda
    `Initializing` e o `bind_globals` do quadro de `exec`.
  - `runpy`/`python -m`: o `-m` do programa já roda no laço mais externo; `runpy.run_module` cai em `exec` e
    `importlib.import_module` em `__import__`, ambos cobertos.
  - Resta recursivo: finders do programa em `sys.meta_path` (`load_with_finder`), `from pacote import submódulo`
    pelo `Op::ImportName`, `importlib.reload`, módulos embutidos em Python (`pysrc::import`, o corpo roda
    aninhado), o `__setitem__` do `locals` que não é `dict` no fim do `exec`, e `exec`/`import` chamados por uma
    nativa (`vm.call`). Casos: `nest-fork-in-imported-module`, `nest-fork-in-package-chain`,
    `nest-fork-circular-import-partial-module`, `nest-fork-failing-import-removes-module`,
    `nest-thread-block-in-imported-module`, `nest-fork-in-exec-and-eval`, `nest-fork-in-exec-with-mapping-locals`,
    `nest-thread-block-in-exec-and-eval` e `nest-fork-in-compiled-code-and-dunder-import` em `nest_suspend.toml`
    (goldens do oráculo).
- G4. Callbacks de nativas (`sorted`/`list.sort(key)`, `map`, `filter`, `functools.reduce`,
  `partial`, dunders de `classes.rs`) por continuações: a nativa devolve "chame f e continue assim" e o
  laço executa. Maior fatia, dividir por família (sort/min/max, map/filter/reduce, dunders de
  contêiner, dunders de objeto).
- G5. Threads cooperativas (`threading`) rodando `run` como quadro da pilha explícita, e
  finalizadores/`atexit`.
- Finalização de geradores (feita, sem compilar; vazamento da regra suprema: o `finally` de um gerador
  descartado não rodava). `Drop for GenCore` (`generator.rs`) move o quadro suspenso (com bloco protegido,
  exceção tratada ou delegação pendente) para um `GenCore` ressuscitado e o põe na fila de `finalize.rs`
  (`Doomed::Generator`, ao lado das instâncias com `__del__`); `run_finalizers` o fecha com `GenCore::reap`
  (o `close()` do CPython no descarte, erro vai ao `sys.unraisablehook`) no ponto seguro entre instruções,
  que cobre saída de quadro, reatribuição, `del` e `for ... break`. O registro de vivos virou `Live`
  (instância ou gerador, em ordem de criação) e `finalize_at_exit` fecha os que restaram. Fica de fora o
  gerador assíncrono (depende do gancho do `asyncio`). `reap` roda num `run_loop` aninhado (como o `__del__`),
  então entra na lista de recursão da seção 2.3 junto com os finalizadores. Casos:
  `testbench/corpus/cases/python/generator_finalize.toml` (golden do oráculo, ainda não conferido).
Com G1 a G5 fechadas, o `RuntimeError` da seção 2.3 deixa de ser alcançável e sai (o teste que o
cobre é removido no mesmo commit).

Ordem sugerida: P1, P2, H1..H5 (com os testes de ida e volta), F1, F2, F3, F4, F5, F6, F7, G1..G5.
