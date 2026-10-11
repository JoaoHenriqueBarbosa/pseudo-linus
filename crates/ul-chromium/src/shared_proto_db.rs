//! O `shared_proto_db` do perfil (`Default/shared_proto_db` e `Default/shared_proto_db/metadata`), os dois leveldb dos
//! protos compartilhados do Chromium (segmentation platform, feature engagement e afins). Medido no oráculo (nove
//! execuções seguidas do `--dump-dom` no mesmo perfil, mais 60 de outras páginas e modos):
//!
//! - a primeira execução cria os dois bancos e grava um roteiro fixo: no principal, três lotes vazios, onze registros
//!   `37_DEFAULT_<n>` e oito gravações progressivas de `39_config`; no `metadata`, dois `__global` e duas voltas de
//!   onze chaves de loja (`50_`, `32_`, `44_` ...), a segunda com o mesmo estado `\x18\x01` no lugar do `\x18\x00`;
//! - cada execução seguinte reabre os bancos (o log e o MANIFEST são reaproveitados, o `LOG` anterior vira `LOG.old`) e
//!   repete só o começo do roteiro: no principal os três lotes vazios e os onze `37_DEFAULT_<n>` (4058 bytes a mais no
//!   log), no `metadata` um `__global` (34 bytes a mais);
//! - o roteiro é igual em todas as execuções, menos o instante em segundos WebKit que fecha cada valor de proto (a
//!   mesma conta em toda a execução) e a ordem das onze chaves de loja no `metadata`, que mudou em todas as 68
//!   amostras do oráculo (o `50_` vem sempre primeiro; a segunda volta é a primeira com poucas trocas).
//!
//! O roteiro vem dos logs da primeira execução do oráculo (`profile/shared-proto-db-*.log`), lidos com o leitor de log do
//! `ul-leveldb`; quem grava é o leveldb próprio, então o formato do log, as sequências, os CRCs, o `LOG` e o
//! `LOG.old` saem do mesmo código dos outros bancos.

use std::sync::Arc;

use ul_leveldb::batch::{Op, WriteBatch};
use ul_leveldb::env::Env;
use ul_leveldb::log::read_records;
use ul_leveldb::{Db, Options, Result};

/// Os logs da primeira execução do oráculo: o roteiro de cada banco.
const MAIN_SCRIPT: &[u8] = include_bytes!("../profile/shared-proto-db-main.log");
const METADATA_SCRIPT: &[u8] = include_bytes!("../profile/shared-proto-db-metadata.log");
/// O prefixo dos registros que toda execução repete no banco principal, e a chave que só a primeira grava.
const REPEATED_PREFIX: &[u8] = b"37_DEFAULT_";
const FIRST_RUN_MARK: &[u8] = b"37_DEFAULT_16";
/// A chave do `metadata` que cada execução grava, e a que marca o banco novo.
const GLOBAL_KEY: &[u8] = b"__global";
/// A loja que vem sempre primeiro nas duas voltas.
const FIRST_STORE: &[u8] = b"50_";
/// Quantas chaves de loja cada volta do `metadata` tem; a primeira (`50_`) fica no lugar.
const STORES: usize = 11;

/// O roteiro de um banco: os lotes, cada um com as suas operações.
fn script(log: &[u8]) -> Result<Vec<Vec<Op>>> {
    read_records(log)?.iter().map(|record| WriteBatch::from_contents(record)?.ops()).collect()
}

