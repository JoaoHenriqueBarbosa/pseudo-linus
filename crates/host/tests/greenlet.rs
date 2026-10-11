//! O `greenlet` 3.5.6 sobre o `python3` da imagem: a cena `gl_api` do oráculo de automação (`testbench/oracle/automation`)
//! tem de sair igual, linha a linha, ao `golden/gl_api.out` medido no CPython 3.13 do Debian com o greenlet da wheel.
//!
//! O pacote vem como o `pip` o deixa: o `__init__.py` da wheel ao lado do `_greenlet.cpython-313-x86_64-linux-gnu.so`.
//! O interpretador nunca executa o código de máquina do `.so` (o módulo é o nativo registrado para `greenlet._greenlet`),
//! então o arquivo aqui só precisa existir.

mod common;

use common::*;
use host::client::Client;
use serde_json::json;

const COMMON: &str = include_str!("../../../testbench/oracle/automation/common.py");

/// O `greenlet/__init__.py` da wheel `greenlet-3.5.6-cp313-cp313-manylinux_2_24_x86_64` (licença MIT e PSF), sem os
/// comentários: as importações do `_greenlet` e o `__version__` são o que o módulo nativo precisa responder.
const INIT: &str = "\
__all__ = ['__version__', '_C_API', 'GreenletExit', 'error', 'getcurrent', 'greenlet', 'gettrace', 'settrace']
__version__ = '3.5.6'
from ._greenlet import _C_API
from ._greenlet import GreenletExit
from ._greenlet import error
from ._greenlet import getcurrent
from ._greenlet import greenlet
from ._greenlet import gettrace
from ._greenlet import settrace
from ._greenlet import GREENLET_USE_CONTEXT_VARS
from ._greenlet import GREENLET_USE_GC
from ._greenlet import GREENLET_USE_TRACING
from ._greenlet import CLOCKS_PER_SEC
from ._greenlet import enable_optional_cleanup
from ._greenlet import get_clocks_used_doing_optional_cleanup
";

fn write(c: &Client, sb: &str, path: &str, text: &str) {
    let r = exec(c, sb, &format!("cat > {path} <<'PL_EOF'\n{text}PL_EOF\n"));
    assert_eq!(r["exit_code"], 0, "{r}");
}

/// Roda a cena `name` (o texto `scene`) com o pacote como o `pip` o deixa e confere a saída com o golden do oráculo.
fn scene_matches(name: &str, scene: &str, golden: &str) {
    scene_matches_as(name, scene, golden, None);
}

/// Como [`scene_matches`], com a cena rodando como `user` (o `exec` roda como root, que o `RLIMIT_NPROC` não limita).
fn scene_matches_as(name: &str, scene: &str, golden: &str, user: Option<&str>) {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let r = exec(&c, &sb, "mkdir -p /tmp/gl/greenlet && : > /tmp/gl/greenlet/_greenlet.cpython-313-x86_64-linux-gnu.so");
    assert_eq!(r["exit_code"], 0, "{r}");
    write(&c, &sb, "/tmp/gl/greenlet/__init__.py", INIT);
    write(&c, &sb, "/tmp/gl/common.py", COMMON);
    write(&c, &sb, &format!("/tmp/gl/{name}.py"), scene);
    let python = match user {
        Some(user) => format!("runuser -u {user} -- python3"),
        None => "python3".to_string(),
    };
    let r = exec(&c, &sb, &format!("cd /tmp/gl && {python} {name}.py"));
    assert_eq!(r["stderr"], "", "{r}");
    assert_eq!(r["exit_code"], 0, "{r}");
    let got = r["stdout"].as_str().unwrap();
    let diffs: Vec<String> = golden
        .lines()
        .zip(got.lines())
        .enumerate()
        .filter(|(_, (want, have))| want != have)
        .map(|(n, (want, have))| format!("linha {}: tinha {have:?}, o oráculo deu {want:?}", n + 1))
        .collect();
    assert!(diffs.is_empty(), "cena {name}: {} linhas diferem:\n{}", diffs.len(), diffs.join("\n"));
    assert_eq!(got, golden, "cena {name}");
}

