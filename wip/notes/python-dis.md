# python-dis: bytecode do CPython 3.13 para `dis`, `co_code` e `co_lines`

Objetivo: `f.__code__.co_code`, `dis.dis(f)`, `dis.get_instructions`, `co_lines`, `co_positions`,
`co_linetable`, `co_exceptiontable`, `co_consts`, `co_names` e `co_stacksize` saírem idênticos aos do CPython
3.13 do Debian. O `dis.py`, o `opcode.py` e o `_opcode_metadata.py` rodam como estão no disco
(`kernel/image/usr/lib/python3.13`), embutidos por `modules/pysrc.rs`; só o `_opcode` é nativo.

## Peças

| Arquivo | Papel |
|---|---|
| `crates/ul-python/src/cpyops.rs` | Tabela de opcodes (número, marcas `ARG/CONST/NAME/JUMP/FREE/LOCAL/EXC`, CACHE em linha), efeito na pilha, nomes de intrínsecas e de `NB_*`. Fonte única do `_opcode` e do emissor. As marcas batem com as listas `has*` medidas no oráculo (ver abaixo). |
| `crates/ul-python/src/modules/opcodenative.rs` | `_opcode`: `stack_effect`, `is_valid`, `has_*`, `get_intrinsic1_descs`, `get_intrinsic2_descs`, `get_nb_ops`, `get_executor`, `ENABLE_SPECIALIZATION`. |
| `crates/ul-python/src/cpybc.rs` | Emissor: AST para grafo de blocos, passes do `flowgraph.c`, montagem, tabela de localização. Guarda tudo em `Emitted`. |
| `compile.rs` | `Code::cpy: Option<Rc<Emitted>>`, preenchido em `compile_module`, no fim de `make_function` (depois de `seal_scope`, que fixa `varnames`, `cellvars`, `freevars`; `def` por `cpybc::function`, `lambda` por `cpybc::lambda`) e no `compile(..., 'eval')`. |
| `tbobj.rs`, `builtins_ext.rs` | Atributos `co_code`, `_co_code_adaptive`, `co_linetable`, `co_exceptiontable`, `co_stacksize`, `co_consts`, `co_names` e os métodos `co_lines`, `co_positions`, `_varname_from_oparg` (índice no `localsplus`: `co_varnames`, células que não são parâmetros, livres). O resultado do `compile()` (`CodeSource`) delega ao mesmo objeto. `co_consts` troca cada marca `cpybc::CODE_CONST` (um `Value::Builtin` na posição do código aninhado) pelo objeto `code` da n-ésima entrada de `Code::functions`; o `repr` do `code` leva `, line N` e o endereço do `Rc<Code>` (o mesmo em todo acesso). |

## Emissão paralela

O interpretador continua executando o `Op` interno. O emissor não traduz o `Op`: ele percorre a mesma árvore
com a lógica de `Python/codegen` do 3.13 e produz uma sequência de instruções do CPython. Traduzir o `Op` não
funciona porque o interno não guarda o que o 3.13 precisa (CACHE, `TO_BOOL`, `COPY` de atribuição múltipla,
`PUSH_NULL`, localização por instrução, ordem de `co_consts` e `co_names`).

Pipeline (cada passo existe em `Cfg` ou `Gen` com o nome do passo do CPython):

1. Geração (`codegen.c`): blocos básicos, `JUMP` pseudo, localização por instrução, `NO_LOCATION` onde o CPython usa.
   Depois de salto ou de saída (`RETURN_*`) o próximo `push` abre bloco novo. O `FOR_ITER` conta como salto.
2. `eliminate_empty_basic_blocks`.
3. `inline_small_exit_blocks` (salto incondicional para saída de até 4 instruções vira cópia; o salto vira `NOP`).
4. `fold_tuple_on_constants` (`Gen::fold_tuples`: `LOAD_CONST` n vezes + `BUILD_TUPLE n` vira o `LOAD_CONST` da tupla) e
   `optimize_basic_block`, só os padrões gerados: `COMPARE_OP`+`TO_BOOL` (o `TO_BOOL` some e o `COMPARE_OP` ganha o bit 16),
   `IS_OP`/`CONTAINS_OP`+`TO_BOOL`, `LOAD_CONST`+`RETURN_VALUE` para `RETURN_CONST`, enfiada de saltos (`jump_thread`, menos `FOR_ITER`): como no 3.13 o salto antigo vira `NOP` (com a localização dele) e um salto novo, com o opcode do antigo e a localização do salto do destino e sem tratador (`i_except`), fecha o bloco. Visível no `if` de uma compreensão: o `POP_JUMP_IF_TRUE` e o `JUMP_BACKWARD` levam a localização do elemento, não a da condição, e ficam fora da faixa da tabela de exceções (golden `dis-listcomp`, `dis-genexp`). O `POP_BLOCK` não vira `NOP` em `label_exception_targets`: segue pseudo-opcode (sem localização, nenhum passe de limpeza o remove) até `convert_pseudo_ops`, depois de `resolve_line_numbers`; o `NOP` herda a linha da instrução anterior e só some se a anterior do mesmo bloco ou a próxima tem a mesma linha, então o do `with` sobra quando abre um bloco cuja anterior está noutro bloco (golden `dis-generator-try-finally`, `dis-try-nested`).
5. `remove_redundant_nops`: o `NOP` some sem linha, quando a próxima instrução tem a mesma linha (ou nenhuma: ela herda a
   localização do `NOP`) e, por último, quando a anterior tem a mesma linha. A ordem foi medida: `def h(): pass` deixa o
   `RETURN_CONST` com a posição do `pass`.
6. `mark_reachable` (apaga o inalcançável), NOPs, blocos vazios, `remove_redundant_jumps`. Não há segunda `inline_small_exit_blocks`: o
   3.13.5 só inlina antes do `optimize_basic_block` (`inline_small_or_no_lineno_blocks`, confirmado no `flowgraph.c` da tag v3.13.5), e o
   oráculo mostra o fim de um `match` (`POP_TOP`, `POP_TOP`, `RETURN_CONST`, três instruções depois dos NOPs) sem a cópia. O
   `remove_redundant_nops_and_pairs` (`LOAD_CONST`/`COPY 1` seguido de `POP_TOP` viram NOP) ainda não existe no emissor.
7. `remove_unused_consts` (a constante 0 fica sempre, pois pode ser a docstring).
8. `add_checks_for_loads_of_uninitialized_variables`: `LOAD_FAST` de local possivelmente sem valor vira `LOAD_FAST_CHECK`
   (máscara de 64 bits por bloco, junção por união; índice >= 64 sempre checa).
9. `insert_superinstructions`: `LOAD_FAST_LOAD_FAST`, `STORE_FAST_LOAD_FAST`, `STORE_FAST_STORE_FAST` (índices < 16 e mesma linha).
10. `resolve_line_numbers`: `duplicate_exits_without_lineno` (cópia da saída sem linha para cada salto que a alcança, posta logo
    depois da original, com a localização do salto) e `propagate_line_numbers` (herda a localização da instrução anterior).
11. `insert_prefix_instructions` (`COPY_FREE_VARS n`, depois um `MAKE_CELL` por célula em ordem de índice, sem localização:
    o `dis` mostra `--`) e `normalize_jumps` (salto condicional para trás vira o salto inverso para o bloco seguinte mais um
    bloco novo com o `JUMP` para trás, na localização do condicional).
