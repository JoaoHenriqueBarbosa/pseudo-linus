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
use ul_common::time::WEBKIT_EPOCH_US;

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

/// Como o History chega ao disco (medido no oráculo): a saída do `--dump-dom`, o `Browser.close` do DevTools (a mesma
/// linha, sem o bit a mais da transição) ou o commit de 10 segundos do navegador de pé, que grava a visita ainda aberta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Dump,
    Close,
    Live,
}

/// O contexto do processo: de onde vêm os campos que não dependem da página.
#[derive(Debug, Clone, Copy)]
pub struct Process {
    /// O relógio de parede em que o navegador subiu e em que a saída começou.
    pub start_us: i64,
    pub exit_us: i64,
    /// O id da janela (a aba é o seguinte): aleatório por execução.
    pub window_id: i64,
    pub ending: Ending,
}

/// O commit do navegador de pé: 10 segundos depois da primeira navegação (`kCommitIntervalSeconds`; o oráculo tem a
/// visita no banco com o SIGTERM aos 11 s e não aos 10 s).
const LIVE_COMMIT_US: i64 = 10_000_000;

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
pub fn origin_with_slash(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => format!("{scheme}://{}/", rest.split('/').next().unwrap_or_default()),
        None => url.to_owned(),
    }
}

/// Grava a cadeia `navigations` no History em `path`; o erro do SQLite vem como texto e o resultado é o id da última
/// visita (0 sem navegações). O banco pode já ter linhas de execuções anteriores: o endereço que já existe só ganha uma
/// visita.
pub fn write(path: &str, navigations: &[Navigation], process: Process) -> Result<i64, String> {
    if navigations.is_empty() {
        return Ok(0);
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
        let mut transition = if typed { TRANSITION_TYPED | CHAIN_START | if process.ending == Ending::Dump { DUMP_QUALIFIER } else { 0 } } else { CLIENT_REDIRECT };
        if index == last {
            transition |= CHAIN_END;
        }
        // A navegação seguida por redirecionamento do cliente aponta a visita anterior e a `visited_links` dela.
        let link_id = if typed { 0 } else { visited_link(&tx, url_id, &navigations[index - 1].url).map_err(sql)? };
        let segment_id = if typed { segment(&tx, &nav.url, url_id, time).map_err(sql)? } else { 0 };
        tx.execute(
            "INSERT INTO visits VALUES (NULL, ?1, ?2, ?3, '', ?4, ?5, ?6, ?7, 0, '', 0, 0, 0, 0, 1, ?8, '')",
            params![url_id, time, previous_visit, transition, segment_id, if index < last || process.ending != Ending::Live { duration } else { 0 }, i64::from(typed), link_id],
        )
        .map_err(sql)?;
        let visit_id = tx.last_insert_rowid();
        let open = index == last && process.ending == Ending::Live;
        let foreground = if open { -1_000_000 } else if typed { end_us - process.start_us.min(nav.start_us) } else { duration + 15_000 };
        let (root, parent) = if typed { (time, -1) } else { (-1, time) };
        let end_reason = if open { 0 } else if index == last { END_REASON_CLOSE } else { END_REASON_NAVIGATION };
        tx.execute(
            "INSERT INTO context_annotations VALUES (?1, 0, -1000000, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, 200)",
            params![visit_id, end_reason, foreground, process.window_id, process.window_id + 1, time, root, parent],
        )
        .map_err(sql)?;
        previous_visit = visit_id;
        last_visit = visit_id;
    }
    tx.execute("INSERT INTO content_annotations VALUES (?1, -1, NULL, '', -1, 0, '', '', '', '', '', '', 1, 0)", params![last_visit]).map_err(sql)?;
    tx.commit().map_err(sql)?;
    Ok(last_visit)
}

