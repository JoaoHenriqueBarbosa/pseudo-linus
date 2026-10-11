//! Corredor do test262: um programa (o harness, os `includes` e o teste, montados pelo `scripts/test262-run.py`) roda num realm
//! novo, e o resultado sai como dado: a exceção que escapou (descrita como `Construtor: mensagem`, inclusive `Test262Error`, que
//! não herda de `Error`) e o que o teste imprimiu com `print` (o canal do `doneprintHandle.js` e do marcador de fase).
//!
//! `print` e o objeto `$262` (só `global`, `evalScript`, `gc`, `detachArrayBuffer`) são JavaScript do prelúdio. O que o motor ou
//! o prelúdio não têm (`createRealm`, `agent`, `IsHTMLDDA`) falta de verdade e o teste falha, que é a medida que se quer.

use crate::api::eval::{evaluate, named_program_source, new_global_object, program_source, read_global_result, VmFinalizer};
use crate::runtime::cell_registry::run_program;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::conversion_mode::ConversionMode;

/// O que um programa do test262 deixou.
pub struct Test262Outcome {
    /// A exceção que escapou, `Construtor: mensagem`; `None` quando o programa terminou.
    pub thrown: Option<String>,
    /// As linhas de `print`, uma por linha.
    pub output: String,
}

const PRELUDE: &str = "var __t262_out = [];
var print = function print() { __t262_out.push(Array.prototype.join.call(arguments, ' ')); };
var $262 = {
  global: globalThis,
  gc: function () {},
  evalScript: function (code) { return (0, eval)(String(code)); },
  detachArrayBuffer: function (buffer) { if (typeof buffer.transfer === 'function') buffer.transfer(); else throw new TypeError('detachArrayBuffer'); },
};";

const DESCRIBE: &str = "(function (e) {
  try {
    if (e === null || (typeof e !== 'object' && typeof e !== 'function')) return typeof e + ': ' + String(e);
    var c = e.constructor;
    return (c && c.name || 'object') + ': ' + String(e.message);
  } catch (x) { return 'unformattable: ' + typeof e; }
})";

fn text_of(value: JSValue) -> Option<String> {
    value.is_string().then(|| String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned())
}

fn describe(global_object: &JSGlobalObjectRef, exception: JSValue) -> String {
    let text = evaluate(global_object, &program_source(DESCRIBE)).ok().and_then(|function| {
        crate::runtime::call_data::call_with_error_message(global_object, function, JSValue::undefined(), &[exception], "descrição do erro").ok()
    });
    text.and_then(text_of).unwrap_or_else(|| {
        global_object.vm().clear_exception();
        crate::api::eval::describe_exception(&exception)
    })
}

fn output_of(global_object: &JSGlobalObjectRef) -> String {
    read_global_result(global_object, "__t262_out.join('\\n')").ok().and_then(text_of).unwrap_or_default()
}

/// Roda `source` como script global (o modo estrito vem da diretiva no começo do próprio texto), esvazia as microtarefas e o laço
/// de eventos virtual, e devolve a exceção e as linhas impressas.
pub fn run_script(source: &str, url: &str) -> Test262Outcome {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        let _ = evaluate(&global_object, &program_source(PRELUDE));
        let thrown = evaluate(&global_object, &named_program_source(source, url)).err().map(|exception| describe(&global_object, exception));
        vm.drain_microtasks();
        crate::runtime::timers::run_event_loop(&global_object);
        Test262Outcome { thrown, output: output_of(&global_object) }
    })
}

/// Roda o mapa de módulos `files` (chave, fonte) a partir de `entry`. O módulo de entrada começa por `print = L`, então as linhas
/// impressas são o `log` do corredor de módulos; `thrown` é o erro que rejeitou a avaliação (parser, ligação ou execução).
pub fn run_module(files: &[(String, String)], entry: &str) -> Test262Outcome {
    let outcome = crate::api::module::evaluate_module_map(files, entry);
    let lines: Vec<String> = log_lines(&outcome.log_json);
    Test262Outcome { thrown: outcome.error, output: lines.join("\n") }
}

/// O `log` (`["a","b"]`) em linhas, sem depender de uma crate de JSON: só strings com os escapes de `JSON.stringify`.
fn log_lines(json: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut chars = json.trim().trim_start_matches('[').trim_end_matches(']').chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut line = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => line.push('\n'),
                    Some('t') => line.push('\t'),
                    Some('r') => line.push('\r'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        line.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{fffd}'));
                    }
                    Some(other) => line.push(other),
                    None => {}
                },
                other => line.push(other),
            }
        }
        lines.push(line);
    }
    lines
}