12. Montagem: pseudo `JUMP` vira `JUMP_FORWARD` ou `JUMP_BACKWARD` (pela posição do bloco de destino), `LOAD_CLOSURE` vira
    `LOAD_FAST`, argumento relativo em unidades de código contado depois das CACHE (para trás: fim da instrução menos o
    destino), `EXTENDED_ARG` com ponto fixo para os saltos, CACHE zeradas, tabela de localização de 3.13 (formas curta, uma
    linha, sem coluna, longa e nenhuma; entradas de até 8 unidades; instruções vizinhas com a mesma localização viram uma entrada).

`co_stacksize` vem da profundidade máxima por varredura do grafo com o `stack_effect` de `cpyops`. `RETURN_CONST` conta +1
(o oráculo dá 1 para `def h(): pass` e para o módulo vazio); o efeito dele isolado não foi medido (ver abaixo).

O emissor devolve `None` (e `Emitted::synthetic` toma o lugar) se alguma construção não é coberta, ou se o número de funções
aninhadas que ele criou não bate com `Code::functions` (a correspondência marca a marca, na ordem de criação, depende disso).

## Medido no oráculo (2026-10-07) e reproduzido por teste

`wip/notes/python-dis-oracle.txt` guarda a saída crua. O teste `cpython_bytecode_matches_oracle_measurement`
(`lang_tests.rs`) compara com ela, byte a byte (menos os endereços do `repr`): `co_linetable` e `co_stacksize` do módulo de
exemplo, o `dis.dis` completo, `co_linetable`, `co_stacksize`, `co_code` e as duas últimas posições de `f`, `g` e `h`, o
módulo vazio, as listas `hasarg`/`hasconst`/`hasname`/`hasjump`/`hasfree`/`haslocal`/`hasexc` e o `stack_effect` de
`RETURN_GENERATOR`, `SEND`, `FOR_ITER`, `LOAD_SUPER_ATTR`, `CALL_FUNCTION_EX` (os três últimos com `oparg` 1; `jump` ausente,
falso e verdadeiro). O que a medição mudou:

- `RETURN_GENERATOR` tem efeito +1 (a tabela dava 0). `RETURN_CONST` passou a +1 pelo `co_stacksize` medido.
- Marcas: `COPY_FREE_VARS` e `LOAD_CLOSURE` não são `has_free`; `INSTRUMENTED_LOAD_SUPER_ATTR` não é `has_name`;
  `INSTRUMENTED_CALL_FUNCTION_EX` não tem argumento; `INSTRUMENTED_FOR_ITER`, `INSTRUMENTED_JUMP_*`,
  `INSTRUMENTED_POP_JUMP_*` e `SETUP_*` não são `has_jump` (`SETUP_*` seguem `has_exc`).
- `os.getcwd()` no módulo, dentro de função: `LOAD_GLOBAL`, `LOAD_ATTR` simples, `PUSH_NULL` (com a localização do callee), `CALL`.
  Confirma que o 3.13 não otimiza chamada de método sobre nome importado.
- `x is None` em condição: `POP_JUMP_IF_NOT_NONE`/`POP_JUMP_IF_NONE` direto, na localização do teste inteiro.
- Chamada de método com nomeados: o `LOAD_CONST ('k',)` leva a localização do `LOAD_ATTR` (`x.m`), o `CALL_KW` a da chamada.
- `return` de constante: `RETURN_CONST` com a localização da constante (`NOP` do `return` some).

## Como cada construção sai

