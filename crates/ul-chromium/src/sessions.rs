//! Os arquivos de sessão do perfil (`Default/Sessions/Session_*` e `Tabs_*`), no formato SNSS que o Chromium 154 grava
//! (medido no oráculo com `testbench/oracle/measure-navigation.sh`; os dois moldes de estado de página saem de
//! `testbench/oracle/session-state-template.py`). Um arquivo é uma sequência de comandos (`u16` com o tamanho, o id e o
//! conteúdo); a navegação de cada entrada leva o `PageState` do Blink serializado em mojo, e dele só variam o endereço
//! da página (a matriz UTF-16 muda de tamanho e os ponteiros que a atravessam acompanham), os dois GUIDs e o número de
//! sequência. Os moldes e a remontagem conferem byte a byte com dois arquivos do oráculo (endereços de 34 e de 66
//! caracteres, `tests/sessions.rs`).
//!
//! Cobre a forma da execução com uma navegação para uma página (a entrada `about:blank` e a da página) e a do navegador
//! de pé com o DevTools (`SessionsLog`). O que mais o Chromium grava (redirecionamentos, POST, recarga, páginas de
//! erro) ainda não foi medido.

/// O estado de página da entrada `about:blank` que tem uma navegação depois dela.
static BLANK_STATE: &[u8] = include_bytes!("../profile/session-state-blank.bin");
/// O estado de página da entrada da página, com o endereço de 34 caracteres do molde.
static PAGE_STATE: &[u8] = include_bytes!("../profile/session-state-page.bin");

/// Onde, no estado, ficam o número de sequência (dois `i64`), os GUIDs (36 caracteres UTF-16 cada) e a matriz UTF-16 do
/// endereço (o molde guarda o de 34 caracteres: 80 bytes com o cabeçalho), e os ponteiros do `FrameState` que a
/// atravessam. Coordenadas do estado, que começa com o cabeçalho do pickle (4 bytes) e o do mojo (a versão e o tamanho).
const SEQUENCE_AT: usize = 0x6C;
const GUID_AT: [usize; 2] = [0x1D4, 0x234];
const URL_ARRAY_AT: usize = 0xCC;
const TEMPLATE_URL_ARRAY_LEN: usize = 80;
const CROSSING_POINTERS: [usize; 7] = [0x44, 0x54, 0x64, 0x7C, 0x84, 0x94, 0x9C];

/// A transição da entrada da página (digitada, pela linha de comando) e a do `about:blank` inicial.
const TRANSITION_TYPED_FROM_API: i32 = 0x0800_0001;
const TRANSITION_AUTO_TOPLEVEL: i32 = 6;

/// O que a execução fez: com isso os dois arquivos saem completos.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// O id da janela; a aba é o seguinte e a aba do `Tabs_` o outro.
    pub window_id: u32,
    /// O GUID da aba, com sublinhados no lugar dos hífens.
    pub tab_guid: String,
    /// Tempos em microssegundos desde 1601 (relógio WebKit): a criação da aba, a navegação inicial e a da página, o fim
    /// da página e o que o `Tabs_` guarda.
    pub created: i64,
    pub blank_time: i64,
    pub page_time: i64,
    pub page_end: i64,
    /// O instante, logo depois do fim da página, do último comando de atividade da aba.
    pub closed: i64,
    pub tabs_time: i64,
    /// Os dois GUIDs do estado de cada entrada (`about:blank`, página), em minúsculas com hífens.
    pub guids: [[String; 2]; 2],
    /// O número de sequência da primeira entrada; a da página tem dois a mais.
    pub sequence: i64,
    pub url: String,
    pub title: String,
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pad4(out: &mut Vec<u8>) {
    out.resize(out.len().next_multiple_of(4), 0);
}

/// Uma string do pickle: o tamanho, os bytes e o enchimento até 4.
fn put_str(out: &mut Vec<u8>, bytes: &[u8]) {
    put_i32(out, bytes.len() as i32);
    out.extend_from_slice(bytes);
    pad4(out);
}

