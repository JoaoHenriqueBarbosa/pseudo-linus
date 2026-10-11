//! O QuotaManager do perfil (`Default/WebStorage/QuotaManager`), medido no oráculo com `quota-*.html`: o bucket
//! `_default` de cada origem que usou o IndexedDB (qualquer chamada, até `deleteDatabase` e `databases`) e as duas
//! chaves de `meta` do bootstrap (sempre com um bucket; sem bucket, metade das execuções). O banco é SQLite de verdade, gravado pelo VFS do
//! sandbox; as execuções seguintes somam ao bucket que já existe (o id e a contagem de uso acumulam).
//!
//! Fora do medido: o IndexedDB em `iframe`. O oráculo (`quota-frame-*.html`) dá ao `iframe` do mesmo site o bucket da
//! própria origem (um só, `use_count` 2) e ao de outro site um bucket particionado, de chave
//! `http://localhost:8000/^0http://127.0.0.1` (a origem do quadro, `^0` e o site da página de topo, sem porta). O motor
//! daqui ainda não executa o script de um `iframe`, então nenhum desses buckets nasce: a divergência é a do `iframe`
//! inteiro, não só do QuotaManager.

use rusqlite::{params, OpenFlags};

use crate::idb::disk::BucketUse;

/// A chave de armazenamento de uma origem: a origem serializada com a barra final.
fn storage_key(origin: &str) -> String {
    format!("{origin}/")
}

/// O host da origem, sem esquema nem porta.
fn host(origin: &str) -> &str {
    let rest = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    match rest.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => rest,
    }
}

/// Grava os buckets de `uses` no banco em `path`, e as chaves do bootstrap se `bootstrapped`. Toda execução passa por
/// uma transação (o Chromium deixa o `QuotaManager-journal` com zero byte). O erro do SQLite vem como texto.
pub fn write(path: &str, uses: &[(String, BucketUse)], bootstrapped: bool) -> Result<(), String> {
    let mut connection = ul_sqlite::cli::open_conn(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let sql = |e: rusqlite::Error| e.to_string();
    connection.query_row("PRAGMA journal_mode=TRUNCATE", [], |_| Ok(())).map_err(sql)?;
    let tx = connection.transaction().map_err(sql)?;
    tx.execute("UPDATE meta SET value = value WHERE key = 'version'", []).map_err(sql)?;
    if bootstrapped {
        for key in ["IsBucketsBootstrapped", "IsMediaLicenseDatabaseRemoved"] {
            tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, '1')", params![key]).map_err(sql)?;
        }
    }
    for (origin, usage) in uses {
        let key = storage_key(origin);
        let known = tx
            .execute(
                "UPDATE buckets SET use_count = use_count + ?1, last_accessed = ?2, last_modified = ?3 WHERE storage_key = ?4 AND name = '_default'",
                params![usage.use_count, usage.accessed, usage.modified, key],
            )
            .map_err(sql)?;
        if known == 0 {
            tx.execute(
                "INSERT INTO buckets(storage_key, host, name, use_count, last_accessed, last_modified, expiration, quota, persistent, durability) VALUES (?1, ?2, '_default', ?3, ?4, ?5, 0, 0, 0, 0)",
                params![key, host(origin), usage.use_count, usage.accessed, usage.modified],
            )
            .map_err(sql)?;
        }
    }
    tx.commit().map_err(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_drops_scheme_and_port() {
        assert_eq!(host("http://127.0.0.1:8000"), "127.0.0.1");
        assert_eq!(host("https://example.com"), "example.com");
    }

    #[test]
    fn storage_key_has_the_trailing_slash() {
        assert_eq!(storage_key("http://127.0.0.1:8000"), "http://127.0.0.1:8000/");
    }
}