| Fonte | Instruções (3.13) |
|---|---|
| módulo | `RESUME 0` com localização `(0, 1, 0, 0)` (linetable `f0 03 01 01 01`), corpo, `RETURN_CONST None` herdando a localização anterior. Docstring: `LOAD_CONST`, `STORE_NAME __doc__` na linha da instrução. |
| `def` | `RESUME 0` em `co_firstlineno` (o primeiro decorador, se houver); `co_consts[0]` é a docstring ou `None`; docstring sai do corpo. |
| nome local | `LOAD_FAST i` / `STORE_FAST i` (índice em `co_varnames`); `LOAD_FAST_CHECK` pela análise do passo 8. |
| nome de célula ou livre | `LOAD_DEREF i` / `STORE_DEREF i`, `i` no `localsplus` (célula que é parâmetro usa o índice do parâmetro). |
| nome global | função: `LOAD_GLOBAL (i<<1)\|nulo` (o `PUSH_NULL` da chamada é fundido no bit 0); módulo: `LOAD_NAME`, `PUSH_NULL` separado. |
| `a.b` | `LOAD_ATTR (i<<1)`, com `update_start_location_to_match_attr` quando o atributo atravessa linhas. |
| `f(x)` | callee, `PUSH_NULL` (ou fundido), argumentos, `CALL n` (3 CACHE), localização = a chamada inteira. |
| `o.m(x)` | `o`, `LOAD_ATTR (i<<1)\|1`, argumentos, `CALL n`. Se `o` é nome importado no módulo (`DEF_IMPORT` no escopo global), a chamada não é otimizada: `LOAD_ATTR` simples e `PUSH_NULL`. |
| `f(k=v)` | argumentos, valores, `LOAD_CONST ('k',)`, `CALL_KW n+k` (sem CACHE no 3.13). |
| `a + b` | `BINARY_OP nb` (1 CACHE); atribuição aumentada soma 13 ao `nb` e grava com a localização da instrução inteira. |
| constantes dobradas | `-1`, `-1.5`, `1 + 2`, `2 ** 3`, `'a' + 'b'`, tupla de constantes `(1, 2)`: `LOAD_CONST` com a localização do nó inteiro (`Gen::fold`, o `ast_opt.c`). Divisão por zero, `str % x` e tipos que falham em tempo de execução ficam sem dobrar. |
| `not x`, `-x`, `~x`, `+x` | `TO_BOOL`+`UNARY_NOT`, `UNARY_NEGATIVE`, `UNARY_INVERT`, `CALL_INTRINSIC_1 5`. |
| `a < b` etc | `COMPARE_OP (cmp<<5)\|máscara` (`<` 2, `<=` 42, `==` 72, `!=` 103, `>` 132, `>=` 172); `is`/`in` são `IS_OP`/`CONTAINS_OP`. `x in [1, 2]` e `for x in [1, 2]` percorrem a tupla constante. |
| `x[i]` | `BINARY_SUBSCR`; `x[i] = v` / `x.a = v`: `STORE_SUBSCR` / `STORE_ATTR`. |
| `a = b = v` | `v`, `COPY 1`, alvo, alvo. O `COPY` leva a localização do comando inteiro, não a do valor (golden `dis-unpack-assign` e `dis-unpack-attr-subscr-targets`: `i = j = k = t` dá `(4, 17)`). `a, b = b, a` e `a, b, c = c, a, b`: `BUILD_TUPLE`+`UNPACK_SEQUENCE` viram `SWAP n` e `apply_static_swaps` reordena os `STORE_FAST` (os alvos de atributo e subscrito mantêm o `SWAP`). |
| `[a, b]`, `(a, b)` | elementos, `BUILD_LIST n` / `BUILD_TUPLE n` (n <= 30). Lista com mais de dois elementos constantes: `BUILD_LIST 0`, `LOAD_CONST (tupla)`, `LIST_EXTEND 1`. |
| `{k: v}` | `{}`: `BUILD_MAP 0`. Chaves todas constantes e mais de uma: valores, `LOAD_CONST (chaves)`, `BUILD_CONST_KEY_MAP n`. Senão chave e valor alternados e `BUILD_MAP n` (n <= 15). |
| `if` | `compiler_jump_if`: `Not` inverte, `and`/`or` encadeiam, `x is None` vira `POP_JUMP_IF_NONE/NOT_NONE`, o resto `TO_BOOL`+`POP_JUMP_IF_*`; `JUMP` sem localização antes do `else`. |
| `for` | iterável, `GET_ITER`, `FOR_ITER` (1 CACHE, localização do iterável), `NOP` na linha do alvo, alvo, corpo, `JUMP_BACKWARD` sem localização, `END_FOR`, `POP_TOP` (sem localização). `else` depois do `POP_TOP`. |
| `while` | o teste duas vezes: no topo (`POP_JUMP_IF_FALSE` para depois do laço) e no fim do corpo (`POP_JUMP_IF_TRUE` para o corpo, que o `normalize_jumps` vira `POP_JUMP_IF_FALSE` + `JUMP_BACKWARD`). Teste constante: `None` (ver abaixo). |
| `break`, `continue` | `NOP` na linha do comando; `break` em `for` solta o iterador (`POP_TOP`) e salta para depois do laço; `continue` salta para o início (`FOR_ITER` ou o teste do topo). `return` dentro de `for`: `SWAP 2` + `POP_TOP` (só o `POP_TOP` se o valor é constante). |
| `return` | regra de `compiler_return`: constante ganha `NOP` na linha dela e `RETURN_CONST`; expressão: `RETURN_VALUE` com a localização da instrução `return`. |
| `def` (corpo do módulo ou de função) | decoradores; padrões (`BUILD_TUPLE n`, que o `fold_tuples` dobra se forem constantes); padrões só-nomeados (`LOAD_CONST (nomes)`, `BUILD_CONST_KEY_MAP n`); células de closure (`LOAD_CLOSURE i` por variável livre do código de dentro, em ordem alfabética, `BUILD_TUPLE n`); `LOAD_CONST (code)`, `MAKE_FUNCTION`, `SET_FUNCTION_ATTRIBUTE` com 8 (closure), 2 (padrões só-nomeados), 1 (padrões), nessa ordem; `CALL 0` por decorador, do último ao primeiro; `STORE_NAME`/`STORE_FAST`/`STORE_DEREF`. Tudo na localização do `def` inteiro. |
| `lambda` | igual ao `def` (expressão); o corpo é `None` como primeira constante, a expressão e `RETURN_VALUE` na localização da expressão. |
| célula e livre no prefixo | `COPY_FREE_VARS n`, `MAKE_CELL i` (um por célula, em ordem de índice), `RESUME`; as duas primeiras sem localização. |
| `import a`, `import a.b`, `import a as b` | `LOAD_CONST 0`, `LOAD_CONST None`, `IMPORT_NAME`, `STORE_*` do primeiro componente (ou do `as`). Tudo na localização do comando. |
| `from m import a, b as c` | `LOAD_CONST nível`, `LOAD_CONST ('a', 'b')`, `IMPORT_NAME m`, `IMPORT_FROM a`, `STORE_*`, ..., `POP_TOP`. |
| `global`, `nonlocal` | nada. |
| `pass`, constante solta | `NOP` na linha. |
| `eval` | `RESUME`, expressão, `RETURN_VALUE`. As posições são as da expressão tal como o `parse_expression` as dá (ver `compile::eval_module`), não as do embrulho `__eval_value__ = (...)`. |
| `class` (fatia 3) | Fora: decoradores, `LOAD_BUILD_CLASS`, `PUSH_NULL`, `LOAD_CONST (code)` (+ `MAKE_FUNCTION`, `SET_FUNCTION_ATTRIBUTE 8` se fecha variáveis), `LOAD_CONST 'Nome'`, bases, `CALL 2+n` (ou `LOAD_CONST (nomes)` + `CALL_KW`), `CALL 0` por decorador, `STORE_*`; tudo na localização do comando. Corpo (`cpybc::class_body`): `RESUME` na primeira linha (decorador incluso), `LOAD_NAME __name__`, `STORE_NAME __module__`, `LOAD_CONST qualname`, `STORE_NAME __qualname__`, `LOAD_CONST firstlineno`, `STORE_NAME __firstlineno__` (todos em `(L, L, 0, 0)`), docstring (`__doc__`), o corpo, `LOAD_CONST (atributos)`, `STORE_NAME __static_attributes__` e `RETURN_CONST None` sem localização. O objeto `code` só é emitido se o corpo não usa `__class__` nem fecha variável de função de fora. Classe dentro de função que fecha variáveis (golden `dis-class-in-function`): `compile.rs` (`class_def`) lista em `Code::freevars` da classe as variáveis de função de fora que o corpo e os métodos usam (ordem alfabética; o interpretador as ignora na execução); o corpo leva `COPY_FREE_VARS n` e `MAKE_CELL` da `__class__` no prefixo, o `localsplus` é células e depois livres, a leitura direta de uma livre é `LOAD_LOCALS` + `LOAD_FROM_DICT_OR_DEREF i` (`Gen::class_free`) e a tupla de células da criação da classe sai por `closure_code`. Fica de fora (esqueleto) o nome que o corpo também liga e fecha. |
| exceções (fatia 3) | `label_exception_targets` (pilha de `SETUP_*` por bloco, `POP_BLOCK` vira `NOP`), `push_cold_blocks_to_end` (tratadores no fim, com `JUMP` explícito onde o frio cairia no quente), `convert_pseudo_ops`, profundidade de entrada de cada bloco e `co_exceptiontable` (entrada: início, tamanho, destino, `profundidade << 1 \| lasti`, varint de 6 bits com o bit 7 no primeiro campo). `LOAD_FAST` depois de `DELETE_FAST` vira `LOAD_FAST_CHECK`, e os tratadores herdam a máscara de cada instrução protegida. |
| `try`/`except` | `SETUP_FINALLY except` (sem localização), corpo, `POP_BLOCK`, `else`, `JUMP end`; `except:` `SETUP_CLEANUP cleanup`, `PUSH_EXC_INFO`, por cláusula tipo, `CHECK_EXC_MATCH`, `POP_JUMP_IF_FALSE próxima` (na localização da cláusula); sem nome `POP_TOP`, corpo, `POP_BLOCK`, `POP_EXCEPT`, `JUMP end`; com nome `STORE_*`, `SETUP_CLEANUP cleanup_end`, corpo, `POP_BLOCK` x2, `POP_EXCEPT`, `LOAD_CONST None`, `STORE_*`, `DELETE_*`, `JUMP end`; `cleanup_end:` `LOAD_CONST None`, `STORE_*`, `DELETE_*`, `RERAISE 1`; depois das cláusulas `RERAISE 0` e `cleanup:` `COPY 3`, `POP_EXCEPT`, `RERAISE 1`. |
| `try`/`finally` | `SETUP_FINALLY end` (na localização do comando), corpo (ou o `try`/`except` inteiro), `POP_BLOCK`, o `finally`, `JUMP exit`; `end:` `SETUP_CLEANUP cleanup`, `PUSH_EXC_INFO`, o `finally` outra vez, `RERAISE 0` (na localização do último comando dele), `cleanup:` `COPY 3`, `POP_EXCEPT`, `RERAISE 1`. |
| `with` | contexto, `BEFORE_WITH`, `SETUP_WITH final`, alvo (`STORE_*`) ou `POP_TOP`, corpo, `POP_BLOCK`, `LOAD_CONST None` x3, `CALL 2`, `POP_TOP`, `JUMP exit`; `final:` `SETUP_CLEANUP cleanup`, `PUSH_EXC_INFO`, `WITH_EXCEPT_START`, `TO_BOOL`, `POP_JUMP_IF_TRUE suprime`, `RERAISE 2`; `suprime:` `POP_TOP`, `POP_BLOCK`, `POP_EXCEPT`, `POP_TOP` x2, `JUMP exit`; `cleanup:` `COPY 3`, `POP_EXCEPT`, `RERAISE 1`. |
| `return`, `break`, `continue` | `compiler_unwind_fblock_stack`: `for` solta o iterador (`SWAP 2` se guarda o valor, `POP_TOP`), `try` com `except` faz `POP_BLOCK`, tratador faz (um `POP_BLOCK` a mais se tem nome) `SWAP 2` se guarda o valor, `POP_BLOCK`, `POP_EXCEPT` e, se tem nome, `LOAD_CONST None`, `STORE_*`, `DELETE_*`. Dentro de `with` e de `try`/`finally` o emissor recusa. |
| `a and b`, `a or b` | cada operando menos o último: `COPY 1`, `TO_BOOL`, `POP_JUMP_IF_FALSE` (`and`) ou `_TRUE` (`or`) para o fim, `POP_TOP`; o último operando e o rótulo. Tudo na localização da expressão. |
| `a if c else b` | `compiler_jump_if(c, else, 0)`, `a`, `JUMP end` (sem localização), `b`. |
| `a < b < c` | valor: `a`, por elo `b`, `SWAP 2`, `COPY 2`, `COMPARE_OP`, `COPY 1`, `TO_BOOL`, `POP_JUMP_IF_FALSE cleanup`, `POP_TOP`; último `COMPARE_OP`, `JUMP end` (sem localização), `cleanup:` `SWAP 2`, `POP_TOP`. Em condição: sem o `COPY 1`, com `TO_BOOL` e o salto final, `cleanup:` `POP_TOP` (e `JUMP` para o destino se a condição é falsa). |
| `not (a is b)`, `not (a in b)` | o otimizador de AST troca por `is not` e `not in`, na posição da comparação; em condição vira `IS_OP 1` e salto direto, ou `POP_JUMP_IF_NOT_NONE`. |
| `f"..."` | cada parte; `FormattedValue`: valor, `CONVERT_VALUE 1\|2\|3` (`s`, `r`, `a`), especificação (um `JoinedStr`) e `FORMAT_WITH_SPEC`, senão `FORMAT_SIMPLE`; `BUILD_STRING n` se há mais de uma parte. Posições (golden `dis-fstring-many-parts`): o literal vizinho de um campo leva a posição do próprio token (`"x" "y"` fundidos cobrem os dois), e `{{`/`}}` contam as duas chaves: o tokenizador entrega uma só e pula a outra, então `parser/expr.rs` (`fstring_body`) estende em 1 o fim do literal quando o token seguinte começa uma coluna depois (`f"{{}}{a}"`: o `'{}'` vai de 79 a 83). |
| `x[a:b]`, `x[a:b] = v`, `x[a:b] += v` | `BINARY_SLICE`/`STORE_SLICE` com os dois limites (`None` no que falta); com passo ou em `del`: `BUILD_SLICE`. Aumentada: `COPY 3` x3 e `SWAP 4, 3, 2`. Subscrito: `COPY 2` x2 e `SWAP 3, 2`. Atributo: `COPY 1` e `SWAP 2`. |
| `a, *b = x` | `UNPACK_SEQUENCE n` ou `UNPACK_EX (antes + (depois << 8))` na localização do alvo, e um store por elemento. |
| `[*a, b]`, `(*a,)`, `{*a}`, `f(*a, k=1, **d)` | `BUILD_LIST/SET i` no primeiro `*`, `LIST_EXTEND 1`/`SET_UPDATE 1`, `LIST_APPEND 1`/`SET_ADD 1`; tupla termina em `CALL_INTRINSIC_1 6`. Chamada: posicionais como tupla (o único `*x` vai direto), nomeados num `BUILD_MAP`/`BUILD_CONST_KEY_MAP` com `DICT_MERGE 1`, `CALL_FUNCTION_EX 0\|1`. `{**a, 'k': v}`: `BUILD_MAP 0`, `DICT_UPDATE 1`. |
| `del`, `assert`, `raise` | `DELETE_FAST/DEREF/GLOBAL/NAME`, `DELETE_ATTR`, `DELETE_SUBSCR`; `assert`: salto se verdadeiro, `LOAD_ASSERTION_ERROR`, mensagem + `CALL 0`, `RAISE_VARARGS 1`; `raise`: `RAISE_VARARGS 0\|1\|2`. |
| `(x := v)` | `v`, `COPY 1`, store. |

