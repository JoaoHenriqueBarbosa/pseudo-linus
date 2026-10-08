# Desempenho do ul-python (fatia 1)

Medida de partida (release): laço simples 6 a 12 vezes mais lento que o CPython 3.13. O perfil de 3 milhões de
chamadas `f(i)` apontou `run_frames` 31%, `globalsview::sync_pull` 16%, `open_call`/`end_call` 6%, alocador
5%, e uma cauda de buscas em mapa, `Value::clone` e `drop`.

## 1. `globalsview::sync_pull` (ganho esperado: quase todos os 16%)

Causa: `sync_pull` roda antes de CADA instrução enquanto existir alguma visão (`ARMED`), não por chamada. Ele
entrava no `thread_local` `VIEWS`, tentava `try_borrow_mut` e, para cada visão, `try_borrow` do dict e comparava
`generation`. Qualquer programa que tenha tocado em `module.__dict__`, `globals()` ou `f_globals` (o `__main__`
e a stdlib embutida fazem isso na partida) pagava isso em todo opcode.

Desenho: o contador global de mutações de dict (`object::dict::GENERATION`) já é a fonte das `generation`.
`sync_pull` agora lê esse contador (uma leitura atômica relaxada) e compara com `SEEN`, o valor da última varredura
completa. Igual quer dizer que nenhum dict do processo mudou desde então, logo nenhuma visão mudou: retorna sem
tocar em `VIEWS` nem em `RefCell`. Só uma mutação de dict de verdade (que bumpa o contador) paga uma varredura, e
ela atualiza `SEEN`. Varredura incompleta (um `try_borrow` falhou) não atualiza `SEEN`, então tenta de novo.
Semântica preservada: a varredura em si é a mesma; só deixa de rodar quando é garantidamente um no-op.

Invariante nova: toda mutação de `Dict` tem de bumpar o contador. `set`, `remove` e `from_hashed` já bumpavam;
`dict.clear()` trocava o dict por `Dict::default()` (generation 0, sem bump), o que antes só era percebido por a
generation ser diferente. Agora `Dict::clear` bumpa. Quem for criar outro caminho de mutação em `Dict` precisa
passar pelo contador.

Custo residual: cada gravação de global com visão armada (`push`) bumpa o contador, então a instrução seguinte faz
uma varredura (poucas visões, compara generation). Se isso aparecer num laço de nível de módulo, o próximo passo é
`push` atualizar `SEEN` quando ele era igual ao contador antes do seu próprio `set`.

## 2. Despacho de chamada (ganho esperado: 2 a 4%)

`call_or_enter` testava, para toda chamada, `reports_*`, `collect_source` (`for_call`, `builtin_name`) e cinco
comparações de nativas (`exec`, `eval`, `__import__`, `next` duas vezes, `Bound`) antes de chegar a
`enter_callable`. Nenhum desses casos reconhece `Value::Function` nem `Value::BoundFn`, e sem perfil
(`profiling()` falso) os dois `reports_*` são falsos. Atalho exato no topo: função Python ou método ligado, sem
perfil, vai direto a `enter_callable`. Com perfil ligado segue o caminho antigo.

## 3. `tracing::call_event` e `fold::for_call` (ganho esperado: cerca de 1%)

`call_event` agora sai na primeira linha com `!hooked()` (duas leituras de `Cell` no thread local), antes de
`active()`, do `clone` do rastreador e de `profiling()`. Equivalente: sem gancho nenhum os dois eram falsos e a
função já devolvia `Ok`. O `fold::for_call` deixou de ser chamado no caminho de função Python pelo atalho do item
2; para nativas continua igual.

## 4. `tracking_allocator` (vendor/tracking-allocator, ganho esperado: 2 a 4%)

Nenhum código chama `enable_tracking`/`set_global_tracker` ainda (a contabilidade de memória por processo é desenho
futuro), então o wrapper só pagava custo: cabeçalho de 16 bytes, três chamadas de `Layout` com `expect` por
alocação e por liberação, e o `realloc` padrão (aloca, copia, libera sempre). Mudanças, todas mantendo o
comportamento com o rastreio ligado:
- `get_wrapped_layout` virou conta direta (`align = max(16, pedido)`, deslocamento igual ao alinhamento, tamanho
  arredondado ao alinhamento) e `#[inline(always)]`: era uma função separada no perfil.
