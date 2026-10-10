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
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let r = exec(&c, &sb, "mkdir -p /tmp/gl/greenlet && : > /tmp/gl/greenlet/_greenlet.cpython-313-x86_64-linux-gnu.so");
    assert_eq!(r["exit_code"], 0, "{r}");
    write(&c, &sb, "/tmp/gl/greenlet/__init__.py", INIT);
    write(&c, &sb, "/tmp/gl/common.py", COMMON);
    write(&c, &sb, &format!("/tmp/gl/{name}.py"), scene);
    let r = exec(&c, &sb, &format!("cd /tmp/gl && python3 {name}.py"));
    assert_eq!(r["stderr"], "", "{r}");
    assert_eq!(r["exit_code"], 0, "{r}");
    let got = r["stdout"].as_str().unwrap();
    for (n, (want, have)) in golden.lines().zip(got.lines()).enumerate() {
        assert_eq!(have, want, "linha {} da cena {name}; saída inteira:\n{got}", n + 1);
    }
    assert_eq!(got, golden, "cena {name}");
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