## O que ainda não é coberto (devolve `None` e cai no esqueleto)

Enquanto o emissor devolve `None`, o `code` mostra `Emitted::synthetic` (um `RESUME` e um `NOP` por `Op` interno, com a tabela de
linhas e de colunas reais). **Isso ainda denuncia a simulação** em `dis.dis` desses códigos:

- compreensões inline (`cpybc/suspend.rs`: `LOAD_FAST_AND_CLEAR`, `SETUP_FINALLY` virtual, `STORE_FAST_MAYBE_NULL`, `SWAP` estático): na função, no módulo e no corpo de classe (o alvo é local rápido escondido: `Compiler::note_comp_target` o põe em `co_varnames` de qualquer escopo, `Gen::comp_locals` o resolve dentro da compreensão), com célula (o alvo que `lambda`/expressão geradora de dentro fecha ganha `MAKE_CELL` depois do `LOAD_FAST_AND_CLEAR`, e `STORE_DEREF` no laço; só em função: no módulo e na classe é recusado), com `await` e `async for` (`GET_AITER`, `SETUP_FINALLY`, `GET_ANEXT`, `SEND`, `END_ASYNC_FOR`). Geradores, `yield from`, `await`, `async def`, `async for`, `async with` e `<genexpr>` (síncrona ou assíncrona, `INTRINSIC_ASYNC_GEN_WRAP`) saem. `match` sai de `cpybc/pattern.rs`. Fora de função o alvo que `lambda`/expressão geradora fecha ainda é recusado (sem golden: o `co_cellvars` do módulo e a posição do `MAKE_CELL` não foram medidos). O nome de alvo lido fora da compreensão como global sai por `LOAD_GLOBAL` (`Gen::comp_only`, preenchido por `Compiler::comp_only`).
- `return`, `break` e `continue` dentro de `try`/`finally` (o emissor recusa, pois o CPython repete o corpo do `finally`); dentro de `with` e `async with` o emissor cobre (`POP_BLOCK`, `SWAP 2`, `__exit__(None, None, None)` e o `await` dele, e a localização corrente passa a ser a do `with`); `try`/`except*`, `match`.
- `class` com `*bases` ou `**kw`; classe com `global`/`nonlocal` ou que também liga um nome de função de fora que fecha; bases ou palavras-chave que contêm `lambda` (o interpretador cria a função antes do corpo, o CPython depois). O corpo com método que usa `__class__` ou `super()` sai: `Code::cellvars` do corpo é `('__class__',)` (`compile.rs`), `MAKE_CELL` no prefixo e `LOAD_CLOSURE`, `COPY 1`, `STORE_NAME __classcell__`, `RETURN_VALUE` no fim.
- `def` e `class` com parâmetros de tipo e `type X[T] = ...` (PEP 695, `cpybc/generic.rs` e `pep695.rs`): o escopo `<generic parameters of f>` sai com `TypeVar`/`ParamSpec`/`TypeVarTuple` por intrínsecas, limite, restrições e padrão como funções (`INTRINSIC_TYPEVAR_WITH_BOUND`, `_WITH_CONSTRAINTS`, `SET_TYPEPARAM_DEFAULT`), `.type_params`, `.generic_base` e `INTRINSIC_SET_FUNCTION_TYPE_PARAMS`; `type X = v` sem parâmetros não tem escopo. Conferido no golden `dis-generic-function-type-params` (alias, alias genérico, função e classe sem limites). O interpretador numera os locais do escopo à sua maneira, então `cpybc::layout` devolve o desenho do CPython para `co_varnames`/`co_cellvars`/`co_freevars` (campo `Code::type_params_role`). Fora: valores padrão na função genérica (`.defaults`/`.kwdefaults` como argumentos do escopo, ordem do `SWAP` não medida), genérico dentro de corpo de classe (`__classdict__`), nomes dos limites/padrões (o `code` dos lambdas leva o nome do parâmetro, mas a numeração de `co_consts` do escopo com limites não foi medida no oráculo), função genérica recursiva (o interpretador liga o nome dentro do escopo, o CPython usa o global: `co_freevars` difere), corpo de classe genérica que lê `T` (fica no esqueleto).
- Dobramento de constantes: `complex` (`2j`, `1 + 2j`, `-2j`, `+ - * /`; `**` fica) é constante do `co_consts` (`cpybc/complex.rs`: `repr`, `==`, `hash` do CPython; o programa continua compilando o literal como `complex(re, im)`, então `co_consts` mostra um valor que não é a classe `complex` de `_complex.py`); o inteiro grande (literal e conta, com os limites de `safe_multiply`/`safe_power`/`safe_lshift`: 128 bits) e `int ** negativo` saem por `crate::bigint::binary`; ficam `float //`, `float %`, `float **`, `not <constante>`, `~True`. Texto, bytes e tupla com `*` (limites de `safe_multiply`: 4096 itens de texto, 256 de tupla, 1024 no total aninhado), tupla com `+` e subscrito de constante por inteiro (`"abc"[1]`, `(1, 2, 3)[0]`) saem (`fold_seq_mult`, `fold_subscript`). O `co_consts[i] == 2j` compara com o `complex` do programa (`richcmp` do `ComplexConst`).
- `import`, `from m import x`, `from m import *` (`CALL_INTRINSIC_1 2`), `import a.b.c as d` (`IMPORT_FROM` encadeado), `from . import x` e `from __future__` saem; `x: T = v` e `x: T` (`SETUP_ANNOTATIONS`, `__annotations__` no módulo e na classe, só o alvo na função, texto da anotação com `from __future__ import annotations`) também.
- `try`/`finally`: o corpo do `finally` sai duas vezes, e uma `def` ou `lambda` dentro dele cria dois objetos `code` no CPython; só passa se o interpretador também os duplica (a conferência do número de funções barra o resto).
- `super().a`, `super(C, x).a` e a chamada de método sobre eles saem por `LOAD_SUPER_ATTR` (`Gen::super_attr`); `super` ligado no módulo (atribuição, `def`, `class`, `for`, `with`, `except`, `del`, `global`) desliga o `LOAD_SUPER_ATTR`, como o `import` (chave `SUPER_BOUND` de `module_imports`). Nomes `__privados` (mangling) e `__debug__` seguem fora.
- `f_lasti` e `tb_lasti` já seguem o `co_code` emitido (primeiro deslocamento da linha); no esqueleto seguem o esquema antigo.
- `stack_effect` de opcodes especializados (150 a 235) e `_opcode.is_valid` neles: a tabela só tem os não especializados.
- `co_lines()` e `co_positions()` devolvem `list_iterator`; o CPython devolve `line_iterator` e `positions_iterator`.
- `py/dis.py` (o stub antigo) ficou sem uso; remover com `git rm`.

