# E01: modelos de execução de pseudo-processos

Experimento que responde H01 a H10 do `hypotheses.toml`: qual modelo de execução o kernel-biblioteca
usa pros pseudo-processos, dado que todo o nosso código é `forbid(unsafe_code)`.

Rodar (grava `testbench/results/e01-exec-models.json`; cerca de 50 s numa máquina quieta, mais o
`cargo check` das sondas do H01 na primeira vez):

```sh
cd testbench/experiments/e01-exec-models
cargo run --release                 # tudo, grava o JSON
cargo run --release -- quick        # tudo com tamanhos reduzidos, não grava
cargo run --release -- one h05      # uma hipótese só, imprime a evidência
cargo test --release                # semântica dos três modelos + doctests compile_fail do H01
```

## Hipóteses

| Id | Frase | Critério (resumo) |
|---|---|---|
| H01 | Corrotinas stackful permitem M:N com migração entre workers em Rust seguro | refutada se toda crate exige `unsafe impl Send` ou API unsafe (provado por compile_fail) |
| H02 | Troca de corrotina é muito mais barata que handoff de thread, e isso pesa no throughput de pipelines | confirmada se A perde mais de 20% de throughput de pipeline pra B ou C; parcial se a troca é muito mais cara mas o pipeline não sente |
| H03 | Pilha virtual de 256 KiB só ocupa memória física quando tocada; milhares de processos custam pouco | RSS por processo ocioso abaixo de 64 KiB em 1k, 10k e 30k |
| H04 | Criar pseudo-processo custa microssegundos e cabem dezenas de milhares | spawn+exit abaixo de 50 µs e 30k simultâneos |
| H05 | Checkpoint com AtomicBool custa quase nada e dá preempção fina | overhead abaixo de 2% e p99 do timer até ceder abaixo de 100 µs |
| H06 | SIGKILL via unwind libera fds e roda Drops, bloqueado ou em laço com checkpoint | morre, Drops rodam, tempo medido |
| H07 | Laço sem checkpoint não é preemptado nem morto; rebaixar a thread host mitiga | demonstra, e mede o vizinho com e sem nice 19 no A |
| H08 | Stack overflow derruba o host; stacker mitiga | em subprocesso, por modelo |
| H09 | Panic num builtin fica isolado e não envenena o kernel | dentro e fora de lock, std::Mutex e parking_lot |
| H10 | Thread-local de processo corrente viabiliza o shim e a contabilidade | A natural; B mostra o que quebra; C task-local |

## Método

### O mini-kernel

O mesmo kernel mínimo existe nos três modelos, e só o mecanismo de execução muda:

- `kernel.rs`: tabela de processos, sinais pendentes, `wait`, tabela de descritores, término (fecha fds,
  publica status, acorda quem espera), timer de fatia e a sonda de latência.
- `pipe.rs`: pipe com buffer circular de 64 KiB (`VecDeque`), operações no estilo `poll` que registram o
  `Waker` do processo. Leitura vazia sem escritores dá EOF; escrita sem leitores vira SIGPIPE.
- Sinais: `kill` marca o sinal pendente, liga `attention` e acorda o processo. A entrega acontece na
  entrada de cada chamada, na volta de cada bloqueio e no `checkpoint()`, por unwind com payload próprio
  (`resume_unwind(KillUnwind)`, que não passa pelo panic hook), capturado por `catch_unwind` na entrada
  do processo. `exit(código)` usa o mesmo caminho com outro payload.
- `checkpoint()`: uma leitura `Relaxed` de `attention` (um `AtomicBool` por processo). Uma thread de timer
  liga o flag do processo que está rodando em cada CPU virtual a cada 1 ms. O caminho lento apaga o
  flag, entrega sinal e cede a CPU virtual se houver outro processo pronto.
- N CPUs virtuais configuráveis em todos os modelos. A fila é FIFO simples: o EEVDF de verdade é do E02;
  aqui interessa o custo do mecanismo.

Os três modelos:

- **A** (`model_a.rs`): uma thread do SO por processo (`std::thread::Builder` com pilha de 256 KiB ou
  64 KiB) e N tokens de CPU virtual. Ceder ou bloquear devolve o token direto pro próximo da fila,
  `unpark` nele e `park` em si. Variante **A-spin**: 20 µs de espera ativa antes do `park`, pra que um
  handoff rápido não passe por futex. Opcional: watchdog no timer que rebaixa pra nice 19 a thread de um
  processo que ignora `attention` por mais de 3 ms e consumiu mais de 2 ms de CPU nesse tempo (lido de
  `/proc/self/task/<tid>/schedstat`).
- **B** (`model_b.rs`): N workers, cada um com suas corrotinas corosensei (pilha `DefaultStack` de 64 KiB
  ou 256 KiB), fila local e caixa de entrada pra wakes vindos de outras threads; wake de dentro do
  próprio worker vai direto pra fila local. Sem migração: o processo fica no worker escolhido no spawn
  (round-robin).
- **C** (`model_c.rs`): processo = `Future`, executor próprio sem tokio (fila global com contagem de
  workers dormindo, slot LIFO por worker com limite de 16 seguidas). Kill: o executor vê o sinal antes
  do poll e descarta o future. Panic e término dentro do poll: `catch_unwind` em volta do poll.

A API de processo é o trait `Sys` (`sys.rs`) pra A e B, e `CtxC` com as mesmas chamadas em `async` pro C.
Os programas de teste (`workloads.rs`, `vm.rs`) existem nas duas formas.

### Medições

- **H01**: o binário roda `cargo check` em cada sonda de `probes/h01` (crate com `forbid(unsafe_code)`)
  e registra se compilou e os códigos de erro exatos; depois roda a sonda do `generator`, que compila, e
  mede se a corrotina migrada continua usando o thread-local da thread de origem. Os mesmos casos estão
  como doctests `compile_fail` em `src/lib.rs` (o rustdoc estável não confere o código de erro anotado,
  só a falha; por isso o teste `h01_probes_fail_with_expected_codes` roda as sondas).
- **H02**: ping-pong por `yield_now` com 1 CPU virtual (ns por troca efetiva, contada pelo kernel);
  ping-pong de 1 byte por pipe com 1 e 2 CPUs; linhas de base cruas (`resume`+`suspend` do corosensei,
  handoff park/unpark entre duas threads); pipelines `yes | head -n 64Mi` (128 MB), 4 estágios em bloco
  de 16 KiB (gerador, `tr a-z A-Z`, `cat`, `wc`; 128 MB) e 4 estágios em registros de 64 bytes (4 MB),
  com 1 e 4 CPUs virtuais, timer ligado, 3 repetições (mediana), CPU do host gasta por run
  (`CLOCK_PROCESS_CPUTIME_ID`). Toda saída é conferida (bytes, status de cada estágio, SIGPIPE no `yes`).
- **H03/H04**: subprocesso `scale <modelo> <n>` dentro de
  `systemd-run --user --scope -p TasksMax=40000 -p MemoryMax=8G`, com 1k, 10k e 30k processos
  bloqueados lendo o mesmo pipe vazio, 4 CPUs virtuais. Memória por processo = diferença de VmRSS e do
  `memory.current` do cgroup do scope (que inclui pilha de kernel e tabelas de página) dividida por N.
  Latência de spawn+exit+wait cronometrada dentro de um processo "shell" (2 mil iterações no A, 20 mil
  nos outros).
- **H05**: interpretador de bytecode de pilha (`vm.rs`, 16 instruções por iteração) sozinho em 1 CPU
  virtual com timer de 1 ms, sem checkpoint, com checkpoint antes de cada instrução, e só nos saltos pra
  trás tomados; dentro de cada modelo e fora do kernel. 40 rodadas por ambiente; em cada rodada os três
  modos rodam colados (6,4 milhões de instruções cada, ordem girando), cronometrados pelo tempo de CPU
  da thread (`CLOCK_THREAD_CPUTIME_ID`); a desaceleração é a mediana das razões pareadas dentro da
  rodada, o que cancela a interferência de outros processos. Decide a variante "mesmo código"
  (`run_dyn`: o modo é lido em tempo de execução e os três modos rodam o mesmo código de máquina); a
  variante "especializada" (uma versão compilada por modo via `const`) vai como evidência de quanto o
  layout de código sozinho mexe no resultado. No C, cada rodada também roda a versão síncrona do
  interpretador dentro do future, na mesma thread, pra medir o custo de o laço ser async. Latência:
  dois interpretadores disputando 1 CPU virtual, a sonda do kernel registra o tempo do timer ligar
  `attention` até o processo entrar no caminho lento (preempção) e até o outro processo estar rodando
  (handoff); 3 execuções por modelo, fica a de menor p99.