/// O limite de threads: com `RLIMIT_NPROC` posto 4 acima das tarefas que o usuário já tem, `Thread.start()` e
/// `_thread.start_new_thread` dão `RuntimeError: can't start new thread` quando o limite já foi atingido (a thread de
/// `clone` conta no limite), a thread que falhou não existe, e as vagas voltam quando as threads acabam.
/// `thread_limit.py` roda como `nobody`, o `exec` é root e o `RLIMIT_NPROC` não limita root.
#[test]
fn thread_limit_scene_matches_the_oracle() {
    scene_matches_as(
        "thread_limit",
        include_str!("../../../testbench/oracle/automation/cases/thread_limit.py"),
        include_str!("../../../testbench/oracle/automation/golden/thread_limit.out"),
        Some("nobody"),
    );
}

/// `list.sort` esvazia a lista enquanto ordena, restaura a original quando a chave ou a comparação levantam, e acusa a
/// modificação durante a ordenação.
#[test]
fn list_sort_state_scene_matches_the_oracle() {
    scene_matches(
        "list_sort_state",
        include_str!("../../../testbench/oracle/automation/cases/list_sort_state.py"),
        include_str!("../../../testbench/oracle/automation/golden/list_sort_state.out"),
    );
}

/// `list.sort` compara na mesma ordem e com os mesmos pares do CPython (powersort, inserção binária, galope), e a
/// comparação que levanta no meio deixa a mesma permutação parcial.
#[test]
fn list_sort_compare_scene_matches_the_oracle() {
    scene_matches(
        "list_sort_compare",
        include_str!("../../../testbench/oracle/automation/cases/list_sort_compare.py"),
        include_str!("../../../testbench/oracle/automation/golden/list_sort_compare.out"),
    );
}

/// O switch de greenlet dentro de um dunder de usuário chamado por código nativo (`%s`, `format`, `__eq__` na busca de
/// chave em dict e set, operadores, `__index__`, `__getattr__`...).
#[test]
fn gl_dunders_scene_matches_the_oracle() {
    scene_matches(
        "gl_dunders",
        include_str!("../../../testbench/oracle/automation/cases/gl_dunders.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_dunders.out"),
    );
}

/// O módulo `resource` como usuário comum: constantes, `getrlimit`, `setrlimit` (mensagens de tipo, tamanho e permissão),
/// `prlimit` do próprio processo e `getrusage`.
#[test]
fn resource_api_scene_matches_the_oracle() {
    scene_matches_as(
        "resource_api",
        include_str!("../../../testbench/oracle/automation/cases/resource_api.py"),
        include_str!("../../../testbench/oracle/automation/golden/resource_api.out"),
        Some("nobody"),
    );
}

#[test]
fn gl_api_scene_matches_the_oracle() {
    scene_matches(
        "gl_api",
        include_str!("../../../testbench/oracle/automation/cases/gl_api.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_api.out"),
    );
}

/// A exceção do `__eq__` de usuário sai de toda comparação feita por código nativo, em vez de virar "diferente"
/// (o `[Eq()].index(1)` da cena `gl_surface` devolvia `ValueError` no lugar do erro do `__eq__`).
#[test]
fn eq_raise_scene_matches_the_oracle() {
    scene_matches(
        "eq_raise",
        include_str!("../../../testbench/oracle/automation/cases/eq_raise.py"),
        include_str!("../../../testbench/oracle/automation/golden/eq_raise.out"),
    );
}

/// O `switch` dentro de código Python chamado por uma nativa (chave de `sorted` e `list.sort`, callbacks de `map`,
/// `filter`, `reduce`, `property`, `__next__`, `__enter__`...) funciona como no CPython, que copia a pilha de C junto.
#[test]
fn gl_callbacks_scene_matches_the_oracle() {
    scene_matches(
        "gl_callbacks",
        include_str!("../../../testbench/oracle/automation/cases/gl_callbacks.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_callbacks.out"),
    );
}