/// Uma string16 do pickle: o número de caracteres UTF-16, os caracteres e o enchimento.
fn put_str16(out: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    put_i32(out, units.len() as i32);
    out.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    pad4(out);
}

fn ints(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_le_bytes()).collect()
}

fn command(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() + 1) as u16).to_le_bytes().to_vec();
    out.push(id);
    out.extend_from_slice(payload);
    out
}

fn snss(commands: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"SNSS".to_vec();
    put_i32(&mut out, 3);
    out.extend(commands.into_iter().flatten());
    out
}

/// A matriz de `uint16` do mojo: o cabeçalho (bytes e elementos), os caracteres e o enchimento até 8.
fn utf16_array(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out = ((8 + units.len() * 2) as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&(units.len() as u32).to_le_bytes());
    out.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    out.resize(out.len().next_multiple_of(8), 0);
    out
}

fn patch_u32(state: &mut [u8], at: usize, delta: usize) {
    let value = u32::from_le_bytes(state[at..at + 4].try_into().expect("quatro bytes"));
    state[at..at + 4].copy_from_slice(&(value + delta as u32).to_le_bytes());
}

/// O estado de página: o molde com o endereço (`None` mantém o do molde), os GUIDs e a sequência.
fn page_state(template: &[u8], url: Option<&str>, guids: &[String; 2], sequence: i64) -> Vec<u8> {
    let mut state = template.to_vec();
    let mut delta = 0;
    if let Some(url) = url {
        let array = utf16_array(url);
        delta = array.len() - TEMPLATE_URL_ARRAY_LEN;
        state.splice(URL_ARRAY_AT..URL_ARRAY_AT + TEMPLATE_URL_ARRAY_LEN, array);
        for at in CROSSING_POINTERS {
            let value = u64::from_le_bytes(state[at..at + 8].try_into().expect("oito bytes"));
            if value != 0 {
                state[at..at + 8].copy_from_slice(&(value + delta as u64).to_le_bytes());
            }
        }
        patch_u32(&mut state, 0, delta);
        patch_u32(&mut state, 8, delta);
    }
    for (index, value) in [sequence, sequence + 1].into_iter().enumerate() {
        state[SEQUENCE_AT + 8 * index..SEQUENCE_AT + 8 * index + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (at, guid) in GUID_AT.into_iter().zip(guids) {
        let units: Vec<u8> = guid.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
        state[at + delta..at + delta + units.len()].copy_from_slice(&units);
    }
    state
}

/// Uma entrada de navegação (`SerializedNavigationEntry`): o conteúdo do comando de navegação da aba `tab`.
struct Entry<'a> {
    tab: i32,
    index: i32,
    url: &'a str,
    title: &'a str,
    state: Vec<u8>,
    transition: i32,
    status: i32,
    /// Os três ids de tarefa do fim da entrada.
    tasks: [i64; 3],
    time: i64,
}

impl Entry<'_> {
    fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        put_i32(&mut body, self.tab);
        put_i32(&mut body, self.index);
        put_str(&mut body, self.url.as_bytes());
        put_str16(&mut body, self.title);
        put_i32(&mut body, self.state.len() as i32);
        body.extend_from_slice(&self.state);
        pad4(&mut body);
        put_i32(&mut body, self.transition);
        put_i32(&mut body, 0);
        put_str(&mut body, b"");
        put_i32(&mut body, 2);
        put_str(&mut body, self.url.as_bytes());
        put_i32(&mut body, 0);
        put_i64(&mut body, self.time);
        put_str16(&mut body, "");
        put_i32(&mut body, self.status);
        put_i32(&mut body, 6);
        put_i32(&mut body, 0);
        for task in self.tasks {
            put_i64(&mut body, task);
        }
        put_i32(&mut body, 0);
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend(body);
        out
    }
}

