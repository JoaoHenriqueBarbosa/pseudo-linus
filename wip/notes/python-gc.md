# Coletor de ciclos do ul-python (desenho)

Estado: desenho de 2026-10-11, antes da primeira fatia. Medições citadas vêm do oráculo (CPython 3.13.5, cena
`testbench/oracle/automation/cases/gc_probe0.py`, golden `gc_probe0.out`).

## O problema

A VM conta referências (`Rc`) e não tem coletor de ciclos. Um ciclo (classe com referência ao pai, closure que se
cita, exceção com traceback, `asyncio`) vaza para sempre, e o que depende do coletor diverge do CPython:
`gc.collect()` devolve 0, `__del__` e callbacks de `weakref` de objetos em ciclo só rodam na saída do interpretador,
`gc.get_objects()`/`get_referrers()` devolvem `[]`, `gc.get_stats()` e `get_count()` são fixos.

## O que o oráculo mede (fatos, não hipóteses)

- `gc.get_threshold()` é `(2000, 10, 10)` no 3.13.5 (não 700), `get_count()` começa `(20, 3, 0)` depois do import.
- `gc.collect()` devolve o número de objetos inalcançáveis achados, contando cada objeto do CPython: 10 pares de
  instâncias em ciclo dão 20; `a.me = a` dá 1; a lista que se contém dá 0 até soltar (2 ao soltar a lista e o dict
  juntos); um closure que se cita dá 4 (a lista, a função, a célula e a tupla de closure).
- Instância sem `__dict__` materializado conta 1 (valores inline); o `__dict__` só conta se foi pedido.
- `gc.collect(n)` incrementa `collections` da geração `n` em `get_stats()`; `gc.disable()` não impede `collect()`.
- `get_freeze_count()` é 0 no começo; `get_objects(2)` vazio de início (tudo em gen 0/1 depois do import).

## A decisão: registro das alocações mais eliminação por tentativa sobre contagens de `Rc`

Não dá para achar lixo cíclico sem uma lista dos objetos vivos, e `Rc` não tem gancho de decremento. Logo:

1. **Registro por alocação.** Cada contêiner mutável que pode fechar um ciclo entra num registro por thread no
   momento em que nasce: `list`, `dict`, `set`, instância, `Env` (o escopo de uma função, que guarda as células das
   closures) e o núcleo de gerador. É um `Weak` por objeto, num vetor por geração (0, 1, 2), mais o número de
   alocações menos desalocações para o limiar. Os pontos de alocação são poucos (`Value::list/dict/set`,
   4 `Rc::new` diretos, 3 de `InstanceObj`, 2 de `Env`), então o custo é um `Rc::downgrade` e um `push`. Tupla,
   método ligado, função e célula não precisam entrar: nenhum ciclo existe sem um contêiner mutável, e esses são
   achados como nós intermediários na travessia (por endereço).
2. **Arestas exatas, separadas do `get_referents`.** O `referents.rs` imita o `tp_traverse` do CPython (o que o
   usuário enxerga). O coletor precisa das arestas de `Rc` que de fato seguram cada objeto, uma função `edges(nó)`
   à parte, **exata**: cada aresta devolvida corresponde a um `Rc` forte guardado pelo nó. Aresta a mais coleta
   objeto vivo (corrupção); aresta a menos só deixa o objeto como raiz (vazamento conservador). Por isso a regra é
   sempre subestimar: estrutura que não sei percorrer (dict de módulo, dict de classe, quadros em execução, `Native`)
   conta como dono externo.
3. **Eliminação por tentativa (como o `deduce_unreachable` do CPython).** Para a geração `g` coletada (e as mais
   novas): `gc_refs(o) = strong_count(o)`; para cada nó `o` do conjunto, cada aresta `o -> p` com `p` no conjunto
   subtrai 1 de `gc_refs(p)`; os nós com `gc_refs > 0` são raízes (alguém de fora segura); tudo que as raízes
   alcançam é vivo; o resto é inalcançável. Nós achados só por travessia (tupla, função, célula) entram no
   conjunto da coleta em que foram achados. A thread da VM é única por processo, então não há corrida.