- `dealloc` só lê o cabeçalho e consulta o grupo quando há rastreador; desligado é só a liberação de baixo.
- `realloc` sobrescrito: desligado, redimensiona no alocador de baixo (o mimalloc em geral estende no lugar),
  zerando o cabeçalho como o `alloc` faz; ligado, o caminho padrão com contagem. Vec que cresce deixa de copiar.

## 5. Alocações por chamada (não feito nesta fatia)

Por chamada hoje: o `Vec` de args, o `Rc<Env>`, o `Vec` de `LocalMap.items` (já com `reserve(args+4)`), a pilha do
`Frame` (`with_capacity(8)`) e a entrada de `vm.frames`. Reaproveitar `stack` e `items` com um pool por thread pede
`Drop for Frame` (hoje `Frame` é desmontado por padrão em vários pontos, e `Drop` proíbe isso) e devolução do
`Env` quando o `Rc` é único no `end_call`. Fica para a fatia 2, com o perfil novo em mãos: depois dos itens 1 e 4
o peso relativo de `alloc`/`drop` muda e vale medir antes de mexer.

## Fatia 2

### 6. `fold::builtin_name` O(1) (ganho esperado: 1 a 3% em laços com `len`/`ord`/`range`...)

A varredura linear de `builtins::TABLE` (dezenas de entradas, comparação de `&str`) por chamada de nativa virou
um `HashSet<&'static str>` num `OnceLock`, construído na primeira consulta com os mesmos nomes de `TABLE`. O
resultado é idêntico (`Some(f.name)` se o nome está na tabela, `None` se não; `Builtin(name)` segue direto).

### 7. `gthread::slice_expired` com `Cell<bool>` (ganho esperado: 1 a 3%)

Antes: dois `thread_local` com `RefCell` (`SCHED.borrow()` e `SLICE`) por instrução. Agora `OTHERS`
(`thread_local` `Cell<bool>` const) espelha `!SCHED.entries.is_empty()`, e o caso comum (nenhuma outra thread
verde) é uma leitura. Invariante: toda mutação de `Sched::entries` chama `sync_others` no fim: `spawn`, o
`remove` de `green_switch`, o reinsert do ramo de falha de `green_switch`, e `park_and_switch`. O fim de uma
thread (`Finish`) passa pelo `remove` de `green_switch` (e por `park_and_switch` com `mine = None`), sem outro
ponto de escrita em `entries`. Quem adicionar um novo caminho que mexa em `entries` precisa chamar `sync_others`.

### 8. Checagens por instrução no topo de `run_frames` (NÃO juntadas)

Medi no código: `finalize::pending()`, `signals_armed()`, `tracing::active()` e `ARMED` já são uma leitura de
`Cell` const (ou de atômico relaxado, que vira um `mov`); `slice_expired` agora também. Juntá-las num único
"há trabalho" exigiria que cada gravador (queda de objeto com `__del__` em `finalize`, `arm_signals`,
`settrace`/`setprofile`, `globalsview::ARMED`) também levantasse um sinalizador combinado, em arquivos de
outros donos e sem poder compilar nem medir aqui. Além disso as quatro condições têm de valer
instrução a instrução (`__del__` logo depois da que soltou a referência, `line` sem atraso, escrita por
`globals()` antes da seguinte), então o ganho teórico (quatro leituras de memória quente e previsível por uma)
não paga o risco de atrasar um efeito observável. Reavaliar só se o perfil novo ainda mostrar o topo do laço.

## Outros achados para as próximas fatias

- `fold::builtin_name` faz `TABLE.iter().any` linear para toda `NativeFn` chamada (`len(x)`, `ord(c)`...), e
  `call_or_enter` o chama duas vezes por chamada de nativa. Candidato a comparação por ponteiro ou campo no `NativeFn`.
- O topo do laço de `run_frames` consulta por instrução `finalize::pending()`, `signals_armed()`, `tracing::active()`
  e `ARMED`: quatro leituras de thread local ou atômico. Pode virar um único `u8` de "há trabalho entre instruções".
- `Dict::set` faz `fetch_add` atômico por escrita; um contador por thread seria mais barato, mas a unicidade entre
  dicts depende dele hoje (o filtro do item 1 também).