/// O varint de um número (o instante em segundos que fecha os valores).
fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while value >= 0x80 {
        out.push((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
    out
}

/// O instante do roteiro gravado: o varint de cinco bytes que fecha o primeiro registro `37_DEFAULT_<n>`.
fn recorded_time(batches: &[Vec<Op>]) -> Option<Vec<u8>> {
    let value = batches.iter().flatten().find_map(|op| match op {
        Op::Put { key, value } if key.starts_with(REPEATED_PREFIX) => Some(value),
        _ => None,
    })?;
    value.len().checked_sub(7).map(|at| value[at..at + 5].to_vec())
}

/// O lote com os valores do roteiro, o instante gravado trocado pelo desta execução.
fn rebuilt(ops: &[Op], recorded: &[u8], now: &[u8]) -> WriteBatch {
    let mut batch = WriteBatch::new();
    for op in ops {
        match op {
            Op::Put { key, value } => {
                let mut value = value.clone();
                let mut at = 0;
                while at + recorded.len() <= value.len() {
                    if value[at..at + recorded.len()] == *recorded {
                        value.splice(at..at + recorded.len(), now.iter().copied());
                        at += now.len();
                    } else {
                        at += 1;
                    }
                }
                batch.put(key, &value);
            }
            Op::Delete { key } => batch.delete(key),
        }
    }
    batch
}

/// A ordem das chaves de loja do `metadata` nesta execução: a primeira volta embaralhada (o `50_` fica primeiro) e a
/// segunda com duas ou três trocas.
fn store_orders(random: &mut dyn FnMut(u64) -> u64) -> (Vec<usize>, Vec<usize>) {
    let mut first: Vec<usize> = (0..STORES).collect();
    for i in (2..STORES).rev() {
        first.swap(i, 1 + random(i as u64) as usize);
    }
    let mut second = first.clone();
    for _ in 0..2 + random(2) {
        let (a, b) = (1 + random(STORES as u64 - 1) as usize, 1 + random(STORES as u64 - 1) as usize);
        second.swap(a, b);
    }
    (first, second)
}

/// O `metadata` de uma primeira execução: o roteiro com as duas voltas de chaves de loja na ordem sorteada.
fn metadata_first_run(script: &[Vec<Op>], random: &mut dyn FnMut(u64) -> u64) -> Vec<Vec<Op>> {
    let (first, second) = store_orders(random);
    let mut out: Vec<Vec<Op>> = script[..2].to_vec();
    for (lap, order) in [(&script[2..2 + STORES], first), (&script[2 + STORES..2 + 2 * STORES], second)] {
        // A volta guarda as lojas na ordem do oráculo; o sorteio dá a ordem pelo índice delas.
        let mut by_key: Vec<&Vec<Op>> = lap.iter().collect();
        by_key.sort_by(|a, b| (key_of(a) != FIRST_STORE, key_of(a)).cmp(&(key_of(b) != FIRST_STORE, key_of(b))));
        out.extend(order.into_iter().map(|index| by_key[index].clone()));
    }
    out.extend_from_slice(&script[2 + 2 * STORES..]);
    out
}

fn key_of(batch: &[Op]) -> &[u8] {
    match &batch[0] {
        Op::Put { key, .. } | Op::Delete { key } => key,
    }
}

/// Grava no `shared_proto_db` desta execução do `--dump-dom`: `dir` é o do banco principal, `now_secs` o instante em
/// segundos WebKit e `random` o sorteio (a ordem das lojas do `metadata`).
pub fn run(env: Arc<dyn Env>, dir: &str, thread_id: u64, now_secs: u64, random: &mut dyn FnMut(u64) -> u64) -> Result<()> {
    let main = script(MAIN_SCRIPT)?;
    let metadata = script(METADATA_SCRIPT)?;
    let recorded = recorded_time(&main).unwrap_or_default();
    let now = varint(now_secs);

    let mut db = Db::open(env.clone(), dir, Options::chromium(thread_id))?;
    let repeated = main.iter().rposition(|batch| batch.iter().any(|op| matches!(op, Op::Put { key, .. } if key.starts_with(REPEATED_PREFIX)))).map_or(0, |last| last + 1);
    let fresh = db.get(FIRST_RUN_MARK)?.is_none();
    for ops in if fresh { &main[..] } else { &main[..repeated] } {
        db.write(rebuilt(ops, &recorded, &now))?;
    }
    drop(db);

    let mut meta = Db::open(env, &format!("{dir}/metadata"), Options::chromium(thread_id))?;
    if meta.get(GLOBAL_KEY)?.is_none() {
        for ops in metadata_first_run(&metadata, random) {
            meta.write(rebuilt(&ops, &recorded, &now))?;
        }
    } else {
        meta.write(rebuilt(&metadata[0], &recorded, &now))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_scripts_have_the_measured_shape() {
        let main = script(MAIN_SCRIPT).unwrap();
        // Três lotes vazios, onze `37_DEFAULT_<n>` e oito gravações progressivas de `39_config`.
        assert_eq!(main.iter().filter(|batch| batch.is_empty()).count(), 3);
        assert_eq!(main.len(), 22);
        assert_eq!(recorded_time(&main).map(|time| time.len()), Some(5));
        let metadata = script(METADATA_SCRIPT).unwrap();
        assert_eq!(metadata.len(), 2 + 2 * STORES);
        assert_eq!(key_of(&metadata[0]), GLOBAL_KEY);
    }

    #[test]
    fn varint_matches_the_recorded_time() {
        // 13436171567 s, o instante do `persist-dump` do oráculo: afeaee8632.
        assert_eq!(varint(13_436_171_567), [0xaf, 0xea, 0xee, 0x86, 0x32]);
    }

    #[test]
    fn rebuilt_swaps_only_the_recorded_time() {
        let ops = vec![Op::Put { key: b"k".to_vec(), value: [b"ab".as_slice(), &[0xaf, 0xea, 0xee, 0x86, 0x32], b"cd", &[0xaf, 0xea, 0xee, 0x86, 0x32]].concat() }];
        let batch = rebuilt(&ops, &[0xaf, 0xea, 0xee, 0x86, 0x32], &[1, 2, 3, 4, 5]);
        let Op::Put { value, .. } = &batch.ops().unwrap()[0] else { panic!() };
        assert_eq!(value, &[b"ab".as_slice(), &[1, 2, 3, 4, 5], b"cd", &[1, 2, 3, 4, 5]].concat());
    }

    #[test]
    fn store_orders_keep_the_first_store_and_stay_a_permutation() {
        let mut state = 7u64;
        let mut random = |bound: u64| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) % bound.max(1)
        };
        for _ in 0..50 {
            let (first, second) = store_orders(&mut random);
            for order in [first, second] {
                assert_eq!(order[0], 0);
                let mut sorted = order.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, (0..STORES).collect::<Vec<_>>());
            }
        }
    }
}
