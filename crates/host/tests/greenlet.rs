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

/// A finalização do lixo cíclico: o `__del__` uma vez e em ordem de criação, a ressurreição, as referências fracas
/// (limpas antes, função de retorno antes do `__del__`), `DEBUG_SAVEALL` com `gc.garbage`, `gc.callbacks`, o erro do
/// `__del__` e o `__del__` que aloca. Como no oráculo.
#[test]
fn gc_finalize_scene_matches_the_oracle() {
    scene_matches(
        "gc_finalize",
        include_str!("../../../testbench/oracle/automation/cases/gc_finalize.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_finalize.out"),
    );
}

/// A coleta de ciclos: o que `gc.collect` devolve para cada forma de lixo (pares de instâncias, closures, o `__dict__`
/// materializado), `get_stats`, o `disable`, as gerações, os limiares e a memória de um loop de ciclos (a coleta
/// automática mantém o heap limitado). Como no oráculo.
#[test]
fn gc_collect_scene_matches_the_oracle() {
    scene_matches(
        "gc_collect",
        include_str!("../../../testbench/oracle/automation/cases/gc_collect.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_collect.out"),
    );
}

/// O registro do coletor: `gc.is_tracked` por tipo (o dict e a tupla pelo conteúdo), `gc.get_objects` com os contêineres
/// criados, `gc.get_referrers`, `gc.freeze/unfreeze` e o erro da geração fora da faixa. Como no oráculo.
#[test]
fn gc_registry_scene_matches_the_oracle() {
    scene_matches(
        "gc_registry",
        include_str!("../../../testbench/oracle/automation/cases/gc_registry.py"),
        include_str!("../../../testbench/oracle/automation/golden/gc_registry.out"),
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

/// As divergências da suíte `greenlet/tests` do 3.5.6 que ainda existem, cada uma com a causa. A suíte inteira roda no
/// `python3` da imagem (`unittest discover` do pacote) e o conjunto de testes que falham tem de ser exatamente este:
/// falha nova é regressão, e item daqui que passar sai da lista.
const DECLARED_DIVERGENCES: &[(&str, &str)] = &[
    ("greenlet.tests.test_cpp (_FailedTest)", "extensão C `_test_extension` (.so) da suíte, que o sandbox não carrega: no oráculo ela existe (divergência declarada, item 6)"),
    ("greenlet.tests.test_extension_interface (_FailedTest)", "extensão C `_test_extension` (.so) da suíte, que o sandbox não carrega: no oráculo ela existe (divergência declarada, item 6)"),
    ("test_api_getcurrent_no_system_error_at_module_gc_time (test_interpreter_shutdown)", "extensão C `_test_extension` (.so) da suíte, que o sandbox não carrega: no oráculo ela existe (divergência declarada, item 6)"),
    ("test_can_access_f_back_of_suspended_greenlet (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_finalizer_crash (test_gc)", "o teste espera um vazamento (`expect_greenlet_leak`) que o CPython tem e o plinus não: aqui o greenlet morre ao soltar a última referência (divergência declarada, item 3)"),
    ("test_frame (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_frames_always_exposed (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_get_stack_with_nested_c_calls (test_greenlet)", "extensão C `_test_extension` (.so) da suíte, que o sandbox não carrega: no oráculo ela existe (divergência declarada, item 6)"),
    ("test_issue251_issue252_explicit_reference_not_collectable (test_leaks)", "o teste espera um vazamento (`expect_greenlet_leak`) que o CPython tem e o plinus não: aqui o greenlet morre ao soltar a última referência (divergência declarada, item 3)"),
    ("test_issue251_issue252_need_to_collect_in_background (test_leaks)", "o teste espera um vazamento (`expect_greenlet_leak`) que o CPython tem e o plinus não: aqui o greenlet morre ao soltar a última referência (divergência declarada, item 3)"),
    ("test_issue251_issue252_need_to_collect_in_background_cleanup_disabled (test_leaks)", "o teste espera um vazamento (`expect_greenlet_leak`) que o CPython tem e o plinus não: aqui o greenlet morre ao soltar a última referência (divergência declarada, item 3)"),
    ("test_issue251_killing_cross_thread_leaks_list (test_leaks)", "o plinus não acha ciclos de instâncias (sem coletor: `gc.collect`, `__del__` e `weakref` de ciclo); fica para o projeto do coletor de ciclos (item 1)"),
    ("test_parent_restored_on_kill (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_switch_no_error_at_module_gc_time (test_interpreter_shutdown)", "extensão C `_test_extension` (.so) da suíte, que o sandbox não carrega: no oráculo ela existe (divergência declarada, item 6)"),
    ("test_switching_holding_critical_section_no_crash (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_threaded_adv_leak (test_leaks)", "o plinus não acha ciclos de instâncias (sem coletor: `gc.collect`, `__del__` e `weakref` de ciclo); fica para o projeto do coletor de ciclos (item 1)"),
    ("test_threaded_updatecurrent (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_throw_to_dead_thread_doesnt_crash (test_greenlet)", "o teste espera um vazamento (`expect_greenlet_leak`) que o CPython tem e o plinus não: aqui o greenlet morre ao soltar a última referência (divergência declarada, item 3)"),
    ("test_trace_events_multiple_greenlets_switching (test_tracing)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_trace_events_multiple_greenlets_switching_siblings (test_tracing)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
    ("test_tuple_subclass (test_greenlet)", "o leakcheck vê objetos que o coletor ainda não alcança (quadros de greenlet, exceções com traceback, geradores seguram escopos fora do registro); some com a fatia de quadros e exceções do coletor"),
];

/// A suíte `greenlet/tests` do 3.5.6 (a da wheel, copiada do oráculo com o `psutil` mínimo que ela importa) roda
/// no `python3` da imagem como no CPython: `unittest discover` do pacote. Os testes que falham são os declarados em
/// `DECLARED_DIVERGENCES`, nem mais nem menos.
#[test]
fn greenlet_test_suite_fails_only_where_declared() {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let suite = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testbench/oracle/automation/fixtures/greenlet_suite");
    write_tree(&c, &sb, &suite, "/tmp/gls");
    let r = exec(&c, &sb, "touch /tmp/gls/greenlet/_greenlet.cpython-313-x86_64-linux-gnu.so");
    assert_eq!(r["exit_code"], 0, "{r}");
    write(&c, &sb, "/tmp/gls/greenlet/__init__.py", INIT);
    let command = "cd /tmp/gls && PYTHONPATH=/tmp/gls python3 -m unittest discover -s greenlet/tests -t . > out.txt 2>&1; cat out.txt";
    let r = c.call("exec", json!({ "sandbox_id": sb, "command": command, "timeout_ms": 900_000 })).unwrap();
    let out = r["stdout"].as_str().unwrap();
    assert!(out.contains("Ran 156 tests"), "{out} {r}");
    let mut failed: Vec<String> = out
        .lines()
        .filter_map(|l| l.strip_prefix("FAIL: ").or_else(|| l.strip_prefix("ERROR: ")))
        .map(|l| l.split(" (").next().unwrap().to_string() + " (" + l.split(" (").nth(1).map_or("", |x| x.split('.').nth(2).unwrap_or("")) + ")")
        .collect();
    failed.sort();
    failed.dedup();
    let mut declared: Vec<String> = DECLARED_DIVERGENCES.iter().map(|(n, _)| n.to_string()).collect();
    declared.sort();
    assert_eq!(failed, declared, "{out}");
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

/// O principal de uma thread que já acabou é dado por morto (`dead`, `bool` falso, `(thread exited) dead` no `repr`) e
/// não recebe troca. Como no oráculo.
#[test]
fn main_gone_scene_matches_the_oracle() {
    scene_matches(
        "gl_main_gone",
        include_str!("../../../testbench/oracle/automation/cases/gl_main_gone.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_main_gone.out"),
    );
}

/// O pai que ainda não começou e não tem `run` (o `__getattribute__` levanta): o erro sobe ao avô e o pai segue
/// pendente, então cada troca para o filho morto levanta de novo. Como no oráculo.
#[test]
fn perverse_parent_scene_matches_the_oracle() {
    scene_matches(
        "gl_perverse_parent",
        include_str!("../../../testbench/oracle/automation/cases/gl_perverse_parent.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_perverse_parent.out"),
    );
}

/// O `repr` do greenlet que ainda não começou tem `(otid=0x(nil))`, como no oráculo.
#[test]
fn repr_pending_scene_matches_the_oracle() {
    scene_matches(
        "gl_repr_pending",
        include_str!("../../../testbench/oracle/automation/cases/gl_repr_pending.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_repr_pending.out"),
    );
}

/// `Context.run` guarda o contexto anterior como ele está (sem criar um): o greenlet que roda dentro de `run` sem ter
/// contexto acaba com `gr_context` `None`. Como no oráculo.
#[test]
fn context_run_scene_matches_the_oracle() {
    scene_matches(
        "gl_context_run",
        include_str!("../../../testbench/oracle/automation/cases/gl_context_run.py"),
        include_str!("../../../testbench/oracle/automation/golden/gl_context_run.out"),
    );
}

/// Criar, trocar e retomar milhares de greenlets suspensos custa o mesmo por greenlet (a troca não varre os suspensos):
/// 2000 e 20000 greenlets cabem em tempo limitado, e todos terminam no retorno.
#[test]
fn switching_many_greenlets_scales_linearly() {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let r = exec(&c, &sb, "mkdir -p /tmp/gl/greenlet && : > /tmp/gl/greenlet/_greenlet.cpython-313-x86_64-linux-gnu.so");
    assert_eq!(r["exit_code"], 0, "{r}");
    write(&c, &sb, "/tmp/gl/greenlet/__init__.py", INIT);
    let script = "\
import time, greenlet
def f():
    greenlet.getcurrent().parent.switch()
    return 1
for n in (2000, 20000):
    t0 = time.time()
    gl = []
    for _ in range(n):
        g = greenlet.greenlet(f)
        gl.append(g)
        g.switch()
    for g in gl:
        g.switch()
    assert all(g.dead for g in gl)
    print(n, round(time.time() - t0, 2))
";
    write(&c, &sb, "/tmp/gl/scale.py", script);
    let command = "cd /tmp/gl && python3 scale.py";
    let r = c.call("exec", json!({ "sandbox_id": sb, "command": command, "timeout_ms": 300_000 })).unwrap();
    assert_eq!(r["exit_code"], 0, "{r}");
    let times: Vec<f64> = r["stdout"].as_str().unwrap().lines().map(|l| l.split(' ').nth(1).unwrap().parse().unwrap()).collect();
    assert!(times[0] < 5.0 && times[1] < 60.0, "{times:?}");
}
