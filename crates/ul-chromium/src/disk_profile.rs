//! O perfil que o navegador grava no disco, igual ao do Chromium 154 do oráculo.
//!
//! O Chromium cria a árvore do perfil no primeiro uso (`--user-data-dir`): 27 diretórios na raiz, `Local State`,
//! `Last Version`, os bancos SQLite e LevelDB vazios, os caches, mais os `Singleton*` e o diretório de soquete em
//! `/tmp`. Quem faz `ls` ou `cat` nesse diretório vê tudo isso, então ele tem de existir com os mesmos nomes, tipos,
//! modos e tamanhos. O molde (`profile/template.pack`) é gerado por `testbench/oracle/profile-pack.py` a partir de
//! três execuções do oráculo: o que é igual nelas vira byte fixo, o que varia (relógio, ids aleatórios, o caminho do
//! perfil) vira marcador que este módulo preenche a cada execução.
//!
//! O que o molde ainda não refaz, e as fatias seguintes fecham: os checksums dos arquivos binários com campos
//! aleatórios (o WAL do SQLite, o índice do cache de disco), e o perfil do `--dump-dom`, que sai do processo antes de
//! a árvore estar completa e pede um molde à parte. O banco do Local Storage já não vem do molde: o leveldb próprio
//! (`ul-leveldb`) o cria e o `LocalStorageStore` grava nele o que as páginas guardam.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use sysabi::{sys, AtFlags, Errno, Fd, Mode};

use ul_common::fsutil::mkdir_p;
use ul_common::time::{Civil, WEBKIT_EPOCH_US};

use crate::history::{self, Navigation};
use crate::http_cache::{self, Captured};
use crate::sessions;
use crate::idb::disk::IndexedDbStore;
use crate::local_storage::LocalStorageStore;
use crate::session_storage::{Exit, SessionStorageStore};
use crate::sys_env::SysEnv;
use ul_leveldb::env::Env;

/// O molde, gerado no oráculo (ver o cabeçalho de `testbench/oracle/profile-pack.py` para o formato): o começo e a
/// saída do navegador com DevTools remoto.
static PACK: &[u8] = include_bytes!("../profile/template.pack");
/// A saída do `--dump-dom` (o começo é o mesmo do DevTools): a árvore inteira da saída e o que ela apaga.
static PACK_DUMP: &[u8] = include_bytes!("../profile/template-dump.pack");
/// O diretório do banco do Local Storage no perfil: o molde o traz (de uma execução do oráculo), mas quem o grava é o
/// leveldb próprio, com o log, o MANIFEST e o `LOG` de verdade.
const LOCAL_STORAGE_DB: &[u8] = b"Default/Local Storage/leveldb";
/// O diretório do banco do Session Storage: apagado e recriado a cada início, pelo leveldb próprio.
const SESSION_STORAGE_DB: &[u8] = b"Default/Session Storage";
/// Os arquivos do molde que uma execução seguinte encontra e mantém, como o Chromium faz.
const KEPT_ACROSS_RUNS: [&[u8]; 2] = [b"Default/History", b"Default/History-journal"];
/// Os bancos que o molde traz mas que o leveldb próprio grava.
const OWN_DBS: [&[u8]; 2] = [LOCAL_STORAGE_DB, SESSION_STORAGE_DB];
/// O diretório do IndexedDB no perfil: só existe depois que uma página o usa.
const INDEXED_DB_DIR: &[u8] = b"Default/IndexedDB";

/// Delimitador dos marcadores (U+E000 em UTF-8).
const MARK: [u8; 3] = [0xEE, 0x80, 0x80];

const KIND_DIR: u8 = 0;
const KIND_FILE: u8 = 1;
const KIND_REMOVE: u8 = 3;
const ROOT_USER_DATA: u8 = 0;
const ROOT_CONFIG: u8 = 1;
const LAYER_START: u8 = 0;
const LAYER_EXIT: u8 = 1;

