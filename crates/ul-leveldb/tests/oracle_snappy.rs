//! O compressor snappy contra os blocos que o leveldb do Chromium 154 do oráculo comprimiu: cada bloco de dados de uma
//! tabela dele é descomprimido pela nossa crate e comprimido de novo, e os bytes têm de ser os mesmos.

use ul_leveldb::snappy;
use ul_leveldb::table::Table;

fn blocks_of(dir: &str) -> Vec<(Vec<u8>, u8)> {
    let path = format!("{}/tests/fixtures/{dir}/000003.ldb", env!("CARGO_MANIFEST_DIR"));
    Table::open(std::fs::read(path).unwrap()).unwrap().stored_data_blocks()
}

/// Confere todos os blocos de uma tabela; devolve a descrição do primeiro que diverge.
fn first_divergence(dir: &str) -> Option<String> {
    for (index, (stored, block_type)) in blocks_of(dir).into_iter().enumerate() {
        let raw = if block_type == 1 { snappy::uncompress(&stored).unwrap() } else { stored.clone() };
        let ours = snappy::compress(&raw);
        let uses_snappy = ours.len() < raw.len() - raw.len() / 8;
        if (block_type == 1) != uses_snappy {
            return Some(format!("{dir} bloco {index}: o oráculo usa tipo {block_type}, nós comprimiríamos {} para {}", raw.len(), ours.len()));
        }
        if block_type == 1 && ours != stored {
            let at = ours.iter().zip(&stored).position(|(a, b)| a != b).unwrap_or(ours.len().min(stored.len()));
            return Some(format!(
                "{dir} bloco {index} (bruto {} bytes): oráculo {} bytes, nós {} bytes, primeira diferença no byte {at}: oráculo {:?}, nós {:?}",
                raw.len(),
                stored.len(),
                ours.len(),
                &stored[at.saturating_sub(4)..(at + 12).min(stored.len())],
                &ours[at.saturating_sub(4)..(at + 12).min(ours.len())]
            ));
        }
    }
    None
}

#[test]
fn blocks_of_large_values_compress_to_the_oracles_bytes() {
    assert_eq!(first_divergence("large/run2"), None);
}

#[test]
fn blocks_of_repetitive_data_compress_to_the_oracles_bytes() {
    assert_eq!(first_divergence("fill80/run2"), None);
}

#[test]
fn blocks_of_varied_data_compress_to_the_oracles_bytes() {
    assert_eq!(first_divergence("mixed/run2"), None);
}