/// As duas entradas da aba (`about:blank` e a página) da aba de id `tab`; `in_tabs` é o `Tabs_`, que guarda o título da página e
/// os ids de tarefa dela (o `Session_` não).
fn entries(snapshot: &Snapshot, tab: i32, in_tabs: bool) -> [Vec<u8>; 2] {
    let blank_state = page_state(BLANK_STATE, None, &snapshot.guids[0], snapshot.sequence);
    let page_state = page_state(PAGE_STATE, Some(&snapshot.url), &snapshot.guids[1], snapshot.sequence + 2);
    let blank = Entry { tab, index: 0, url: "about:blank", title: "", state: blank_state, transition: TRANSITION_AUTO_TOPLEVEL, status: 0, tasks: [snapshot.blank_time, -1, snapshot.blank_time], time: snapshot.blank_time };
    let page = Entry {
        tab,
        index: 1,
        url: &snapshot.url,
        title: if in_tabs { &snapshot.title } else { "" },
        state: page_state,
        transition: TRANSITION_TYPED_FROM_API,
        status: 200,
        tasks: if in_tabs { [snapshot.page_time, -1, snapshot.page_time] } else { [-1; 3] },
        time: snapshot.page_time,
    };
    [blank.encode(), page.encode()]
}

/// A configuração da janela que o `Session_` leva: a chave e o valor como string do pickle, depois do id da janela.
fn setting(window_id: u32, key: &str, value: &str) -> Vec<u8> {
    let mut body = window_id.to_le_bytes().to_vec();
    put_str(&mut body, key.as_bytes());
    put_str(&mut body, value.as_bytes());
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    out
}

/// O `Session_`: a janela, a aba e as duas entradas, com o fim da página.
pub fn session_file(snapshot: &Snapshot) -> Vec<u8> {
    let window = snapshot.window_id as i32;
    let tab = window + 1;
    let [blank, page] = entries(snapshot, tab, false);
    let mut guid = vec![0x2C, 0, 0, 0];
    put_i32(&mut guid, tab);
    put_str(&mut guid, snapshot.tab_guid.as_bytes());
    let time = |id: i32, at: i64| {
        let mut payload = ints(&[id, 0]);
        put_i64(&mut payload, at);
        payload
    };
    let mut commands = vec![
        command(9, &ints(&[window, 0])),
        command(15, &ints(&[8, window, 0])),
        command(31, &ints(&[8, window, 0])),
        command(23, &ints(&[8, window, 0])),
        command(32, &ints(&[window, 0])),
        command(34, &setting(snapshot.window_id, "vertical_tab_strip_collapsed", "false")),
        command(34, &setting(snapshot.window_id, "vertical_tab_strip_uncollapsed_width", "240")),
        command(20, &ints(&[window])),
        command(12, &ints(&[tab, 0])),
        command(2, &ints(&[tab, 0])),
        command(25, &[ints(&[tab]), vec![0; 28]].concat()),
        command(0, &ints(&[window, tab])),
        command(19, &guid),
        command(8, &ints(&[window, 0])),
        command(21, &time(tab, snapshot.created)),
        command(7, &ints(&[tab, 0])),
        command(6, &blank),
        command(7, &ints(&[tab, 1])),
        command(6, &page),
        command(16, &time(tab, snapshot.page_end)),
        command(8, &ints(&[window, -1])),
        command(21, &time(-1, snapshot.closed)),
    ];
    commands.extend((0..3).map(|_| command(14, &ints(&[window, 10, 10, 0x30C, 0x244, 1]))));
    commands.push(command(255, &[]));
    snss(commands)
}

/// O `Tabs_`: a aba com as duas entradas, a da página com o título.
pub fn tabs_file(snapshot: &Snapshot) -> Vec<u8> {
    let tab = snapshot.window_id as i32 + 2;
    let [blank, page] = entries(snapshot, tab, true);
    let mut marker = ints(&[tab, 1]);
    put_i64(&mut marker, snapshot.tabs_time);
    snss(vec![command(4, &marker), command(1, &blank), command(1, &page), command(255, &[])])
}