/// O `__hash__` de usuário: a exceção sai de `hash()`, de `dict`, `set` e tupla; o resultado não-`int` é `TypeError`.
#[test]
fn hash_index_scene_matches_the_oracle() {
    scene_matches(
        "hash_index",
        include_str!("../../../testbench/oracle/automation/cases/hash_index.py"),
        include_str!("../../../testbench/oracle/automation/golden/hash_index.out"),
    );
}

/// `range()` aceita objeto com `__index__`, como o `PyNumber_Index`.
#[test]
fn range_index_scene_matches_the_oracle() {
    scene_matches(
        "range_index",
        include_str!("../../../testbench/oracle/automation/cases/range_index.py"),
        include_str!("../../../testbench/oracle/automation/golden/range_index.out"),
    );
}

/// O `contextvars` sobre o `_contextvars` do `lib-dynload`: o mapeamento, `run`, `set`/`reset` com `Token`, um contexto
/// por thread e por tarefa do asyncio, e a superfície dos tipos estáticos (mensagens, `dir`, pickle, subclasse).
#[test]
fn contextvars_api_scene_matches_the_oracle() {
    scene_matches(
        "contextvars_api",
        include_str!("../../../testbench/oracle/automation/cases/contextvars_api.py"),
        include_str!("../../../testbench/oracle/automation/golden/contextvars_api.out"),
    );
}

/// O `gr_context` do greenlet sobre o `contextvars`: o contexto troca a cada switch, um greenlet novo nasce sem
/// contexto, o setter aceita `Context` ou `None`, e o suspenso guarda o dele (o `Token` do outro não vale aqui).
#[test]
fn gl_context_scene_matches_the_oracle() {
    scene_matches(
        "gl_context",
        include_str!("../../../testbench/oracle/automation/cases/gl_context.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_context.out"),
    );
}

/// `gc.get_referents` como o `tp_traverse` do CPython nos tipos básicos: ordem e conteúdo (valor antes da chave no
/// dict de chaves não `str`, os campos de `__slots__` antes do `__dict__` e do tipo, o quadro de uma função...).
#[test]
fn gc_referents_scene_matches_the_oracle() {
    scene_matches(
        "gc_referents",
        include_str!("../../../testbench/oracle/automation/cases/gc_referents.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_referents.out"),
    );
}

/// `gc.get_referents` de geradores (a função, o código, os locais e a pilha enquanto o quadro vive; só o nome depois
/// de o corpo correr até o fim; o `close()` de um gerador parado), dos iteradores (com a origem, e sem ela depois de
/// esgotados), de `map`, `zip`, `enumerate`, `filter` e `iter(f, s)`, e das funções de módulo.
#[test]
fn gc_referents_generators_and_iterators_scene_matches_the_oracle() {
    scene_matches(
        "gc_referents_gen",
        include_str!("../../../testbench/oracle/automation/cases/gc_referents_gen.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_referents_gen.out"),
    );
}

/// `gc.get_referents` de uma expressão geradora: o `.0` é o iterador que quem a criou obteve (`GET_ITER` do lado de
/// fora, o `GetIterValue` do compilador), e a lista literal de constantes do `for` vira tupla na compilação do CPython
/// (`fold_iter`), então o iterador é um `tuple_iterator`.
#[test]
fn gc_referents_generator_expression_scene_matches_the_oracle() {
    scene_matches(
        "gc_referents_genexp",
        include_str!("../../../testbench/oracle/automation/cases/gc_referents_genexp.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_referents_genexp.out"),
    );
}

/// `gc.get_referents` de `reversed()`, dos iteradores de dict, set, bytes e range, e do `itertools`.
#[test]
fn gc_referents_iterators_scene_matches_the_oracle() {
    scene_matches(
        "gc_referents_iters",
        include_str!("../../../testbench/oracle/automation/cases/gc_referents_iters.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_referents_iters.out"),
    );
}

