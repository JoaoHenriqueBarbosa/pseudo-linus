//! O History do perfil (`Default/History`, SQLite): o que o Chromium 154 grava numa navegação, medido no oráculo
//! (`testbench/oracle/measure-navigation.sh` com `ss-none.html`, `ss-two.html` e `about:blank`). O banco nasce do molde
//! com o esquema e sem linhas; a escrita é do SQLite de verdade (o mesmo do `sqlite3` do sandbox, pelo VFS próprio),
//! para que as páginas do arquivo saiam como as do Chromium.
//!
//! Uma cadeia de navegações da aba (a da linha de comando e as redirecionadas pela página com `location.href`) vira:
//! uma linha de `urls` por endereço (todas com o título da última página da cadeia), uma de `visits` por navegação
//! (a primeira digitada, as seguintes redirecionamento do cliente), o `segments`/`segment_usage` da primeira, a
//! `content_annotations` da última, uma `context_annotations` por visita e, nas redirecionadas, a `visited_links` que
//! guarda de onde o link foi seguido.

use rusqlite::{params, OpenFlags};

/// Microssegundos de 1601 (a época do tempo WebKit) até 1970.
const WEBKIT_EPOCH_US: i64 = 11_644_473_600_000_000;
const DAY_US: i64 = 86_400_000_000;

const TRANSITION_TYPED: i64 = 1;
const CHAIN_START: i64 = 0x1000_0000;
const CHAIN_END: i64 = 0x2000_0000;
const CLIENT_REDIRECT: i64 = 0x4000_0000;
/// O bit a mais que a navegação do `--dump-dom` carrega (as do DevTools não têm).
const DUMP_QUALIFIER: i64 = 0x0800_0000;
/// Os motivos de fim de página das `context_annotations`: a página deixada por uma navegação e a que o processo fechou.
const END_REASON_NAVIGATION: i64 = 3;
const END_REASON_CLOSE: i64 = 6;

/// Uma navegação concluída da aba.
#[derive(Debug, Clone)]
pub struct Navigation {
    pub url: String,
    pub title: String,
    /// Relógio de parede (microssegundos desde 1970) em que a navegação começou.
    pub start_us: i64,
}

/// O contexto do processo: de onde vêm os campos que não dependem da página.
#[derive(Debug, Clone, Copy)]
pub struct Process {
    /// O relógio de parede em que o navegador subiu e em que a saída começou.
    pub start_us: i64,
    pub exit_us: i64,
    /// O id da janela (a aba é o seguinte): aleatório por execução.
    pub window_id: i64,
    /// O `--dump-dom` marca a transição com um bit a mais.
    pub dump: bool,
}

/// `esquema://host/caminho`, sem porta, consulta nem fragmento: o nome do segmento.
fn segment_name(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return url.to_owned() };
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    format!("{scheme}://{host}/{path}")
}

/// A origem serializada do endereço, com a barra final (`http://127.0.0.1:8000/`).
fn origin_with_slash(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => format!("{scheme}://{}/", rest.split('/').next().unwrap_or_default()),
        None => url.to_owned(),
    }
}