/// Os campos de uma entrada de navegação que o navegador de pé reescreve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryParts {
    pub tab: i32,
    pub index: i32,
    pub url: String,
    pub state: Vec<u8>,
    pub transition: i32,
    pub status: i32,
    pub time: i64,
}

/// Um cursor sobre o conteúdo de um comando (inteiros little endian e strings do pickle).
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(slice)
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// Uma string do pickle (o tamanho, os bytes e o enchimento até 4).
    fn string(&mut self) -> Option<&'a [u8]> {
        let length = self.i32()? as usize;
        let bytes = self.take(length)?;
        self.take(length.next_multiple_of(4) - length)?;
        Some(bytes)
    }

    /// Uma string16 do pickle (o número de caracteres, os caracteres e o enchimento).
    fn string16(&mut self) -> Option<()> {
        let units = self.i32()? as usize;
        self.take((units * 2).next_multiple_of(4))?;
        Some(())
    }
}

/// Lê o conteúdo de um comando de entrada de navegação (o que `Entry::encode` escreve).
pub fn decode_entry(payload: &[u8]) -> Option<EntryParts> {
    let mut cursor = Cursor { bytes: payload, at: 4 };
    let tab = cursor.i32()?;
    let index = cursor.i32()?;
    let url = String::from_utf8(cursor.string()?.to_vec()).ok()?;
    cursor.string16()?;
    let state_length = cursor.i32()? as usize;
    let state = cursor.take(state_length)?.to_vec();
    cursor.take(state_length.next_multiple_of(4) - state_length)?;
    let transition = cursor.i32()?;
    cursor.i32()?;
    cursor.string()?;
    cursor.i32()?;
    cursor.string()?;
    cursor.i32()?;
    let time = cursor.i64()?;
    cursor.string16()?;
    let status = cursor.i32()?;
    Some(EntryParts { tab, index, url, state, transition, status, time })
}

/// Os comandos de um arquivo SNSS, na ordem, como (id, conteúdo).
fn commands_of(file: &[u8]) -> Option<Vec<(u8, Vec<u8>)>> {
    let mut at = 8;
    let mut out = Vec::new();
    while at + 2 <= file.len() {
        let size = usize::from(u16::from_le_bytes([file[at], file[at + 1]]));
        let body = file.get(at + 2..at + 2 + size)?;
        out.push((*body.first()?, body[1..].to_vec()));
        at += 2 + size;
    }
    Some(out)
}

fn rebuilt(commands: Vec<(u8, Vec<u8>)>) -> Vec<u8> {
    snss(commands.into_iter().map(|(id, payload)| command(id, &payload)).collect())
}

/// A transição do `about:blank` e da página aberta pela linha de comando, e o estado da entrada: no navegador de pé a
/// aba tem uma entrada só.
const LIVE_TRANSITION: i32 = 6;

/// O `Session_` do navegador de pé com a página da linha de comando: a entrada única do molde (`about:blank`) dá lugar à
/// da página (índice 0, transição 6, status 200, sem título), na aba que o molde já tem.
pub fn live_session(template: &[u8], url: &str, guids: &[String; 2], sequence: i64, time: i64) -> Option<Vec<u8>> {
    let mut commands = commands_of(template)?;
    let position = commands.iter().position(|(id, _)| *id == 6)?;
    let blank = decode_entry(&commands[position].1)?;
    let entry = Entry { tab: blank.tab, index: 0, url, title: "", state: page_state(PAGE_STATE, Some(url), guids, sequence), transition: LIVE_TRANSITION, status: 200, tasks: [-1; 3], time };
    commands[position].1 = entry.encode();
    Some(rebuilt(commands))
}