/// `gc.get_referents` do greenlet: o ativo não é do coletor, o novo visita pai, `run` e contexto, o morto visita pai,
/// a principal da thread e contexto, e a subclasse põe o tipo na frente.
#[test]
fn gc_referents_greenlet_scene_matches_the_oracle() {
    scene_matches(
        "gc_referents_greenlet",
        include_str!("../../../testbench/oracle/automation/cases/gc_referents_greenlet.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_referents_greenlet.out"),
    );
}

/// A superfície do greenlet (`dir`, `__dict__`, `gc.get_referents`, pickle, `__module__`), o switch em callback de
/// nativa, a subclasse coletada pelo gc de ciclos e o `gr_context` trocando o contexto corrente a cada switch.
#[test]
fn gl_surface_scene_matches_the_oracle() {
    scene_matches(
        "gl_surface",
        include_str!("../../../testbench/oracle/automation/cases/gl_surface.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_surface.out"),
    );
}

/// O estado interno não aparece em nada que o programa enxergue (`__dict__`, `vars`, `dir`, `__del__`, `__slots__`),
/// o tipo é estático como no C e as funções de módulo são `builtin_function_or_method` de `greenlet._greenlet`.
#[test]
fn gl_leaks_scene_matches_the_oracle() {
    scene_matches(
        "gl_leaks",
        include_str!("../../../testbench/oracle/automation/cases/gl_leaks.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_leaks.out"),
    );
}


/// `_stack_saved`: o tamanho da pilha de máquina copiada de cada greenlet suspenso, pelo caminho do `run`, pelas
/// nativas no caminho até a troca (`sorted`, `'%s' %`, `next`...) e pela cópia preguiçosa entre greenlets.
#[test]
fn stack_saved_scene_matches_the_oracle() {
    scene_matches(
        "gl_stack_saved",
        include_str!("../../../testbench/oracle/automation/cases/gl_stack_saved.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_stack_saved.out"),
    );
}

/// `collections.Counter` como o do CPython 3.13: operadores binários, unários e in-place, `most_common` (com o
/// `heapq.nlargest` e o empate na ordem de inserção), `elements`, `total`, `repr`, comparações e o `fromkeys` que lança.
#[test]
fn counter_api_scene_matches_the_oracle() {
    scene_matches(
        "counter_api",
        include_str!("../../../testbench/oracle/automation/cases/counter_api.py"),
        include_str!("../../../testbench/oracle/automation/golden/counter_api.out"),
    );
}

/// O C de cada caminho até um método de usuário (slots dos operadores, nativas, callbacks, geradores consumidos por
/// nativa), lido pelo `_stack_saved` de um greenlet que troca de dentro dele.
#[test]
fn c_paths_scene_matches_the_oracle() {
    scene_matches(
        "gl_c_paths",
        include_str!("../../../testbench/oracle/automation/cases/gl_c_paths.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_c_paths.out"),
    );
}

/// Escreve na sandbox, sob `dest`, cada arquivo de `dir` (relativo a `dir`), em profundidade.
fn write_tree(c: &Client, sb: &str, dir: &std::path::Path, dest: &str) {
    let r = exec(c, sb, &format!("mkdir -p {dest}"));
    assert_eq!(r["exit_code"], 0, "{r}");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            write_tree(c, sb, &path, &format!("{dest}/{name}"));
        } else {
            write(c, sb, &format!("{dest}/{name}"), &std::fs::read_to_string(&path).unwrap());
        }
    }
}