/// Fecha a visita que o commit de 10 segundos deixou aberta, como o `Browser.close` faz: a duração, o motivo 6 e o tempo
/// em primeiro plano (o mesmo cálculo da saída direta).
fn close_open_visit(path: &str, visit: i64, navigation: &Navigation, process: Process) -> Result<(), String> {
    let connection = ul_sqlite::cli::open_conn(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let sql = |e: rusqlite::Error| e.to_string();
    connection.query_row("PRAGMA journal_mode=TRUNCATE", [], |_| Ok(())).map_err(sql)?;
    let duration = process.exit_us - navigation.start_us;
    let foreground = process.exit_us - process.start_us.min(navigation.start_us);
    connection.execute("UPDATE visits SET visit_duration = ?1 WHERE id = ?2", params![duration, visit]).map_err(sql)?;
    connection.execute("UPDATE context_annotations SET page_end_reason = ?1, total_foreground_duration = ?2 WHERE visit_id = ?3", params![END_REASON_CLOSE, foreground, visit]).map_err(sql)?;
    Ok(())
}

/// As páginas do `History` do molde (40 de 4096 bytes) que o SQLite do Chromium registra no journal quando a navegação
/// abre a transação do commit de 10 segundos, na ordem em que o oráculo as grava (medido em seis execuções).
const HOT_JOURNAL_PAGES: [u32; 16] = [40, 4, 5, 11, 10, 9, 8, 6, 23, 22, 21, 28, 27, 25, 30, 29];
const PAGE_SIZE: usize = 4096;
const SECTOR_SIZE: usize = 512;

/// O `History-journal` quente que o oráculo deixa quando o SIGTERM chega antes do commit de 10 segundos (66176 bytes):
/// o cabeçalho ainda sem o selo do `fsync` (magia e contagem de páginas zeradas), o nonce sorteado, o tamanho do banco,
/// o setor e a página, e para cada página da lista a imagem antes da mudança (a do próprio banco, que não foi tocado)
/// com o checksum do SQLite. Só vale para o banco do molde (40 páginas); outro tamanho devolve `None`.
pub fn hot_journal(db: &[u8], nonce: u32) -> Option<Vec<u8>> {
    if db.len() != 40 * PAGE_SIZE {
        return None;
    }
    let mut journal = vec![0u8; SECTOR_SIZE];
    journal[12..16].copy_from_slice(&nonce.to_be_bytes());
    journal[16..20].copy_from_slice(&40u32.to_be_bytes());
    journal[20..24].copy_from_slice(&(SECTOR_SIZE as u32).to_be_bytes());
    journal[24..28].copy_from_slice(&(PAGE_SIZE as u32).to_be_bytes());
    for number in HOT_JOURNAL_PAGES {
        let image = &db[(number as usize - 1) * PAGE_SIZE..number as usize * PAGE_SIZE];
        let mut sum = nonce;
        let mut at = PAGE_SIZE as isize - 200;
        while at > 0 {
            sum = sum.wrapping_add(u32::from(image[at as usize]));
            at -= 200;
        }
        journal.extend_from_slice(&number.to_be_bytes());
        journal.extend_from_slice(image);
        journal.extend_from_slice(&sum.to_be_bytes());
    }
    Some(journal)
}

/// A navegação da aba que o navegador de pé guarda para o History: ela entra no banco 10 segundos depois (ou na hora, no
/// `Browser.close`); um SIGTERM antes disso a perde, como no oráculo (o journal fica quente e o banco, sem a visita).
#[derive(Debug)]
pub struct HistoryLog {
    path: String,
    start_us: i64,
    window_id: i64,
    state: std::sync::Mutex<LogState>,
}

#[derive(Debug, Default)]
struct LogState {
    navigation: Option<Navigation>,
    /// O id da visita que o commit de 10 segundos gravou aberta.
    open_visit: Option<i64>,
    /// O `Browser.close` já gravou a visita fechada.
    closed: bool,
}

impl HistoryLog {
    pub fn new(path: String, start_us: i64, window_id: i64) -> HistoryLog {
        HistoryLog { path, start_us, window_id, state: std::sync::Mutex::default() }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, LogState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Anota a navegação digitada da aba (a da linha de comando); `about:blank` não entra no History.
    pub fn record(&self, navigation: Navigation) {
        if navigation.url != "about:blank" {
            self.state().navigation.get_or_insert(navigation);
        }
    }

    /// O título atual da página, que fecha a linha de `urls`.
    pub fn set_title(&self, title: &str) {
        if let Some(navigation) = &mut self.state().navigation {
            navigation.title = title.to_owned();
        }
    }

    fn process(&self, ending: Ending, exit_us: i64) -> Process {
        Process { start_us: self.start_us, exit_us, window_id: self.window_id, ending }
    }

    /// O commit de 10 segundos: com a navegação anotada há tanto tempo e ainda fora do banco, grava a visita aberta.
    pub fn flush_due(&self, now_us: i64) {
        let mut state = self.state();
        let Some(navigation) = state.navigation.clone() else { return };
        if state.open_visit.is_none() && now_us - navigation.start_us >= LIVE_COMMIT_US {
            state.open_visit = write(&self.path, &[navigation], self.process(Ending::Live, now_us)).ok();
        }
    }

    /// Há navegação que ainda não chegou ao banco: o journal quente do molde é o que o oráculo deixa nesse caso.
    pub fn pending(&self) -> bool {
        let state = self.state();
        state.navigation.is_some() && state.open_visit.is_none() && !state.closed
    }

    /// O History já tem a visita no banco (o commit de 10 segundos ou o `Browser.close`): o journal fica vazio.
    pub fn committed(&self) -> bool {
        let state = self.state();
        state.open_visit.is_some() || state.closed
    }

    /// O `Browser.close`: grava a visita fechada, ou fecha a que o commit de 10 segundos deixou aberta.
    pub fn close(&self, exit_us: i64) {
        let mut state = self.state();
        let Some(navigation) = state.navigation.clone() else { return };
        state.closed = true;
        let process = self.process(Ending::Close, exit_us);
        let _ = match state.open_visit {
            Some(visit) => close_open_visit(&self.path, visit, &navigation, process),
            None => write(&self.path, &[navigation], process).map(drop),
        };
    }
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
    fn the_hot_journal_has_the_oracle_layout() {
        // Cada página do banco é preenchida com o próprio número: o checksum é o nonce mais 20 amostras (a cada 200
        // bytes de trás para a frente, sem passar do começo) do número da página.
        let db: Vec<u8> = (1..=40u8).flat_map(|number| vec![number; PAGE_SIZE]).collect();
        let journal = hot_journal(&db, 0x1000).unwrap();
        assert_eq!(journal.len(), 66176);
        assert_eq!(&journal[..12], &[0; 12]);
        assert_eq!(&journal[12..28], [0, 0, 0x10, 0, 0, 0, 0, 40, 0, 0, 2, 0, 0, 0, 0x10, 0]);
        assert!(journal[28..SECTOR_SIZE].iter().all(|&byte| byte == 0));
        let second = SECTOR_SIZE + 8 + PAGE_SIZE;
        assert_eq!(&journal[second..second + 4], 4u32.to_be_bytes());
        assert_eq!(&journal[second + 4 + PAGE_SIZE..second + 8 + PAGE_SIZE], (0x1000u32 + 20 * 4).to_be_bytes());
        assert_eq!(hot_journal(&db[..PAGE_SIZE], 1), None);
    }

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