## Atributos do objeto `code` e modos de `compile()` (sessão de 2026-10-07, lidos do golden, sem rodar)

- `co_flags` leva os bits `CO_FUTURE_*` (`Code::future_flags`, herdado pelo código aninhado em `Compiler::child`): só `annotations`
  (0x1000000) e `barry_as_FLUFL` (0x400000) ligam bit no 3.13 (`future.c`). O resultado de `compile()` delega `co_flags` ao `code` do módulo.
- `compile(..., 'single')`: `cpybc::module(.., interactive = true)` (`compile::reemit_interactive`); a expressão solta vira valor +
  `CALL_INTRINSIC_1 INTRINSIC_PRINT` + `POP_TOP` sem localização, e não há docstring (não passa por `compiler_body`).
- `from m import *` dentro de função ou classe é `SyntaxError: import * only allowed at module level` (na linha do comando), antes de rodar.
- Ler o nome `super` numa função usa `__class__` (`symtable_visit_expr`): `super(C, x)` de dois argumentos também fecha a célula, então o
  método tem `__class__` em `co_freevars` e `COPY_FREE_VARS`.
- `code.replace(**kw)` / `code.__replace__`: troca `co_name`, `co_qualname`, `co_filename`, `co_firstlineno` (desloca as linhas; a tabela
  de bytes fica igual); os demais campos que o CPython aceita (`co_code`, `co_consts`...) são aceitos e ignorados. `==` e `hash` de `code`
  seguem `code_richcompare` (o arquivo não entra), também no resultado de `compile()`.
- Pendências desta fatia: `dis-generic-function-type-params` (diferença depois dos primeiros 500 caracteres, não localizada por leitura);
  `dis-const-in-set-tuple` (o golden tem `frozenset({'b', 'a'})`, o repr de um conjunto de textos depende do `PYTHONHASHSEED` do oráculo,
  que não parece ser 0: caso instável); o `DeprecationWarning` de `co_lnotab` sai com `<stdin>:735` em vez de `<stdin>:7` (a linha do
  quadro do script vem errada em `warnings.warn` chamado por dentro de `getattr`).

### Sessão de 2026-10-07 (correções lidas do diff, nada rodado)