/// O `Tabs_` que o `Browser.close` grava: a aba (janela mais 2 no comando 9, mais 3 no resto) com a entrada do
/// `Session_` (a mesma, com o título da página e os ids de tarefa) e os instantes do fechamento. `closed` é o instante
/// do fechamento (relógio WebKit); o comando 9 leva 17 ms antes dele e o 4, 50 µs depois (medido em cinco execuções, de
/// 13 a 22 ms).
pub fn live_tabs(session: &[u8], title: &str, closed: i64) -> Option<Vec<u8>> {
    let commands = commands_of(session)?;
    let window = i32::from_le_bytes(commands.iter().find(|(id, _)| *id == 9)?.1.get(..4)?.try_into().ok()?);
    let parts = decode_entry(&commands.iter().find(|(id, _)| *id == 6)?.1)?;
    let tab = window + 3;
    let entry = Entry {
        tab,
        index: 0,
        url: &parts.url,
        title,
        state: parts.state,
        transition: parts.transition,
        status: parts.status,
        tasks: [parts.time, -1, parts.time],
        time: parts.time,
    };
    let window_time = closed - 17_000;
    let mut size = vec![0x30, 0, 0, 0];
    put_i32(&mut size, window + 2);
    put_i32(&mut size, 0);
    put_i32(&mut size, 1);
    put_i64(&mut size, window_time);
    size.extend(ints(&[10, 10, 0x30C, 0x244, 1, 0, 0]));
    let mut marker = ints(&[tab, 0]);
    put_i64(&mut marker, window_time + 50);
    Some(snss(vec![command(9, &size), command(4, &marker), command(1, &entry.encode()), command(255, &[])]))
}

/// Os arquivos de sessão do navegador de pé (DevTools): o `Session_` ganha a página da linha de comando assim que o
/// navegador a abre e o `Browser.close` grava o `Tabs_` (o SIGTERM só deixa o `Session_`, medido no oráculo aos 4, 8 e 12 s).
#[derive(Debug)]
pub struct SessionsLog {
    folder: String,
    guids: [String; 2],
    sequence: i64,
    title: std::sync::Mutex<String>,
}

impl SessionsLog {
    pub fn new(folder: String, guids: [String; 2], sequence: i64) -> SessionsLog {
        SessionsLog { folder, guids, sequence, title: std::sync::Mutex::default() }
    }

    fn find(&self, prefix: &str) -> Option<String> {
        let found = sysabi::sys::read_dir(self.folder.as_bytes()).ok()?.into_iter().find(|entry| entry.name.starts_with(prefix.as_bytes()))?;
        Some(format!("{}/{}", self.folder, String::from_utf8_lossy(&found.name)))
    }

    /// A página da linha de comando abriu: reescreve o `Session_` do molde (sem efeito para `about:blank`).
    pub fn start(&self, url: &str, time: i64) {
        if url == "about:blank" {
            return;
        }
        let Some(path) = self.find("Session_") else { return };
        let Ok(template) = sysabi::sys::read_file(path.as_bytes()) else { return };
        if let Some(file) = live_session(&template, url, &self.guids, self.sequence, time) {
            let _ = sysabi::util::write_file(path.as_bytes(), &file, 0o600);
        }
    }

    pub fn set_title(&self, title: &str) {
        *self.title.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = title.to_owned();
    }

    /// O `Browser.close`: grava o `Tabs_` com o título da página; o nome é o instante do fechamento.
    pub fn close(&self, closed: i64) {
        let Some(path) = self.find("Session_") else { return };
        let Ok(session) = sysabi::sys::read_file(path.as_bytes()) else { return };
        let title = self.title.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        // Só a página tem título no `Tabs_` (o `about:blank` fica sem).
        let is_blank = commands_of(&session).and_then(|commands| decode_entry(&commands.iter().find(|(id, _)| *id == 6)?.1)).is_some_and(|parts| parts.url == "about:blank");
        if let Some(file) = live_tabs(&session, if is_blank { "" } else { &title }, closed) {
            let _ = sysabi::util::write_file(format!("{}/Tabs_{closed}", self.folder).as_bytes(), &file, 0o600);
        }
    }
}
