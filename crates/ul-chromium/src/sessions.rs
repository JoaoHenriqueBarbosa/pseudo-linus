//! Os arquivos de sessão do perfil (`Default/Sessions/Session_*` e `Tabs_*`), no formato SNSS que o Chromium 154 grava
//! (medido no oráculo com `testbench/oracle/measure-navigation.sh`; os dois moldes de estado de página saem de
//! `testbench/oracle/session-state-template.py`). Um arquivo é uma sequência de comandos (`u16` com o tamanho, o id e o
//! conteúdo); a navegação de cada entrada leva o `PageState` do Blink serializado em mojo, e dele só variam o endereço
//! da página (a matriz UTF-16 muda de tamanho e os ponteiros que a atravessam acompanham), os dois GUIDs e o número de
//! sequência. Os moldes e a remontagem conferem byte a byte com dois arquivos do oráculo (endereços de 34 e de 66
//! caracteres, `tests/sessions.rs`).
//!
//! Cobre a forma da execução com uma navegação para uma página (a entrada `about:blank` e a da página). O que mais o
//! Chromium grava (redirecionamentos, POST, recarga, páginas de erro) ainda não foi medido.

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