- `remove_redundant_jumps`: o salto sem localização removido leva consigo os `POP_BLOCK` que o precedem no bloco (no 3.13.5 a linha é propagada antes, e o `NOP` com a mesma linha do salto some; golden `dis-try-nested`).
- `Fb::With`/`AsyncWith` guardam a localização do item (`cm`), não a do comando (golden `dis-with-return-break-continue`); o `POP_BLOCK` do `async with` leva `loc` (golden `dis-async-with`: sobra o `NOP` do `pass`).
- `Instr::via`: o salto novo de `jump_thread` vira `JUMP_NO_INTERRUPT` só se o bloco do salto por onde passou é frio (golden `with-return-break-continue`: `JUMP_BACKWARD`).
- `co_flags`: corpo de classe nunca leva `CO_NESTED` (`dis-class-in-function`, `dis-decorators`).
- `super()` otimizado: o `LOAD_GLOBAL super` leva a posição do nome, `__class__` e o parâmetro a da chamada (`dis-super-and-class-cell`, `dis-class-in-function`).
- `END_ASYNC_FOR` do `async for` leva a posição do iterável (`dis-async-for`, `dis-async-generator`).
- Pendentes: `dis-class-in-function` (`CO_NESTED` já tratado).
- Correções da rodada seguinte (lidas do golden, nada rodado; conferir na próxima medição):
  - `match`: o `POP_TOP` que descarta o sujeito depois do padrão (e da guarda) leva a localização do padrão, não a herdada da guarda (`dis-match-capture-guard-as`, `-class`, `-subject-tuple`).
  - `type X = v`: o `RETURN_VALUE` da função do valor leva a localização da instrução `type` inteira (o `bool` da tupla `(nome, valor_de_alias)` em `pep695::Generic::lambda_names`, `Compiler::alias_return`, parâmetro `return_pos` de `cpybc::lambda`); limites e padrões de `TypeVar` seguem na da expressão (`dis-generic-function-type-params`).
  - `compile(..., 'eval')`: o módulo `__eval_value__ = expr` é remontado com a expressão do `parse_expression` (`compile::eval_module`), então o código aninhado (`lambda`, compreensão) sai com as colunas reais e o `shift` do `Gen` deixou de existir. Se a expressão começa com espaço ou o `parse_expression` recusa, o `cpy` do `eval` fica `None` como antes (`dis-eval-and-single-modes`).
  - `JUMP_NO_INTERRUPT` em `push_cold_blocks_to_end`: só o salto que sai de um bloco frio para um quente (o `via` conta o bloco de origem). Depois de um `async for` o resto da função só é alcançável pelo tratador do `END_ASYNC_FOR`, então fica todo frio e inline; o `JUMP` de volta do laço dentro dele continua `JUMP_BACKWARD` (`dis-async-comprehension`).
  - `DeprecationWarning` de `co_lnotab` com `<stdin>:735`: o corpo do módulo embutido (`warnings.py`, 735 linhas) rodava em `pysrc` sem devolver a `Vm::cur_line` ao valor do chamador; `import_value` por nativa deixava a linha do script velha até a próxima instrução. `pysrc` agora guarda e devolve `cur_line` em volta do corpo (vale para todo `warn` com `stacklevel` depois de um `import` nativo).
- `dis-const-in-set-tuple`: a bancada não fixa `PYTHONHASHSEED` no caso (só `yaml_libyaml.toml` fixa, com `=0`). Correção certa: prefixar `PYTHONHASHSEED=0` no script do caso e regenerar o golden no oráculo (não fiz: exige rodar o oráculo).

## Para medir no oráculo (Debian 3.13 em Docker), uma saída por construção

Rodar cada trecho no oráculo e devolver `co_code.hex()`, `co_linetable.hex()`, `co_stacksize`, `co_consts`, `co_names`, o
`dis.dis` completo e `list(co_positions())` (de cada função, módulo inclusive). O que está em dúvida em cada um fica ao lado.

1. **`for` simples** (fonte numa linha cada, sem linha em branco):
   ```python
   def f(xs):
       t = 0
       for x in xs:
           t += x
       return t
   ```
   Dúvidas: localização do `NOP` e do `END_FOR`/`POP_TOP`; se o `JUMP_BACKWARD` herda a linha do corpo; `STORE_FAST_LOAD_FAST` entre o alvo e o corpo; `LOAD_FAST_CHECK` de `t` depois do laço.
2. **`for` com `break`, `continue`, `else` e `return` dentro**:
   ```python
   def g(xs):
       for x in xs:
           if x == 1:
               continue
           if x == 2:
               break
           if x == 3:
               return x
       else:
           return -1
       return 0
   ```
   Dúvidas: `NOP` do `break`/`continue`; `POP_TOP` do `break`; `SWAP 2`+`POP_TOP` do `return x`; cópia da saída; `RETURN_CONST -1` (constante dobrada).
3. **`while` com teste variável e `else`**:
   ```python
   def w(n):
       while n:
           n -= 1
       else:
           n = 7
       return n
   ```
   Dúvidas: o teste duplicado e as localizações (`LOC(s)` ou do teste); `POP_JUMP_IF_FALSE`+`JUMP_BACKWARD` do `normalize_jumps`; caches.
4. **`while True`, `while 1`, `if 0`, `if 1 + 1`** (cada um em função própria, com `pass` e com `break`): é o que desbloqueia teste constante.
5. **Closure**:
   ```python
   def outer(n, m):
       k = n + 1
       def inner(x, y=2):
           return x + n + k
       return inner
   ```
   Dúvidas: `COPY_FREE_VARS` e `MAKE_CELL` sem localização e a ordem; índices de `LOAD_CLOSURE` (`dis` mostra `LOAD_FAST`); ordem dos `SET_FUNCTION_ATTRIBUTE` com closure e padrões juntos; localização do `BUILD_TUPLE` e do `LOAD_CLOSURE`; `co_cellvars`, `co_freevars`, `co_varnames`, `_varname_from_oparg(i)` para cada `i`.
6. **Padrões de argumentos**:
   ```python
   def p(a=1, b=(2, 3), *, c=4, d=None): pass
   m = lambda a=[1], b=None: a
   ```
   Dúvidas: `BUILD_TUPLE` dobrado em constante (`(1, (2, 3))`); `LOAD_CONST ('c', 'd')` + `BUILD_CONST_KEY_MAP 2`; localização do `BUILD_TUPLE` de padrões; ordem dos flags; consts do `lambda` (`None` primeiro).
7. **Decoradores**:
   ```python
   import functools
   @functools.lru_cache
   @staticmethod
   def d(): pass
   ```
   Dúvidas: ordem de `LOAD_*` dos decoradores; localização do `CALL 0` (do decorador) e do `STORE_NAME`; `co_firstlineno` do `d` (linha do primeiro decorador) e a linha do `RESUME`.
8. **Literais**:
   ```python
   def lit(a):
       x = [1, 2, 3]
       y = [a, 2]
       z = {'k': 1, 'j': a}
       w = {}
       v = {a: 1}
       u = (a, 1)
       t = (1, 2) + (3,)
       s = [1, 2] if a else (4, 5)
       return a in [1, 2, 3], 2 ** 3, -a, ~5, 'a' + 'b', 1 / 2, 7 // 2, 7 % 3, 1 << 3, True + 1
   ```
   Dúvidas: `BUILD_LIST 0`+`LOAD_CONST`+`LIST_EXTEND`; `BUILD_CONST_KEY_MAP`; `BUILD_MAP 0`; cada dobramento do `return` (quais viram `LOAD_CONST` e com que valor); `(1, 2) + (3,)` e `s` caem hoje no esqueleto (mostrar o que o CPython faz).