4. **Finalização e quebra, na ordem do CPython** (`gc_collect_main`): (a) `handle_weakrefs` limpa as referências
   fracas ao lixo e chama os callbacks das que sobrevivem; (b) `finalize_garbage` roda o `__del__` uma vez por
   objeto (`finalized`, a marca que o `finalize.rs` já tem); (c) `handle_resurrected_objects` refaz a análise: se
   o `__del__` ressuscitou algo, o objeto e o que ele alcança voltam a ser vivos; (d) `delete_garbage` chama o
   equivalente do `tp_clear` (esvazia a lista, o dict, o dict de atributos da instância, as variáveis do `Env`),
   o que derruba as contagens e faz o `Drop` normal liberar. Com `DEBUG_SAVEALL` o lixo vai para `gc.garbage` em
   vez de ser limpo.
5. **O valor de `gc.collect()`** é a contagem de objetos do CPython no lixo, não de nós daqui: cada tipo tem um
   peso por objeto (instância 1, lista 1, dict 1, função 1, célula 1, tupla de closure 1, o `__dict__` de uma
   instância só se foi materializado). Cada peso é fixado por uma cena do oráculo; onde o plinus tem um nó a mais
   ou a menos que o CPython (por exemplo o `Env`, que no CPython vira as células e a tupla de closure), a contagem
   vem de uma tabela por tipo, nunca de um ajuste global.
6. **Gatilho.** O contador de alocações da geração 0 passa de `threshold0` (2000): coleta a geração 0 no próximo
   ponto seguro do laço (o mesmo da finalização, `finalize::pending`), promove os sobreviventes; `threshold1`
   coletas da 0 disparam a 1, e `threshold2` da 1 a 2 (o 3.13.5 não tem o limite de 25% de longevidade da 3.12
   para a geração 2: a medição por cena confirma). `gc.disable()` desliga só o gatilho automático.

## Fatias (um commit cada, em container, com medição de memória)

1. **Registro e visão:** o registro, `gc.get_objects(generation)`, `gc.get_referents`, `gc.get_referrers`,
   `gc.is_tracked`, `gc.get_count`, `gc.freeze/unfreeze/get_freeze_count`. Medição: loop que aloca e solta
   contêineres não cresce o registro (os `Weak` mortos são podados como no `finalize.rs`).
2. **Coleta:** as arestas exatas, a eliminação por tentativa, o `gc.collect()` com o número do oráculo, os limiares e
   as gerações, `get_stats`, `gc.disable/enable/isenabled`. Medição: loop criando ciclos de instâncias, listas e
   closures mantém a memória (USS e `len(gc.get_objects())`) estável.
3. **Finalização:** `__del__` uma vez, ressurreição, ordem, callbacks de `weakref` de ciclo, `gc.garbage` com
   `DEBUG_SAVEALL`, `gc.callbacks`, `DEBUG_STATS/COLLECTABLE/UNCOLLECTABLE`.
4. **Greenlet:** os cinco testes de ciclos saem das divergências declaradas (`test_circular_greenlet`,
   `test_inactive_ref`, `test_threaded_leak`, `test_threaded_adv_leak`, `test_issue251_killing_cross_thread_leaks_list`).

## Riscos conhecidos

- Aresta de mais corrompe: cada tipo ganha um teste que compara `strong_count` esperado com as arestas listadas.
- Custo de alocação: medir (`pystone`-like) antes e depois da fatia 1; o registro é um `push` por contêiner.
- Quadros em execução e o `Env` do módulo (`globals`) seguram valores por fora do registro: são raízes, e o que só
  eles alcançam nunca é coletado, como no CPython.
- Uma coleta roda código Python (`__del__`, callbacks) e pode alocar: o gatilho só dispara no ponto seguro do laço,
  nunca no meio de uma instrução.