- **H06**: 100 kills por cenário e modelo: processo bloqueado em `read` de pipe e processo em laço com
  checkpoint, cada um segurando um guarda com `Drop` e o fd de leitura. Mede do `kill` ao `wait` colher;
  confere sinal 9, contador de Drops, pipe sem leitores e EPIPE pro escritor.
- **H07**: 1 CPU virtual, L gira 300 ms sem checkpoint, N pronto na mesma CPU; kill em L aos 50 ms.
  Mitigação no A: 2 CPUs virtuais, threads de L e N fixadas no mesmo núcleo do host
  (`sched_setaffinity`); com L em nice 0 e com o watchdog ligado, mede a fração da CPU que N recebe do
  total que N e L dividem (schedstat das duas threads, que não depende de quanto o resto do host usa o
  núcleo) e a taxa de iterações de N contra N sozinho; tenta voltar L pra nice 0 quando ele finalmente
  passa num checkpoint.
- **H08**: subprocesso `overflow <variante>` (sem core dump): recursão sem limite, recursão com
  `stacker::maybe_grow(32 KiB, 1 MiB)` e limite de 100 mil quadros, e a variante intercalada (X suspende
  no meio da recursão, dentro de um segmento do stacker, enquanto Y recursa). O subprocesso escreve no
  stderr o que `stacker::remaining_stack()` dizia na entrada. Variantes com stacker rodam 5 vezes (o
  resultado dentro de corrotina depende de onde o mmap pôs cada pilha); as outras, 2.
- **H09**: objeto do "kernel" atrás de `std::sync::Mutex` e de `parking_lot::Mutex`; um processo entra em
  panic dentro do lock ou fora; 20 processos usam o objeto depois; um pipeline confere que o kernel
  continua atendendo.
- **H10**: 200 processos em 4 CPUs virtuais gravam o pid num thread-local e conferem depois de cada uma
  de 50 trocas; B ingênuo, B com o worker trocando o valor a cada `resume`, `RefCell` thread-local
  emprestado através de um `yield`, e task-local no C contando migrações entre workers.

O binário refaz tudo sozinho e grava o JSON; o veredito de cada hipótese é calculado pelo código a partir
dos números medidos.

## Candidatos

| Candidato | Papel | Encaixe |
|---|---|---|
| Modelo A (thread do SO por processo) | modelo de execução | ver Veredito |
| Modelo A-spin (A com 20 µs de espera ativa) | variante do A | ver Resultado |
| Modelo B (corosensei 0.3.4 preso ao worker, pilha 64 KiB e 256 KiB) | modelo de execução | não serve |
| Modelo C (future + executor próprio) | modelo de execução | ver Veredito |
| corosensei 0.3.4 | corrotina stackful | API segura, mas `Coroutine` é `!Send`: só presa ao worker |
| may 0.3.51 | M:N pronto | não serve: `coroutine::spawn` é `unsafe fn` |
| generator 0.8 | corrotina stackful | não serve: `unsafe impl Send` próprio e unsound |
| stacker 0.1.25 | crescimento de pilha | serve em thread do SO (A) e em worker (C); não em corrotina (B) |

## Resultado

Números da execução gravada em `results/e01-exec-models.json` (load do host no fim: 5,9; as execuções
anteriores, com outros experimentos compilando e load entre 20 e 1500, deram os mesmos vereditos com
caudas de latência muito piores). O runner refaz tudo numa máquina quieta.

### H01: migração