9. **`import`**:
   ```python
   import os, sys
   import os.path
   import os.path as p
   from os import path, sep as s
   from . import q
   from __future__ import annotations
   ```
   Dúvidas: ordem e localização dos `LOAD_CONST` do `import`; `STORE_NAME` do primeiro componente; `import a.b as c` (`IMPORT_FROM`, `SWAP`, `POP_TOP`); `from m import a, b` termina com `POP_TOP`; nível e `fromlist`; `from . import q`.
10. **`not` sobre `is`/`in` e teste constante em `if`**: `def n(a, b): return not (a is b), not (a in b), not a` e `def c(): if 1 + 1: return 1`. Dúvidas: o `not` vira `IS_OP 1`/`CONTAINS_OP 1` com a localização de quem; o `if` constante some inteiro?
11. **Efeito na pilha isolado**: `_opcode.stack_effect` de `RETURN_CONST` (oparg 0), `INSTRUMENTED_RETURN_CONST`, `END_FOR`, `GET_ITER`, `SET_FUNCTION_ATTRIBUTE`, `MAKE_FUNCTION`, `MAKE_CELL`, `COPY_FREE_VARS`, `LOAD_CLOSURE`, `IMPORT_FROM`, `IMPORT_NAME`, `BUILD_CONST_KEY_MAP` (oparg 2), `LIST_EXTEND` (oparg 1), `SWAP` (oparg 2), com `jump` ausente, falso e verdadeiro.
12. **Repr**: `repr(f.__code__)` (já sai `<code object f at 0x..., file "m", line 2>`); `repr(compile('1', 'm', 'exec'))` para o módulo; `outer.__code__.co_consts[2].co_qualname` (`outer.<locals>.inner`) e `co_qualname` do módulo (`<module>`).

### Fatia 3 (exceções, `class` e expressões): a medir, mesma saída por trecho

13. **`class` simples e com bases, palavra-chave e decorador**:
    ```python
    class A: pass
    class B(A, metaclass=type):
        "doc"
        x = 1
        def m(self): self.y = 2
    @dec
    class C: pass
    ```
    Dúvidas: ordem `LOAD_BUILD_CLASS`, `PUSH_NULL` (3.13 chama com o chamável antes do `NULL`; a 3.12 punha o `NULL` antes); índice de cada constante do módulo (código, nome, `None`); localização do `CALL` e do `STORE_NAME`; no corpo: `co_consts` completo (`'A'`, `1`, `()`, `None`?), `co_names`, localização do `RESUME` e dos `LOAD_NAME __name__`/`STORE_NAME __module__` (`(L, L, 0, 0)`?), se `__static_attributes__` sai com `NO_LOCATION` e herda a localização de `__firstlineno__`; docstring depois de `__firstlineno__`; `co_firstlineno` e `__firstlineno__` da classe `C` (linha do decorador); `co_flags` do corpo.
14. **`try`/`except` simples, com tipo, com nome e com `else`**:
    ```python
    def t(a):
        try:
            a()
        except ValueError as e:
            return e
        except (KeyError, TypeError):
            pass
        except:
            raise
        else:
            a = 1
        return a
    ```
    Dúvidas: o `return e` dentro do tratador com nome (`POP_BLOCK` x2, `SWAP 2`, `POP_EXCEPT`, `LOAD_CONST None`, `STORE_FAST e`, `DELETE_FAST e`, `RETURN_VALUE`); um ou dois `POP_BLOCK` no fim do tratador com nome; `COPY 3`/`POP_EXCEPT`/`RERAISE 1` do `cleanup`; localização do `RERAISE 0` final; `co_exceptiontable` (profundidade, `lasti`), ordem dos blocos frios, `LOAD_FAST_CHECK` de `e`/`a`; `co_stacksize`.
15. **`try`/`finally` e `try`/`except`/`finally`**:
    ```python
    def f(a):
        try:
            a()
        finally:
            a = 0
    def g(a):
        try:
            a()
        except E:
            pass
        finally:
            a = 0
    ```
    Dúvidas: localização do `SETUP_FINALLY` (comando) e do `RERAISE 0` (último comando do `finally`?); duas cópias do `finally` com localizações iguais?; `JUMP exit` sem localização; entradas da tabela de `g` (corpo, tratador do `except`, tratador do `finally`); `return` e `break` dentro de `try`/`finally` (o emissor recusa hoje).
16. **`with` e `with a, b`**:
    ```python
    def w(a, b):
        with a as f, b:
            f.x = 1
    ```
    Dúvidas: um ou dois `POP_TOP`, `TO_BOOL` antes do `POP_JUMP_IF_TRUE` do `WITH_EXCEPT_START`; `POP_BLOCK` da saída normal (localização); localização do `CALL 2` e dos três `LOAD_CONST None` (item `a` ou comando?); `SETUP_WITH` com `lasti`; `return` dentro do `with` (recusado hoje: `LOAD_CONST None` x3, `CALL 2`, `POP_TOP` inline).
17. **Compreensões** (a medir antes de emitir):
    ```python
    def c(xs, y):
        a = [x for x in xs]
        b = {x for x in xs if x}
        d = {x: y for x in xs}
        return a, b, d, [y for _ in xs]
    cm = [i for i in range(3)]
    ```
    Dúvidas: `LOAD_FAST_AND_CLEAR`, `SWAP 2`, `SETUP_FINALLY` (antes ou depois do `BUILD_LIST`), `FOR_ITER`/`END_FOR`/`POP_TOP`, `LIST_APPEND 2` (profundidade), o `POP_BLOCK` antes ou depois do `END_FOR`, o tratador (`SWAP 2`, `POP_TOP`, `SWAP 2`, `STORE_FAST`, `RERAISE 0`) e a tabela; `co_varnames` de `c` e do módulo (a variável de iteração entra no módulo?); localização do `NOP` do alvo; `y` como célula.
18. **Geradores** (a medir antes de emitir):
    ```python
    def g(xs):
        yield 1
        x = yield
        yield from xs
    async def h(a):
        await a
    ```
    Dúvidas: `RETURN_GENERATOR`, `POP_TOP`, `RESUME 0` (localização `(L, L, -1, -1)`), `YIELD_VALUE` com argumento (profundidade) e `RESUME 1`, `GET_YIELD_FROM_ITER`, `SEND`, `END_SEND`, `CLEANUP_THROW`, `GET_AWAITABLE`, `RESUME 3`; o tratador implícito (`CALL_INTRINSIC_1 3`, `RERAISE 1`) e a tabela; posição de `COPY_FREE_VARS` em relação a `RETURN_GENERATOR`.
19. **f-strings**:
    ```python
    def f(a, b):
        return f"x{a}y{b!r:>{a}}z", f"{a}", f""
    ```
    Dúvidas: localização de cada `LOAD_CONST` literal e do `FORMAT_*` (parte ou f-string inteira); `CONVERT_VALUE` e `FORMAT_WITH_SPEC` com especificação aninhada; `BUILD_STRING n`; `f"{a}"` sem `BUILD_STRING`; `f""` (`LOAD_CONST ''`).
20. **Operadores e comparações**:
    ```python
    def o(a, b, c):
        return a + b, a - b, a ** b, a @ b, a < b < c, a == b != c, a is b is not c, a in b not in c, not a, not (a is b)
    ```
    Dúvidas: `COMPARE_OP` do encadeado (`COPY 1`, `TO_BOOL` antes do `POP_JUMP_IF_FALSE`; o bit 16 do último), `SWAP 2`/`COPY 2`, localização do `JUMP` sem localização, `cleanup:` (`SWAP 2`, `POP_TOP`); `IS_OP`/`CONTAINS_OP` no encadeado; `BINARY_OP` de cada operador; `not (a is b)` como `IS_OP 1` e sua localização.
