//! Golden de quando um `MessagePort` segura o laço de eventos, contra o bun 1.4.2: `tests/golden/message_channel_loop_bun.tsv`
//! sai de `scripts/gen-message-channel-loop-golden.js`. Cada linha é um programa principal sem timer que feche as portas;
//! o resultado é a saída, o código e se o processo estava vivo ao estourar o limite de tempo (laço segurado). O lado Rust
//! observa o mesmo com `evaluate_main_script_reporting_hold`, que devolve em vez de parar para sempre
//! (ver `src/runtime/message_channel.rs`, `holds_event_loop`).
mod common;

const GOLDEN: &str = include_str!("golden/message_channel_loop_bun.tsv");

#[test]
fn message_channel_loop_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let failures: Vec<String> = GOLDEN.lines().filter(|line| !line.is_empty()).flat_map(|line| common::MainScriptRow::parse(line).check("message-channel-loop")).collect();
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}