/// Grava a cadeia `navigations` no History em `path`; o erro do SQLite vem como texto. O banco pode já ter linhas de
/// execuções anteriores: o endereço que já existe só ganha uma visita.
pub fn write(path: &str, navigations: &[Navigation], process: Process) -> Result<(), String> {
    if navigations.is_empty() {
        return Ok(());
    }
    let mut connection = ul_sqlite::cli::open_conn(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let sql = |e: rusqlite::Error| e.to_string();
    // O Chromium deixa o `History-journal` com zero byte depois de cada gravação.
    connection.query_row("PRAGMA journal_mode=TRUNCATE", [], |_| Ok(())).map_err(sql)?;
    let tx = connection.transaction().map_err(sql)?;
    let last = navigations.len() - 1;
    let final_title = &navigations[last].title;
    let mut previous_visit = 0;
    let mut last_visit = 0;
    for (index, nav) in navigations.iter().enumerate() {
        let typed = index == 0;
        let time = nav.start_us + WEBKIT_EPOCH_US;
        let end_us = navigations.get(index + 1).map_or(process.exit_us, |next| next.start_us);
        let duration = end_us - nav.start_us;
        let known: Option<i64> = tx.query_row("SELECT id FROM urls WHERE url = ?1", params![nav.url], |row| row.get(0)).ok();
        let url_id = match known {
            Some(id) => {
                tx.execute("UPDATE urls SET visit_count = visit_count + 1, typed_count = typed_count + ?1, last_visit_time = ?2 WHERE id = ?3", params![i64::from(typed), time, id]).map_err(sql)?;
                id
            }
            None => {
                tx.execute("INSERT INTO urls(url, title, visit_count, typed_count, last_visit_time, hidden) VALUES (?1, '', 1, ?2, ?3, 0)", params![nav.url, i64::from(typed), time]).map_err(sql)?;
                tx.last_insert_rowid()
            }
        };
        // Todos os endereços da cadeia ficam com o título da última página.
        tx.execute("UPDATE urls SET title = ?1 WHERE id = ?2", params![final_title, url_id]).map_err(sql)?;
        let mut transition = if typed { TRANSITION_TYPED | CHAIN_START | if process.dump { DUMP_QUALIFIER } else { 0 } } else { CLIENT_REDIRECT };
        if index == last {
            transition |= CHAIN_END;
        }
        // A navegação seguida por redirecionamento do cliente aponta a visita anterior e a `visited_links` dela.
        let link_id = if typed { 0 } else { visited_link(&tx, url_id, &navigations[index - 1].url).map_err(sql)? };
        let segment_id = if typed { segment(&tx, &nav.url, url_id, time).map_err(sql)? } else { 0 };
        tx.execute(
            "INSERT INTO visits VALUES (NULL, ?1, ?2, ?3, '', ?4, ?5, ?6, ?7, 0, '', 0, 0, 0, 0, 1, ?8, '')",
            params![url_id, time, previous_visit, transition, segment_id, if index < last || process.dump { duration } else { 0 }, i64::from(typed), link_id],
        )
        .map_err(sql)?;
        let visit_id = tx.last_insert_rowid();
        let foreground = if typed { end_us - process.start_us.min(nav.start_us) } else { duration + 15_000 };
        let (root, parent) = if typed { (time, -1) } else { (-1, time) };
        let end_reason = if index == last { END_REASON_CLOSE } else { END_REASON_NAVIGATION };
        tx.execute(
            "INSERT INTO context_annotations VALUES (?1, 0, -1000000, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, 200)",
            params![visit_id, end_reason, foreground, process.window_id, process.window_id + 1, time, root, parent],
        )
        .map_err(sql)?;
        previous_visit = visit_id;
        last_visit = visit_id;
    }
    tx.execute("INSERT INTO content_annotations VALUES (?1, -1, NULL, '', -1, 0, '', '', '', '', '', '', 1, 0)", params![last_visit]).map_err(sql)?;
    tx.commit().map_err(sql)
}

/// O segmento da página digitada (`segments`) e a contagem dele no dia (`segment_usage`); devolve o id do segmento.
fn segment(tx: &rusqlite::Transaction, url: &str, url_id: i64, time: i64) -> rusqlite::Result<i64> {
    let name = segment_name(url);
    let id = match tx.query_row("SELECT id FROM segments WHERE name = ?1", params![name], |row| row.get(0)) {
        Ok(id) => id,
        Err(_) => {
            tx.execute("INSERT INTO segments(name, url_id) VALUES (?1, ?2)", params![name, url_id])?;
            tx.last_insert_rowid()
        }
    };
    let slot = time.div_euclid(DAY_US) * DAY_US;
    let updated = tx.execute("UPDATE segment_usage SET visit_count = visit_count + 1 WHERE segment_id = ?1 AND time_slot = ?2", params![id, slot])?;
    if updated == 0 {
        tx.execute("INSERT INTO segment_usage(segment_id, time_slot, visit_count) VALUES (?1, ?2, 1)", params![id, slot])?;
    }
    Ok(id)
}

/// A linha de `visited_links` do endereço seguido a partir de `from`; devolve o id dela.
fn visited_link(tx: &rusqlite::Transaction, url_id: i64, from: &str) -> rusqlite::Result<i64> {
    let frame = origin_with_slash(from);
    match tx.query_row("SELECT id FROM visited_links WHERE link_url_id = ?1 AND top_level_url = ?2 AND frame_url = ?3", params![url_id, from, frame], |row| row.get(0)) {
        Ok(id) => {
            tx.execute("UPDATE visited_links SET visit_count = visit_count + 1 WHERE id = ?1", params![id])?;
            Ok(id)
        }
        Err(_) => {
            tx.execute("INSERT INTO visited_links(link_url_id, top_level_url, frame_url, visit_count) VALUES (?1, ?2, ?3, 1)", params![url_id, from, frame])?;
            Ok(tx.last_insert_rowid())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_names_drop_port_query_and_fragment() {
        assert_eq!(segment_name("http://127.0.0.1:8000/ss-none.html"), "http://127.0.0.1/ss-none.html");
        assert_eq!(segment_name("http://localhost:8000/a/b.html?q=1#f"), "http://localhost/a/b.html");
        assert_eq!(segment_name("https://example.com/"), "https://example.com/");
    }

    #[test]
    fn the_origin_keeps_the_port_and_gains_a_slash() {
        assert_eq!(origin_with_slash("http://127.0.0.1:8000/ss-two.html"), "http://127.0.0.1:8000/");
    }
}