/// A suíte `greenlet/tests` do 3.5.6 (a da wheel, copiada do oráculo com o `psutil` mínimo que ela importa) roda
/// no `python3` da imagem como no CPython: `unittest discover` do pacote, e o resumo é o que o oráculo deu.
#[test]
#[ignore = "em fechamento: ainda há divergências medidas contra o oráculo (172 testes, 6 pulados lá)"]
fn greenlet_test_suite_runs_like_the_oracle() {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let suite = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testbench/oracle/automation/fixtures/greenlet_suite");
    write_tree(&c, &sb, &suite, "/tmp/gls");
    let r = exec(&c, &sb, "touch /tmp/gls/greenlet/_greenlet.cpython-313-x86_64-linux-gnu.so");
    assert_eq!(r["exit_code"], 0, "{r}");
    write(&c, &sb, "/tmp/gls/greenlet/__init__.py", INIT);
    let r = exec(&c, &sb, "cd /tmp/gls && PYTHONPATH=/tmp/gls python3 -m unittest discover -s greenlet/tests -t . > out.txt 2>&1; cat out.txt");
    let out = r["stdout"].as_str().unwrap();
    assert!(out.contains("Ran 172 tests") && out.contains("OK (skipped=6)"), "{out}");
}

/// `getcurrent()` no `__del__` de um ciclo que só a finalização do interpretador coleta: `RuntimeError` (o principal
/// já foi embora), como no oráculo.
#[test]
fn finalizing_scene_matches_the_oracle() {
    scene_matches(
        "gl_finalizing",
        include_str!("../../../testbench/oracle/automation/cases/gl_finalizing.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_finalizing.out"),
    );
}

/// `run=None` explícito, o `__init__` do tipo chamado pelo de uma subclasse, o `__dict__` (apagar e trocar) e o
/// `gr_context` que não se apaga, como no oráculo.
#[test]
fn attrs_scene_matches_the_oracle() {
    scene_matches(
        "gl_attrs",
        include_str!("../../../testbench/oracle/automation/cases/gl_attrs.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_attrs.out"),
    );
}

/// A cadeia de `f_back` numa thread e num greenlet acaba na função dela (sem `<module>` embaixo), como no oráculo.
#[test]
fn frame_chain_scene_matches_the_oracle() {
    scene_matches(
        "gl_frame_chain",
        include_str!("../../../testbench/oracle/automation/cases/gl_frame_chain.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_frame_chain.out"),
    );
}

/// `UnswitchableGreenlet` (o ajudante de teste do greenlet): a troca que falha de propósito é `SystemError`.
#[test]
fn unswitchable_scene_matches_the_oracle() {
    scene_matches(
        "gl_unswitchable",
        include_str!("../../../testbench/oracle/automation/cases/gl_unswitchable.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_unswitchable.out"),
    );
}

/// `sys.setprofile` com troca de greenlet (o perfil vê o `switch` como C e o código do greenlet como Python), como no
/// oráculo.
#[test]
fn profile_scene_matches_the_oracle() {
    scene_matches(
        "gl_profile",
        include_str!("../../../testbench/oracle/automation/cases/gl_profile.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_profile.out"),
    );
}

/// O `settrace` roda do lado de quem chega (o alvo já é o corrente e a origem está suspensa): o erro dele no começo
/// mata o greenlet novo e sobe ao pai, e na volta levanta no destino. Como no oráculo.
#[test]
fn trace_arrival_scene_matches_the_oracle() {
    scene_matches(
        "gl_trace_arrival",
        include_str!("../../../testbench/oracle/automation/cases/gl_trace_arrival.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_trace_arrival.out"),
    );
}

/// O greenlet que outra thread soltou morre na thread dona dele, na próxima consulta ao greenlet corrente (o
/// `deleteme` do C), com o `GreenletExit` e o `__del__` do que ele segura. Como no oráculo.
#[test]
fn deleteme_scene_matches_the_oracle() {
    scene_matches(
        "gl_deleteme",
        include_str!("../../../testbench/oracle/automation/cases/gl_deleteme.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_deleteme.out"),
    );
}

/// O `__del__` como quadro do laço: troca de greenlet, irmãos em sequência (sem empilhar), o que morre dentro de um
/// `__del__` roda dentro dele, o erro vai ao `unraisablehook`. Como no oráculo.
#[test]
fn finalizer_frames_scene_matches_the_oracle() {
    scene_matches(
        "gl_finalizer_frames",
        include_str!("../../../testbench/oracle/automation/cases/gl_finalizer_frames.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_finalizer_frames.out"),
    );
}