struct Entry {
    layer: u8,
    root: u8,
    kind: u8,
    mode: Mode,
    path: Vec<u8>,
    data: Vec<u8>,
    /// Faixas `(deslocamento, tamanho)` que levam bytes aleatórios.
    patches: Vec<(usize, usize)>,
}

/// Como o navegador roda, o que decide o que a saída deixa no perfil (medido no oráculo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    /// `--remote-debugging-port` ou `--remote-debugging-pipe`: a saída (o `Browser.close` ou o SIGTERM) mantém os
    /// `Singleton*`.
    DevTools = 0,
    /// `--dump-dom` e o carregamento sem DevTools: a saída apaga os bancos que não chegaram a ser usados, os
    /// `Singleton*` e o diretório do soquete.
    DumpDom = 1,
}

/// O molde de `run`. O começo do perfil é o mesmo nas duas execuções e vem do molde do DevTools (o do `--dump-dom` só
/// leva a saída).
fn template(run: Run) -> &'static [Entry] {
    static TEMPLATES: OnceLock<[Vec<Entry>; 2]> = OnceLock::new();
    let templates = TEMPLATES.get_or_init(|| [PACK, PACK_DUMP].map(|pack| parse_pack(pack).expect("o molde do perfil embutido é válido")));
    &templates[run as usize]
}