| Sonda (`probes/h01`, `forbid(unsafe_code)`) | Resultado |
|---|---|
| corosensei, corrotina usada na própria thread (controle) | compila |
| corosensei, mover `Coroutine` suspensa pra outra thread | E0277 (`*mut ()` não é `Send`) |
| corosensei, `Arc<Mutex<Coroutine>>` entre threads | E0277 |
| corosensei, embrulho com `unsafe impl Send` | lint `unsafe_code` |
| may, `coroutine::spawn` sem `unsafe` | E0133 |
| may, `coroutine::spawn` em bloco `unsafe` | lint `unsafe_code` |
| generator 0.8, migrar entre threads | compila e roda: na thread 2 a corrotina usa o thread-local da thread 1 |

No generator, até um acesso novo ao thread-local feito depois da migração, por uma função inlinada,
devolve o endereço da thread 1: o compilador trata o endereço de thread-local como constante dentro da
função, então a migração quebra mesmo código que não segura referência nenhuma.

### H02: troca e pipelines

| ns | A (park) | A-spin | B64 | B256 | C |
|---|---|---|---|---|---|
| troca por `yield`, 1 CPU | 2906 | 175 | 28 | 28 | 34 |
| ida e volta de 1 byte por pipe, 1 CPU | 5930 | 609 | 167 | 164 | 162 |
| ida e volta de 1 byte por pipe, 2 CPUs | 6026 | 561 | 6205 | 6249 | 210 |

Linhas de base cruas: `resume`+`suspend` do corosensei 0,9 ns por troca; handoff park/unpark entre duas
threads do SO 2757 ns.

| MB/s (CPUs virtuais) | A | A-spin | B64 | B256 | C | Linux real |
|---|---|---|---|---|---|---|
| `yes \| head`, 1 | 2355 | 2942 | 3289 | 3289 | 3229 | 1730 |
| `yes \| head`, 4 | 2973 | 3202 | 2994 | 2972 | 2878 | |
| 4 estágios em bloco, 1 | 1551 | 1645 | 2563 | 2581 | 2550 | 1449 |
| 4 estágios em bloco, 4 | 2894 | 3219 | 2851 | 2850 | 2606 | |
| 4 estágios, registros de 64 B, 1 | 353 | 344 | 541 | 551 | 419 | |
| 4 estágios, registros de 64 B, 4 | 286 | 173 | 292 | 286 | 386 | |

"Linux real" é o mesmo pipeline com coreutils (`yes | head -n`, `head -c | tr | cat | wc`). O A-spin
gasta CPU girando: com 1 CPU virtual o A ocupa 1 núcleo do host e o A-spin de 1,4 a 3 (`cores_used` no
JSON); com 4 CPUs virtuais, até 3,9 núcleos contra até 3,5 do A.

### H03 e H04: memória e criação

| Por processo, 30 mil bloqueados | A (256 KiB) | A64 | B64 | B256 | C |
|---|---|---|---|---|---|
| RSS | 10,0 KiB | 10,0 KiB | 4,5 KiB | 4,5 KiB | 0,5 KiB |
| cgroup (inclui kernel) | 34,2 KiB | 33,8 KiB | 5,3 KiB | 5,6 KiB | 0,5 KiB |
| pilha de kernel | 16 KiB | 16 KiB | 0 | 0 | 0 |
| criação no laço de spawn | 21 µs | 21 µs | 0,38 µs | 0,37 µs | 1,0 µs |

Os 30 mil processos simultâneos funcionaram nos cinco modelos (e em 1k e 10k). Spawn+exit+wait,
mediana (p99): A 20,6 µs (43,6), A-spin 16,1 µs (43,0), B64 7,7 µs (10,6), B256 7,9 µs (11,0), C 0,19 µs
(0,25).

### H05: checkpoint

Mesmo código de máquina nos três modos (mediana das razões pareadas em 40 rodadas):

| Desaceleração | fora do kernel | A | B64 | B256 | C |
|---|---|---|---|---|---|
| checkpoint em cada instrução | -1,1% | 0,2% | 0,0% | 0,0% | 3,1% |
| checkpoint no salto pra trás | 0,0% | 0,0% | 0,1% | 0,0% | 0,7% |

O interpretador custa cerca de 1,7 a 1,9 ns por instrução. Compilando uma versão por modo, o layout do
código sozinho mexeu até 19% no resultado (pra cima e pra baixo), mais que o checkpoint. O laço async
sem checkpoint custou 2,6% a mais que o mesmo laço síncrono na mesma thread nesta execução (16% a 38%
nas execuções com a máquina carregada).

