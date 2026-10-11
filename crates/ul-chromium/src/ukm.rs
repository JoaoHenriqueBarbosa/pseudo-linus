//! O UKM do perfil (`segmentation_platform/ukm_db`, SQLite em modo WAL), medido no oráculo na saída do `--dump-dom`
//! (a partir de 44 execuções de `ss-none`, `ss-edge`, `persist` e das páginas `quota-*`, e de 10 execuções seguidas no
//! mesmo perfil):
//!
//! - `urls`: uma linha por endereço visitado (os da cadeia de redirecionamento incluídos), com o `url_id` (os oito
//!   primeiros bytes do MD5 do endereço, little endian, com sinal), `last_timestamp` de 4 a 37 ms depois da visita
//!   (cada endereço seguinte da cadeia, de 0,05 a 1,7 ms depois do anterior), `counter` e `title` nulos e o `profile_id`
//!   `1673893443`, sempre o mesmo; o endereço que já existe só ganha o `last_timestamp` novo;
//! - `metrics`: uma linha por execução, gravada na saída (evento `AC722301695B9315`, métrica `64BD7CCE5A95BF00`, fonte
//!   81 no `--dump-dom`, o `url_id` do endereço final, id sorteado), de 3 a 11 ms depois do fim da visita. Só sai
//!   se a página viveu o bastante: nenhuma abaixo de 46 ms, todas acima de 90 ms (de 60 execuções) e cerca de 4 de 11
//!   entre as duas medidas. O `metric_value` (de 22 a 229 nas amostras) é sorteado na faixa medida: a relação dele com a
//!   página, que o oráculo tem, não foi modelada;
//! - o arquivo fica com 49152 bytes e o WAL é apagado na saída.
//!
//! No `Browser.close` do navegador de pé (`--remote-debugging-port`) o banco sai igual (49152 bytes, sem WAL), com a
//! fonte 49 e sempre um evento, de 3,4 a 4,1 ms antes do instante da saída (sete de sete execuções, com página; sem
//! página não há linha nenhuma). Fora do medido: o SIGTERM do navegador de pé, que deixa o banco de 4096 bytes e o WAL
//! de 90 KB, e as execuções que dão mais de um evento.

use rusqlite::{params, OpenFlags};

/// O `profile_id` de todas as linhas de `urls` (igual em todas as amostras, de execuções e perfis diferentes).
const PROFILE_ID: &str = "1673893443";
const EVENT_HASH: &str = "AC722301695B9315";
const METRIC_HASH: &str = "64BD7CCE5A95BF00";
/// A fonte do evento na saída do `--dump-dom`.
pub const DUMP_SOURCE: i64 = 81;
/// A fonte do evento na saída do `Browser.close` do navegador de pé.
pub const CLOSE_SOURCE: i64 = 49;

/// Os atrasos (µs) do `last_timestamp` depois da visita, nas 44 execuções do `--dump-dom` do oráculo com um endereço só.
const URL_DELAYS_US: [i64; 44] = [
    4068, 4218, 5111, 5115, 5118, 5176, 5242, 5257, 5583, 5604, 5657, 5696, 5698, 5776, 5786, 5998, 6186, 6306, 6383, 6550, 6610, 6909, 7369, 7493, 7585, 7597, 7845, 8202, 8593, 8711, 10684, 10970, 12479, 14006, 16325, 17604, 19891,
    22427, 22671, 22712, 25850, 27822, 28805, 30073,
];

/// Quanto a página viveu (µs) abaixo do qual nunca há evento e acima do qual sempre há.
const NEVER_BELOW_US: i64 = 46_000;
const ALWAYS_ABOVE_US: i64 = 90_000;

/// O `url_id` de um endereço: os oito primeiros bytes do MD5, little endian, como inteiro com sinal.
pub fn url_id(url: &str) -> i64 {
    let digest = ul_common::hash::md5(url.as_bytes());
    i64::from_le_bytes(digest[..8].try_into().expect("oito bytes"))
}

/// O atraso do `last_timestamp` de uma visita, sorteado das execuções medidas (`draw` é qualquer inteiro).
pub fn url_delay_us(draw: u64) -> i64 {
    URL_DELAYS_US[(draw % URL_DELAYS_US.len() as u64) as usize]
}

/// Se a execução grava o evento da saída: pelo tempo que a página viveu, com `draw` (0 a 10) decidindo entre as duas
/// medidas (4 de 11 deram evento).
pub fn has_event(lived_us: i64, draw: u64) -> bool {
    lived_us >= ALWAYS_ABOVE_US || (lived_us >= NEVER_BELOW_US && draw < 4)
}

/// O evento da saída.
#[derive(Debug, Clone, Copy)]
pub struct Event {
    /// Relógio WebKit (µs).
    pub timestamp: i64,
    pub source: i64,
    pub id: i64,
    pub value: i64,
}

/// Grava no banco em `path` as visitas (`url`, instante WebKit em µs, na ordem) e o `event`, se houver, ligado ao
/// último endereço. O erro do SQLite vem como texto.
pub fn write(path: &str, visits: &[(String, i64)], event: Option<Event>) -> Result<(), String> {
    let mut connection = ul_sqlite::cli::open_conn(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let sql = |e: rusqlite::Error| e.to_string();
    // O VFS do sandbox não tem memória compartilhada: o banco em WAL só abre em modo exclusivo. O Chromium sai com o WAL
    // já passado para o arquivo e apagado: o checkpoint aqui faz o mesmo e o arquivo do WAL sai depois de fechar.
    connection.query_row("PRAGMA locking_mode=EXCLUSIVE", [], |_| Ok(())).map_err(sql)?;
    let tx = connection.transaction().map_err(sql)?;
    for (url, time) in visits {
        let id = url_id(url);
        let known = tx.execute("UPDATE urls SET last_timestamp = ?1 WHERE url_id = ?2", params![time, id]).map_err(sql)?;
        if known == 0 {
            tx.execute("INSERT INTO urls(url_id, url, last_timestamp, counter, title, profile_id) VALUES (?1, ?2, ?3, NULL, NULL, ?4)", params![id, url, time, PROFILE_ID]).map_err(sql)?;
        }
    }
    if let (Some(event), Some((last, _))) = (event, visits.last()) {
        tx.execute(
            "INSERT INTO metrics(event_timestamp, ukm_source_id, url_id, event_id, event_hash, metric_hash, metric_value) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event.timestamp, event.source, url_id(last), event.id, EVENT_HASH, METRIC_HASH, event.value],
        )
        .map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(sql)?;
    drop(connection);
    for suffix in ["-wal", "-shm"] {
        let _ = sysabi::sys::current().unlinkat(sysabi::Fd::CWD, format!("{path}{suffix}").as_bytes(), sysabi::AtFlags::empty());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os `url_id` medidos no oráculo (`ss-none.html`, `redir` e `redir/`).
    #[test]
    fn url_id_is_the_first_eight_bytes_of_the_md5() {
        assert_eq!(url_id("http://127.0.0.1:8000/ss-none.html"), 2_988_681_432_195_754_744);
        assert_eq!(url_id("http://127.0.0.1:8000/redir"), 8_262_361_356_634_260_248);
        assert_eq!(url_id("http://127.0.0.1:8000/redir/"), -8_650_618_196_860_082_061);
    }

    #[test]
    fn the_event_follows_how_long_the_page_lived() {
        assert!(!has_event(30_000, 0));
        assert!(has_event(50_000, 3) && !has_event(50_000, 4));
        assert!(has_event(95_000, 10));
    }
}