21. **`and`/`or`/`if` como valor, em condição e aninhados**:
    ```python
    def b(a, b, c):
        x = a and b or c
        y = a if b else c
        if a and (b or c):
            return 1
        return not a and b
    ```
    Dúvidas: `COPY 1`, `TO_BOOL`, `POP_JUMP_IF_*`, `POP_TOP` com a localização da expressão inteira; `TO_BOOL` redundante antes de `UNARY_NOT`; cópia da saída (`RETURN_CONST 1`) e o bloco `end` do `if`.
22. **Subscritos, fatias e atribuição aumentada**:
    ```python
    def s(x, i, v):
        a = x[i]; b = x[1:2]; c = x[:]; d = x[::2]; e = x[1, 2]
        x[i] = v; x[1:2] = v; x[i] += v; x[1:2] += v; x.a += v
        del x[i], x[1:2], x.a
    ```
    Dúvidas: `BINARY_SLICE`/`STORE_SLICE` sem `BUILD_SLICE`, localização dos `LOAD_CONST None` (nó da fatia?); `x[1:2] += v` com `COPY 3` x3 e `SWAP 4, 3, 2`; `x.a += v` (`COPY 1`, `LOAD_ATTR`, `SWAP 2`, `STORE_ATTR`); `del x[1:2]` com `BUILD_SLICE`; `(1, 2)` constante como índice.
23. **Desempacotamento**:
    ```python
    def u(x, a, b):
        p, q = x
        r, *s, t = x
        [m, n] = x
        a, b = b, a
        for i, (j, k) in x:
            pass
    ```
    Dúvidas: `UNPACK_EX` e o argumento; localização do `UNPACK_SEQUENCE` (alvo inteiro); `a, b = b, a` (`SWAP 2` e a reordenação dos stores, hoje recusado); `STORE_FAST_STORE_FAST` entre os stores.
24. **Estrelas em literais e chamadas**:
    ```python
    def v(a, k, g):
        return [*a, 1], (*a, 2), {*a}, {**k, 'x': 1}, g(*a), g(1, *a, z=2), g(**k), g(a, **k, y=1), g.m(*a)
    ```
    Dúvidas: `BUILD_LIST 0` ou `BUILD_LIST i`, `LIST_EXTEND`/`LIST_APPEND`, `CALL_INTRINSIC_1 6`; `LOAD_CONST ()` para `g(**k)`; `BUILD_MAP 0`/`DICT_MERGE 1`/`DICT_UPDATE 1`; `CALL_FUNCTION_EX` e o `PUSH_NULL` (com `LOAD_GLOBAL` fundido); `g.m(*a)` sem o bit de método; `BUILD_CONST_KEY_MAP` em `g(a, **k, y=1)` (um nomeado: `LOAD_CONST 'y'`, valor, `BUILD_MAP 1`).
25. **`del`, `assert`, `raise`, walrus**:
    ```python
    def d(x, m):
        del x
        assert x, m
        assert x
        raise
        raise x from m
        if (y := x): pass
    ```
    Dúvidas: `DELETE_FAST` e o `LOAD_FAST_CHECK` seguinte; `assert`: `TO_BOOL`+`POP_JUMP_IF_TRUE`, `LOAD_ASSERTION_ERROR`, `CALL 0`, `RAISE_VARARGS 1`; o bloco depois de `raise` (inalcançável); `COPY 1` do walrus e a localização do store.
26. **`global`, `nonlocal` e nomes de classe**:
    ```python
    n = 0
    def g():
        global n
        n += 1
        del n
    def o():
        z = 0
        def i():
            nonlocal z
            z += 1
            del z
        return i
    ```
    Dúvidas: `STORE_GLOBAL`/`DELETE_GLOBAL`, `LOAD_GLOBAL` sem bit de chamada; `DELETE_DEREF`; ordem de `co_names`.
27. **`break`/`continue`/`return` dentro de `try`/`except`**:
    ```python
    def k(xs):
        for x in xs:
            try:
                if x: continue
                if x == 2: break
                return x
            except E as e:
                continue
        return 0
    ```
    Dúvidas: `POP_BLOCK` antes do salto em cada saída; `SWAP 2` e `POP_TOP` do iterador no `return x`; no tratador com nome o `POP_BLOCK` x2, `POP_EXCEPT`, `LOAD_CONST None`, `STORE_FAST e`, `DELETE_FAST e` antes do `JUMP`; `NOP` do `break`; tabela.
28. **Efeito na pilha dos opcodes novos**: `_opcode.stack_effect` de `PUSH_EXC_INFO`, `CHECK_EXC_MATCH`, `POP_EXCEPT`, `RERAISE` (oparg 0, 1, 2), `WITH_EXCEPT_START`, `BEFORE_WITH`, `LOAD_BUILD_CLASS`, `LOAD_ASSERTION_ERROR`, `FORMAT_SIMPLE`, `FORMAT_WITH_SPEC`, `CONVERT_VALUE`, `BUILD_STRING` (oparg 3), `UNPACK_SEQUENCE`/`UNPACK_EX`, `DICT_MERGE`, `DICT_UPDATE`, `LIST_APPEND`, `SET_ADD`, `MAP_ADD`, `SET_UPDATE`, `BINARY_SLICE`, `STORE_SLICE`, `DELETE_SUBSCR`, `DELETE_ATTR`, `RAISE_VARARGS` (oparg 0, 1, 2), `SETUP_FINALLY`, `SETUP_CLEANUP`, `SETUP_WITH`, `POP_BLOCK`, com `jump` ausente, falso e verdadeiro.

#### Fatia 3, o que fica para depois

- **Compreensões inline** (`push_inlined_comprehension_state`): para cada local da compreensão, `LOAD_FAST_AND_CLEAR` e `SWAP`; `SETUP_FINALLY cleanup`; `BUILD_*`; laço como o `for` (`GET_ITER`, `FOR_ITER`, alvo, `ifs` por `compiler_jump_if`, `LIST_APPEND depth+1`); `POP_BLOCK`, `JUMP end`; `cleanup:` `SWAP`, `POP_TOP`, restaura os locais, `RERAISE 0`; `end:` restaura os locais. Precisa de `Code::varnames` com a variável de iteração (função e módulo) e da regra de quando o CPython não inlina (genexpr, classe, `async`).
- **Geradores**: `RETURN_GENERATOR`+`POP_TOP` antes do `RESUME` (inseridos antes de `COPY_FREE_VARS`), `YIELD_VALUE n` e `RESUME 1` depois dele, `SETUP_CLEANUP` no início e o bloco `CALL_INTRINSIC_1 3`/`RERAISE 1` no fim (`wrap_in_stopiteration_handler`).
- **`return`/`break`/`continue` em `with` e `finally`**: `compiler_unwind_fblock` de `WITH` (`POP_BLOCK`, `LOAD_CONST None` x3, `CALL 2`, `POP_TOP`) e de `FINALLY_TRY` (`POP_BLOCK` e o corpo do `finally` inline, com o quadro retirado enquanto ele é emitido).
- **`match`, `async with`, `async for`, `except*`**: nada feito.