| µs, p99 (p50) | A | A-spin | B64 | B256 | C |
|---|---|---|---|---|---|
| timer até o processo ceder | 0,5 (0,17) | 0,5 (0,18) | 0,3 (0,12) | 0,3 (0,11) | 0,4 (0,11) |
| timer até o próximo processo rodar | 25,5 (5,1) | 18,1 (5,1) | 1,0 (0,2) | 1,1 (0,2) | 1,3 (0,2) |

### H06 a H10

- **H06**: 800 de 800 kills (100 por cenário e modelo) terminaram com sinal 9, guarda com Drop rodando e
  fd fechado (pipe sem leitores, escrita do host dá EPIPE). Kill até o `wait` colher, p50: 8 a 14 µs em
  todos os modelos, bloqueado ou em laço; p99 até 53 µs.
- **H07**: nos quatro modelos o vizinho fez 0 iterações durante os 300 ms do laço sem checkpoint e o
  SIGKILL mandado aos 50 ms só matou no fim do laço (250 ms depois). No A, com as duas threads no mesmo
  núcleo, o vizinho recebe 50% da CPU que os dois dividem; com o watchdog (detecção em 8,7 ms) recebe
  99%, e a taxa dele vai de 50% pra 98% da taxa sozinho. A volta pra nice 0 falha com EACCES
  (RLIMIT_NICE = 0, sem CAP_SYS_NICE).
- **H08**: recursão sem limite derruba o host em todos os modelos: SIGABRT no A e no C (o handler do std
  reconhece a guard page da thread e aborta), SIGSEGV no B (guard page da corrotina, que o std não
  conhece). Com stacker e limite de profundidade o processo sai com erro e o host sobrevive em 5 de 5
  execuções no A, no C, e no A com dois processos intercalados. No B o stacker usa os limites da thread
  do worker: na corrotina de 64 KiB ele vê 4,4 MB livres e nunca cresce (5 de 5 derrubaram o host); na de
  256 KiB vê 0 e cresce na primeira chamada (sobreviveu 5 de 5); com duas corrotinas intercaladas no
  mesmo worker, a segunda herda o `STACK_LIMIT` do segmento da primeira e derrubou o host em 3 de 5.
- **H09**: nos quatro modelos o panic (dentro e fora de lock) vira término anormal pro pai e o kernel
  continua atendendo (16 de 16 pipelines de verificação). Com `parking_lot`, com `std::Mutex` tratando o
  poison e com panic fora de lock, 240 de 240 processos seguintes terminaram normalmente; com
  `lock().unwrap()` num `std::Mutex` envenenado, 0 de 80.
- **H10**: no A, 0 erros em 10 mil conferências de thread-local. No B ingênuo, 9800 de 10 mil viram o pid
  de outra corrotina; com o worker trocando o valor a cada `resume`, 0 erros e custo nulo (28,8 ns por
  yield com a troca, 28,7 ns sem); mas um `RefCell` thread-local emprestado através de um `yield` deu 10
  conflitos de 10 no B (0 no A), e thread-locals de terceiros não são trocados (o do stacker, H08). No C
  o task-local mantido pelo executor deu 0 erros com 7416 migrações entre workers.

## Veredito