fn parse_pack(pack: &[u8]) -> Option<Vec<Entry>> {
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(pack).ok()?;
    let mut cursor = Cursor { data: &raw, at: 0 };
    if cursor.take(6)? != b"ULPP1\n" {
        return None;
    }
    let count = cursor.u32()? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let layer = cursor.u8()?;
        let root = cursor.u8()?;
        let kind = cursor.u8()?;
        let mode = Mode::from(cursor.u16()?);
        let path_len = cursor.u16()? as usize;
        let path = cursor.take(path_len)?.to_vec();
        let data_len = cursor.u32()? as usize;
        let data = cursor.take(data_len)?.to_vec();
        let patch_count = cursor.u8()? as usize;
        let mut patches = Vec::with_capacity(patch_count);
        for _ in 0..patch_count {
            let offset = cursor.u32()? as usize;
            let length = cursor.u32()? as usize;
            cursor.u8()?;
            patches.push((offset, length));
        }
        entries.push(Entry { layer, root, kind, mode, path, data, patches });
    }
    Some(entries)
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Bytes aleatórios do pseudo-processo; fora de um (os testes de unidade deste módulo), os do host.
fn fill_random(buffer: &mut [u8]) {
    if let Some(process) = sys::try_current() {
        let _ = process.getrandom(buffer);
        return;
    }
    use std::hash::{BuildHasher, Hasher};
    for chunk in buffer.chunks_mut(8) {
        let word = std::collections::hash_map::RandomState::new().build_hasher().finish().to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buffer = [0u8; N];
    fill_random(&mut buffer);
    buffer
}

fn random_below(bound: u64) -> u64 {
    if bound <= 1 {
        return 0;
    }
    u64::from_le_bytes(random_bytes::<8>()) % bound
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const NAME_ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// O double como o Chromium o serializa (o `repr` mais curto, com `e+16` no expoente positivo).
fn float_text(value: f64) -> String {
    if value.abs() >= 1e16 {
        let text = format!("{value:e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        return format!("{mantissa}e+{exponent}");
    }
    format!("{value}")
}

/// Preenche os marcadores do molde com o relógio e o aleatório desta execução.
struct Expander<'a> {
    /// Início do navegador, em microssegundos desde 1970.
    start_us: i64,
    user_dir: &'a [u8],
}

impl Expander<'_> {
    fn expand(&self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        let mut at = 0;
        while let Some(open) = find(input, &MARK, at) {
            out.extend_from_slice(&input[at..open]);
            let Some(close) = find(input, &MARK, open + MARK.len()) else {
                out.extend_from_slice(&input[open..]);
                return out;
            };
            out.extend_from_slice(&self.token(&input[open + MARK.len()..close]));
            at = close + MARK.len();
        }
        out.extend_from_slice(&input[at..]);
        out
    }

    fn token(&self, token: &[u8]) -> Vec<u8> {
        let text = String::from_utf8_lossy(token);
        let text = text.split('#').next().unwrap_or("");
        let mut parts = text.split(':');
        let kind = parts.next().unwrap_or("");
        let args: Vec<i64> = parts.filter_map(|p| p.parse().ok()).collect();
        let arg = |index: usize| args.get(index).copied().unwrap_or(0);
        let webkit = self.start_us + WEBKIT_EPOCH_US;
        match kind {
            "wk" => (webkit + arg(0)).to_string().into_bytes(),
            "wkf" => float_text((webkit + arg(0)) as f64).into_bytes(),
            "unixs" => (self.start_us.div_euclid(1_000_000) + arg(0)).to_string().into_bytes(),
            "unixf" => float_text((self.start_us + arg(0)) as f64 / 1e6).into_bytes(),
            "ts" => leveldb_timestamp(self.start_us + arg(0)).into_bytes(),
            "userdir" => self.user_dir.to_vec(),
            "uuid" => crate::devtools::uuid().into_bytes(),
            "hex" => (0..arg(0)).map(|_| b"0123456789ABCDEF"[random_below(16) as usize]).collect(),
            "rb64" => random_base64(arg(0) as usize, arg(1) as usize),
            "rint" => (arg(0) as u64 + random_below((arg(1) - arg(0)) as u64)).to_string().into_bytes(),
            _ => token.to_vec(),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack.get(from..)?.windows(needle.len()).position(|window| window == needle).map(|p| p + from)
}

/// `AAAA/MM/DD-HH:MM:SS.mmm`, o carimbo do `LOG` do leveldb (em UTC).
fn leveldb_timestamp(micros: i64) -> String {
    let civil = Civil::from_secs(micros.div_euclid(1_000_000));
    let millis = micros.rem_euclid(1_000_000) / 1000;
    format!("{:04}/{:02}/{:02}-{:02}:{:02}:{:02}.{millis:03}", civil.year, civil.mon, civil.mday, civil.hour, civil.min, civil.sec)
}

/// `count` caracteres base64 aleatórios; com `pad` sinais `=` depois, o último caractere é o canônico (os bits que o
/// preenchimento deixa de fora ficam em zero).
fn random_base64(count: usize, pad: usize) -> Vec<u8> {
    let mut out: Vec<u8> = (0..count).map(|_| BASE64[random_below(64) as usize]).collect();
    if let Some(last) = out.last_mut() {
        let index = BASE64.iter().position(|c| c == last).unwrap_or(0);
        let mask = match pad {
            1 => !0x03,
            2 => !0x0f,
            _ => !0,
        };
        *last = BASE64[index & mask];
    }
    out
}

/// O perfil gravado no disco pelo navegador em curso.
pub struct DiskProfile {
    user_data_dir: Option<Vec<u8>>,
    /// `$XDG_CONFIG_HOME` ou `$HOME/.config`: o molde traz o `chromium/Crash Reports` do crashpad a partir daqui.
    config_base: Vec<u8>,
    start_us: i64,
    run: Run,
    /// O diretório do soquete em `/tmp` (`org.chromium.Chromium.<6 letras>`).
    socket_dir: Option<Vec<u8>>,
    /// O banco do Local Storage (`Default/Local Storage/leveldb`), que o molde não traz: o banco próprio o cria.
    local_storage: Option<Arc<LocalStorageStore>>,
    /// O banco do Session Storage (`Default/Session Storage`), recriado a cada início.
    session_storage: Option<Arc<SessionStorageStore>>,
    /// As navegações da aba nesta execução, para o History.
    navigations: Mutex<Vec<Navigation>>,
    /// As respostas que o navegador buscou nesta execução, cada uma com a página de topo, para o cache de disco.
    resources: Mutex<Vec<(String, Captured)>>,
    /// O id da janela das anotações de contexto do History: aleatório por execução.
    window_id: i64,
}

impl DiskProfile {
    /// O primeiro uso: cria a árvore do perfil (com `--user-data-dir`, a do perfil e a do crashpad; sem ele, só a do
    /// crashpad), os `Singleton*` e o diretório de soquete em `/tmp`. Os erros de gravação não impedem o navegador de
    /// subir: o Chromium também segue sem o perfil quando o disco recusa.
    pub fn start(user_data_dir: Option<&[u8]>, run: Run) -> DiskProfile {
        let mut profile = DiskProfile::new(user_data_dir, run);
        let _ = profile.write_layer(template(Run::DevTools), LAYER_START);
        profile.socket_dir = profile.write_singletons().ok();
        profile.local_storage = profile.open_db(LOCAL_STORAGE_DB, LocalStorageStore::open);
        profile.session_storage = profile.open_db(SESSION_STORAGE_DB, |env, dir, thread_id| {
            // O Chromium recria o banco do Session Storage a cada início: o da execução anterior some.
            let _ = ul_common::fsutil::remove_tree(dir.as_bytes());
            SessionStorageStore::open(env, dir, thread_id)
        });
        profile
    }

    fn new(user_data_dir: Option<&[u8]>, run: Run) -> DiskProfile {
        let home = sys::getenv("HOME").filter(|h| !h.is_empty()).unwrap_or_else(|| b"/root".to_vec());
        let config_base = match sys::getenv("XDG_CONFIG_HOME") {
            Some(path) if !path.is_empty() => path,
            _ => [home.as_slice(), b"/.config"].concat(),
        };
        DiskProfile { user_data_dir: user_data_dir.map(<[u8]>::to_vec), config_base, start_us: (ul_common::time::realtime_nanos() / 1000) as i64, run, socket_dir: None, local_storage: None, session_storage: None, navigations: Mutex::new(Vec::new()), resources: Mutex::new(Vec::new()), window_id: random_below(1 << 31) as i64 }
    }

    /// O IndexedDB do perfil (`Default/IndexedDB`): cada origem ganha o leveldb dela no primeiro uso.
    pub fn indexed_db(&self) -> Option<Arc<IndexedDbStore>> {
        let dir = [self.user_data_dir.as_deref()?, b"/", INDEXED_DB_DIR].concat();
        let thread_id = sys::current().gettid() as u64;
        Some(Arc::new(IndexedDbStore::new(Arc::new(SysEnv::new()), &String::from_utf8_lossy(&dir), thread_id)))
    }

    /// O banco do Local Storage do perfil, para o contexto de navegação padrão do navegador.
    pub fn local_storage(&self) -> Option<Arc<LocalStorageStore>> {
        self.local_storage.clone()
    }

    /// O banco do Session Storage do perfil: cada aba do contexto padrão ganha o `namespace` dela nele.
    pub fn session_storage(&self) -> Option<Arc<SessionStorageStore>> {
        self.session_storage.clone()
    }

    /// Abre o banco que mora em `relative` dentro do `--user-data-dir` com o leveldb próprio, ou nada sem o diretório.
    fn open_db<T>(&self, relative: &[u8], open: impl FnOnce(Arc<dyn Env>, &str, u64) -> ul_leveldb::Result<T>) -> Option<Arc<T>> {
        let dir = [self.user_data_dir.as_deref()?, b"/", relative].concat();
        let thread_id = sys::current().gettid() as u64;
        open(Arc::new(SysEnv::new()), &String::from_utf8_lossy(&dir), thread_id).ok().map(Arc::new)
    }

    /// Anota uma navegação concluída da aba: o History a grava na saída.
    pub fn record_navigation(&self, navigation: Navigation) {
        // O `about:blank` não entra no History nem na sessão: o molde já tem a forma dele.
        if navigation.url == "about:blank" {
            return;
        }
        self.navigations.lock().unwrap_or_else(PoisonError::into_inner).push(navigation);
    }

    /// Anota uma resposta buscada pelo navegador (a página ou o favicon dela): o cache de disco a guarda na saída.
    pub fn record_resource(&self, top_url: &str, captured: Captured) {
        self.resources.lock().unwrap_or_else(PoisonError::into_inner).push((top_url.to_owned(), captured));
    }

    /// O cache de disco da saída do `--dump-dom`: um arquivo por resposta e o índice com as que já estavam lá.
    fn write_cache(&self) {
        let Some(dir) = &self.user_data_dir else { return };
        let resources = self.resources.lock().unwrap_or_else(PoisonError::into_inner);
        if resources.is_empty() {
            return;
        }
        let folder = [dir.as_slice(), b"/Default/Cache/Cache_Data"].concat();
        let index_path = [folder.as_slice(), b"/index-dir/the-real-index"].concat();
        let mut index = sys::read_file(&index_path).ok().and_then(|bytes| http_cache::parse_index(&bytes)).unwrap_or_default();
        for (top_url, captured) in resources.iter() {
            let entry = captured.entry(top_url);
            let _ = sysabi::util::write_file(&[folder.as_slice(), b"/", entry.name.as_bytes()].concat(), &entry.content, 0o600);
            match index.iter_mut().find(|line| line.id == entry.index.id) {
                Some(line) => *line = entry.index,
                None => index.push(entry.index),
            }
        }
        let _ = sysabi::util::write_file(&index_path, &http_cache::index_file(&index, ul_common::time::webkit_now_micros() - 30_000), 0o600);
    }

    /// O History da saída do `--dump-dom`: a cadeia de navegações da aba, no banco que o molde trouxe.
    fn write_history(&self) {
        let Some(dir) = &self.user_data_dir else { return };
        let path = String::from_utf8_lossy(&[dir.as_slice(), b"/Default/History"].concat()).into_owned();
        let navigations = self.navigations.lock().unwrap_or_else(PoisonError::into_inner);
        let process = history::Process { start_us: self.start_us, exit_us: (ul_common::time::realtime_nanos() / 1000) as i64, window_id: self.window_id, dump: self.run == Run::DumpDom };
        let _ = history::write(&path, &navigations, process);
    }

    /// Os arquivos de sessão da saída do `--dump-dom` com uma navegação: o `Session_*` e o `Tabs_*` que o molde criou
    /// ganham as entradas da página (os nomes ficam; deles vem o relógio da saída).
    fn write_sessions(&self) {
        let Some(dir) = &self.user_data_dir else { return };
        let Some(page) = self.navigations.lock().unwrap_or_else(PoisonError::into_inner).last().cloned() else { return };
        let folder = [dir.as_slice(), b"/Default/Sessions"].concat();
        let found = |prefix: &[u8]| sys::read_dir(&folder).unwrap_or_default().into_iter().find(|e| e.name.starts_with(prefix)).map(|e| e.name);
        let (Some(session), Some(tabs)) = (found(b"Session_"), found(b"Tabs_")) else { return };
        let exit_time: i64 = String::from_utf8_lossy(&session["Session_".len()..]).parse().unwrap_or_default();
        let created = self.start_us + WEBKIT_EPOCH_US;
        let page_end = exit_time - 20_500;
        let random = || (random_below(1 << 36)) as i64;
        let guid = |_| crate::devtools::uuid();
        let snapshot = sessions::Snapshot {
            window_id: self.window_id as u32,
            tab_guid: crate::devtools::uuid().replace('-', "_"),
            created,
            blank_time: created + 10_000,
            page_time: page.start_us + WEBKIT_EPOCH_US,
            page_end,
            closed: page_end + 5,
            tabs_time: page_end - 900,
            guids: [[guid(0), guid(1)], [guid(2), guid(3)]],
            sequence: (0x65d8 << 36) | random(),
            url: page.url,
            title: page.title,
        };
        for (name, content) in [(session, sessions::session_file(&snapshot)), (tabs, sessions::tabs_file(&snapshot))] {
            let _ = sysabi::util::write_file(&[folder.as_slice(), b"/", &name].concat(), &content, 0o600);
        }
    }

    /// A saída: o que o Chromium só grava ao encerrar (`Preferences`, o `Local State` completo), depois do commit do
    /// que as páginas gravaram no Local Storage e no Session Storage, que ainda grava o que a forma de sair pede.
    pub fn finish(&self, exit: Exit) {
        if let Some(store) = &self.local_storage {
            let _ = store.flush();
        }
        if let Some(store) = &self.session_storage {
            let _ = store.finish(exit);
        }
        let _ = self.write_layer(template(self.run), LAYER_EXIT);
        if self.run == Run::DumpDom {
            self.write_history();
            self.write_sessions();
            self.write_cache();
            self.remove_singletons();
        }
    }

    /// O `--dump-dom` apaga ao sair os `Singleton*` do perfil e o diretório do soquete.
    fn remove_singletons(&self) {
        if let Some(dir) = &self.user_data_dir {
            for name in [&b"SingletonCookie"[..], b"SingletonLock", b"SingletonSocket"] {
                let _ = sys::current().unlinkat(Fd::CWD, &[dir.as_slice(), b"/", name].concat(), AtFlags::empty());
            }
        }
        if let Some(socket_dir) = &self.socket_dir {
            let _ = ul_common::fsutil::remove_tree(socket_dir);
        }
    }

    /// As entradas de `layer`: primeiro as remoções, depois diretórios e arquivos na ordem do molde.
    fn write_layer(&self, entries: &[Entry], layer: u8) -> Result<(), Errno> {
        let user_dir = self.user_data_dir.as_deref().unwrap_or(b"");
        let expander = Expander { start_us: self.start_us, user_dir };
        let selected = || entries.iter().filter(|e| e.layer == layer && !(e.root == ROOT_USER_DATA && OWN_DBS.iter().any(|db| e.path.starts_with(db))));
        for entry in selected().filter(|e| e.kind == KIND_REMOVE) {
            let (Some(dir), ROOT_USER_DATA) = (&self.user_data_dir, entry.root) else { continue };
            let path = [dir.as_slice(), b"/", &expander.expand(&entry.path)].concat();
            match path.strip_suffix(b"*") {
                Some(prefix) => {
                    let parent = ul_common::fsutil::dirname(prefix);
                    let start = ul_common::fsutil::after_last_slash(prefix);
                    for found in sys::read_dir(parent).unwrap_or_default().iter().filter(|e| e.name.starts_with(start)) {
                        ul_common::fsutil::remove_tree(&[parent, b"/", &found.name].concat())?;
                    }
                }
                None => ul_common::fsutil::remove_tree(&path)?,
            }
        }
        for entry in selected().filter(|e| e.kind != KIND_REMOVE) {
            let base = match entry.root {
                ROOT_USER_DATA => match &self.user_data_dir {
                    Some(dir) => dir.as_slice(),
                    None => continue,
                },
                ROOT_CONFIG => self.config_base.as_slice(),
                _ => continue,
            };
            let relative = expander.expand(&entry.path);
            let path = [base, b"/", &relative].concat();
            // O que o Chromium guarda de uma execução para a outra (o History acumula as visitas) não é refeito.
            if entry.kind == KIND_FILE && KEPT_ACROSS_RUNS.contains(&relative.as_slice()) && sys::lstat(&path).is_ok() {
                continue;
            }
            match entry.kind {
                KIND_DIR => {
                    mkdir_p(&path, entry.mode)?;
                    sys::current().fchmodat(Fd::CWD, &path, entry.mode, AtFlags::empty())?;
                }
                KIND_FILE => {
                    mkdir_p(ul_common::fsutil::dirname(&path), 0o700)?;
                    let mut data = expander.expand(&entry.data);
                    for &(offset, length) in &entry.patches {
                        if let Some(range) = data.get_mut(offset..offset + length) {
                            range.copy_from_slice(&random_vec(length));
                        }
                    }
                    sysabi::util::write_file(&path, &data, entry.mode)?;
                    sys::current().fchmodat(Fd::CWD, &path, entry.mode, AtFlags::empty())?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Os `Singleton*` do perfil e o diretório de soquete em `/tmp`, como o Chromium os cria: o cookie é um número
    /// aleatório de 64 bits, o lock é `<hostname>-<pid>`, e o soquete mora em `/tmp/org.chromium.Chromium.<6 letras>`.
    fn write_singletons(&self) -> Result<Vec<u8>, Errno> {
        let sys = sys::current();
        let name: String = (0..6).map(|_| NAME_ALPHABET[random_below(62) as usize] as char).collect();
        let socket_dir = format!("/tmp/org.chromium.Chromium.{name}");
        let cookie = u64::from_le_bytes(random_bytes::<8>()).to_string();
        mkdir_p(socket_dir.as_bytes(), 0o700)?;
        sys.fchmodat(Fd::CWD, socket_dir.as_bytes(), 0o700, AtFlags::empty())?;
        sys.symlinkat(cookie.as_bytes(), Fd::CWD, format!("{socket_dir}/SingletonCookie").as_bytes())?;
        let socket = format!("{socket_dir}/SingletonSocket");
        // O soquete é um nó do sistema de arquivos. O kernel do sandbox o cria; o `TestKit` dos testes só cria FIFO,
        // então lá o nó falta e o resto do perfil sai igual.
        if sys.mknodat(Fd::CWD, socket.as_bytes(), 0o140_755, 0).is_ok() {
            sys.fchmodat(Fd::CWD, socket.as_bytes(), 0o755, AtFlags::empty())?;
        }
        if let Some(dir) = &self.user_data_dir {
            let hostname = String::from_utf8_lossy(&sys.uname().nodename).into_owned();
            let lock = format!("{hostname}-{}", sys.getpid());
            sys.symlinkat(cookie.as_bytes(), Fd::CWD, &[dir.as_slice(), b"/SingletonCookie"].concat())?;
            sys.symlinkat(lock.as_bytes(), Fd::CWD, &[dir.as_slice(), b"/SingletonLock"].concat())?;
            sys.symlinkat(socket.as_bytes(), Fd::CWD, &[dir.as_slice(), b"/SingletonSocket"].concat())?;
        }
        Ok(socket_dir.into_bytes())
    }
}

fn random_vec(length: usize) -> Vec<u8> {
    let mut out = vec![0u8; length];
    fill_random(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_template_parses() {
        let entries = template(Run::DevTools);
        assert!(entries.len() > 200);
        assert!(entries.iter().any(|e| e.path == b"Local State" && e.layer == LAYER_START));
        assert!(entries.iter().any(|e| e.path == b"Default/Preferences" && e.layer == LAYER_EXIT));
    }

    #[test]
    fn float_text_follows_the_chromium_writer() {
        assert_eq!(float_text(1.3436139277210222e16), "1.3436139277210222e+16");
        assert_eq!(float_text(1791665279.892065), "1791665279.892065");
        assert_eq!(float_text(0.5), "0.5");
    }

    #[test]
    fn leveldb_timestamps_use_millis() {
        assert_eq!(leveldb_timestamp(1_791_665_279_892_065), "2026/10/10-20:47:59.892");
    }

    #[test]
    fn markers_are_filled() {
        let expander = Expander { start_us: 1_791_665_279_000_000, user_dir: b"/tmp/p" };
        let mut input = b"a".to_vec();
        input.extend_from_slice(&MARK);
        input.extend_from_slice(b"wk:100#3");
        input.extend_from_slice(&MARK);
        input.extend_from_slice(b"b");
        input.extend_from_slice(&MARK);
        input.extend_from_slice(b"userdir");
        input.extend_from_slice(&MARK);
        assert_eq!(expander.expand(&input), b"a13436138879000100b/tmp/p");
    }

    #[test]
    fn random_base64_keeps_the_padding_canonical() {
        for _ in 0..50 {
            let text = random_base64(2, 2);
            let value = BASE64.iter().position(|c| *c == text[1]).unwrap();
            assert_eq!(value & 0x0f, 0);
        }
    }
}
