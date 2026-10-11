//! `switch` com casos numéricos esparsos: a faixa grande faz o gerador escolher a tabela em lista
//! (`SwitchType::ImmediateList`, pares chave/deslocamento), e o `op_switch_imm` precisa procurar a chave nela
//! (`UnlinkedSimpleJumpTable::offsetForValue` com `isList()`). Os valores esperados são os do bun 1.4.2 e do V8.
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como texto.
fn run(source: &str) -> String {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()
}

/// O `getBumpedLaneForHydrationByLane` do React 19.3 (a hidratação do nextjs.org parava nele).
#[test]
fn react_lane_switch_finds_every_case() {
    let source = "function e$(e){switch(e){case 2:e=1;break;case 8:e=4;break;case 32:e=16;break;case 256:case 512:case 1024:\
        case 2048:case 4096:case 8192:case 16384:case 32768:case 65536:case 131072:case 262144:case 524288:case 1048576:\
        case 2097152:case 4194304:case 8388608:case 0x1000000:case 0x2000000:e=128;break;case 0x10000000:e=0x8000000;break;\
        default:e=0}return e} [2,8,32,256,0x2000000,0x10000000,3].map(e$).join(' ')";
    assert_eq!(run(source), "1 4 16 128 128 134217728 0");
}

#[test]
fn sparse_switch_with_three_cases() {
    let source = "function t(x){switch(x){case 2:return 'a';case 8:return 'b';case 0x10000000:return 'c';default:return 'd'}} \
        [2,8,0x10000000,3,-2].map(t).join(' ')";
    assert_eq!(run(source), "a b c d d");
}

#[test]
fn sparse_switch_with_duplicate_key_takes_the_first() {
    let source = "function t(x){switch(x){case 5:return 'first';case 100000:return 'b';case 5:return 'second';case 9:return 'c';\
        default:return 'd'}} [5,100000,9,1].map(t).join(' ')";
    assert_eq!(run(source), "first b c d");
}

/// O `switch` sobre string feita por concatenação (rope) resolve a rope antes de procurar na tabela, como o
/// `slow_path_switch_string` do C++ (`value(globalObject)`); antes caía no `debug_assert` do `getValueImpl()`.
#[test]
fn string_switch_over_a_rope_resolves_it() {
    let source = "function f(s) { switch (s) { case 'ab': return 1; case 'cd': return 2; case 'ef': return 3; \
                  case 'gh': return 4; default: return 0; } } \
                  var x = 'a'; [f(x + String.fromCharCode(98)), f('c' + x.replace('a', 'd')), f(x + 'z')].join()";
    assert_eq!(run(source), "1,2,0");
}