| Id | Veredito | Número que decidiu |
|---|---|---|
| H01 | Refutada | corosensei E0277, may E0133, generator compila mas usa o thread-local da thread 1 rodando na 2 |
| H02 | Confirmada | pior pipeline em bloco: melhor variante do A com 64% do throughput do melhor de B/C (4 estágios, 1 CPU); troca 100x mais cara no A (2,9 µs contra 28 ns), 6x no A-spin |
| H03 | Confirmada | RSS por processo ocioso de 10 KiB (A), 4,5 KiB (B), 0,5 KiB (C) com 30 mil; no A o custo real com kernel é 34 KiB |
| H04 | Confirmada | spawn+exit mediano de 20,6 µs no A, 7,7 µs no B, 0,19 µs no C; 30 mil simultâneos em todos |
| H05 | Confirmada | checkpoint no salto pra trás abaixo de 0,1% (síncrono) e 0,7% (C); p99 do timer até ceder de no máximo 0,5 µs |
| H06 | Confirmada | 800 de 800 kills com Drops e fd fechado, p50 de 8 a 14 µs |
| H07 | Confirmada | vizinho com 0 iterações durante o laço, kill atrasado 250 ms; no A o watchdog leva o vizinho de 50% pra 99% da CPU dividida (rebaixamento de mão única) |
| H08 | Parcial | overflow derruba o host nos quatro modelos; stacker salva no A e no C (5 de 5), falha no B (5 de 5 com pilha de 64 KiB, 3 de 5 intercalado) |
| H09 | Confirmada | 240 de 240 processos seguem depois do panic; só `lock().unwrap()` em `std::Mutex` envenena (0 de 80) |
| H10 | Parcial | natural no A (0 erros), quebrado no B ingênuo (9800 de 10 mil), task-local no C (0 erros com 7416 migrações) |

### Recomendação: modelo A

A regra, aplicada pelo binário aos números medidos (`metrics.recommendation` no JSON):

1. B sai se tiver falha de segurança medida. Saiu: stacker dentro de corrotina derrubou o host (H08) e
   o thread-local de processo fica errado sem troca manual, com `RefCell` e thread-locals de terceiros
   quebrando mesmo com a troca (H10). O B também não migra (H01), então não tem nem a vantagem do M:N.
2. Entre A e C, A fica se sustentar 30 mil processos (sustenta), se o pior pipeline em bloco dele ficar
   em pelo menos 25% do melhor modelo (64%) e se o watchdog de nice 19 der ao vizinho de um laço sem
   checkpoint mais de 90% da CPU que os dois dividem (99%). Passou nos três.

O custo do A é real e está medido: a troca é duas ordens de grandeza mais cara (2,9 µs contra 28 a 34 ns)
e isso tira até 40% do throughput de pipeline com 1 CPU virtual. Mas o mesmo pipeline com processos de
verdade do Linux fica no mesmo patamar ou abaixo (1449 a 1730 MB/s contra 1551 a 2355 MB/s do A): o A
paga o preço de um handoff de thread que o próprio Linux paga num pipe. Com 4 CPUs virtuais o A empata
com B e C (2894 contra 2851 e 2606 MB/s). A espera ativa (A-spin) baixa a troca pra 175 ns ao custo de
até 3,9 núcleos do host girando; é uma opção de ajuste, não o padrão.

O que o C não entrega e o A entrega, medido aqui: código síncrono de terceiros roda sem adaptação
(no C todo builtin vira `async fn` e cada crate síncrona bloqueia o worker); laço sem checkpoint tem
mitigação por thread (H07; no C a thread é compartilhada e rebaixá-la penaliza todo mundo); stacker
funciona (H08); thread-local é por processo sem nada especial (H10). O que o C ganha: memória por
processo (0,5 KiB contra 34 KiB com kernel), spawn 100x mais barato e troca 85x mais barata, que só
pesam em cargas com dezenas de milhares de processos ou com troca a cada poucos bytes.

## Notas

- `forbid(unsafe_code)` em todo crate (experimento, testes e sondas); nenhuma `unsafe impl`. O unsafe
  que existe está dentro de corosensei, stacker, parking_lot, rustix e std.
- O watchdog do A precisa ler `/proc/self/task/<tid>/schedstat` só dos processos suspeitos (ignoraram
  `attention` por mais de 3 ms), então o custo no timer é desprezível no caso comum.
- O rebaixamento pra nice 19 é de mão única sem `CAP_SYS_NICE` (EACCES medido): um processo rebaixado
  fica rebaixado até terminar, porque a thread dele morre junto.
- Nada faltou no `harness`: o experimento só usa `ExperimentResult`.
- Os doctests `compile_fail` em `src/lib.rs` só garantem que a compilação falha; o rustdoc estável não
  confere o código de erro anotado (verificado num crate de rascunho), por isso o teste
  `h01_probes_fail_with_expected_codes` roda as sondas e confere E0277, E0133 e `unsafe_code`.
