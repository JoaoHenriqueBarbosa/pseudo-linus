//! Usuários e chaves de API.
//!
//! - Cada usuário tem um nome (identidade), um papel (`admin` ou `user`), um estado (ativo ou
//!   desativado) e sobrescritas de quota.
//! - Cada chave pertence a um usuário, tem rótulo, data de criação, expiração opcional, revogação e
//!   último uso. O segredo só existe na saída do comando que cria a chave; o arquivo guarda o SHA-256
//!   do token inteiro. O segredo tem 256 bits aleatórios do `getrandom`, então um hash rápido basta
//!   (KDF lenta como Argon2 serve pra senha de baixa entropia, não pra token aleatório).
//! - Formato do token: `plk_<id de 16 hex>_<segredo de 64 hex>`. O id é público e serve de chave de
//!   busca; a comparação do hash é em tempo constante.
//!
//! Armazenamento: `auth.json` no diretório de dados, modo 0600, escrito de forma atômica (arquivo
//! temporário, `fsync`, `rename`, `fsync` do diretório). Toda escrita, do daemon ou do comando de
//! admin, segura um `flock` exclusivo em `auth.lock`, relê o arquivo, aplica a mudança e grava; por
//! isso duas escritas concorrentes nunca perdem atualização uma da outra. Quem só lê (a autenticação
//! de cada requisição) compara o `stat` do arquivo com o da última leitura e recarrega quando muda,
//! então chave criada ou revogada pelo comando de admin vale na requisição seguinte, sem reiniciar.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::{Quota, QuotaOverride, io_msg};

pub const TOKEN_PREFIX: &str = "plk_";
const KEY_ID_HEX: usize = 16;
const SECRET_HEX: usize = 64;
const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn parse(s: &str) -> Result<Role, String> {
        match s {
            "admin" => Ok(Role::Admin),
            "user" => Ok(Role::User),
            _ => Err(format!("papel desconhecido {s:?} (use admin ou user)")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    pub name: String,
    pub role: Role,
    pub created_at: u64,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub quota: QuotaOverride,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRecord {
    pub id: String,
    pub user: String,
    #[serde(default)]
    pub label: String,
    /// `sha256:<hex>` do token inteiro.
    pub hash: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub revoked_at: Option<u64>,
    #[serde(default)]
    pub last_used_at: Option<u64>,
}

impl KeyRecord {
    pub fn state(&self, now: u64) -> KeyState {
        if self.revoked_at.is_some() {
            KeyState::Revoked
        } else if self.expires_at.is_some_and(|e| e <= now) {
            KeyState::Expired
        } else {
            KeyState::Active
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyState {
    Active,
    Expired,
    Revoked,
}

/// Conteúdo do `auth.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthData {
    pub version: u32,
    pub users: Vec<UserRecord>,
    pub keys: Vec<KeyRecord>,
}

impl AuthData {
    pub fn user(&self, name: &str) -> Option<&UserRecord> {
        self.users.iter().find(|u| u.name == name)
    }

    fn user_mut(&mut self, name: &str) -> Result<&mut UserRecord, AuthError> {
        self.users.iter_mut().find(|u| u.name == name).ok_or_else(|| AuthError::NoSuchUser(name.to_string()))
    }

    pub fn key(&self, id: &str) -> Option<&KeyRecord> {
        self.keys.iter().find(|k| k.id == id)
    }
}

/// Quem fez a requisição.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub user: String,
    pub role: Role,
    pub key_id: String,
    pub quota: QuotaOverride,
}

impl Principal {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    pub fn effective_quota(&self, defaults: &Quota) -> Quota {
        self.quota.apply(defaults)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("faltou a chave de API (cabeçalho Authorization: Bearer plk_...)")]
    Missing,
    #[error("chave de API mal formada")]
    Malformed,
    #[error("chave de API inválida")]
    Invalid,
    #[error("chave de API revogada")]
    Revoked,
    #[error("chave de API expirada")]
    Expired,
    #[error("usuário desativado")]
    UserDisabled,
    #[error("usuário {0:?} não existe")]
    NoSuchUser(String),
    #[error("usuário {0:?} já existe")]
    UserExists(String),
    #[error("chave {0:?} não existe")]
    NoSuchKey(String),
    #[error("{0}")]
    BadInput(String),
    #[error("{path}: {msg}")]
    Io { path: PathBuf, msg: String },
    #[error("{path}: arquivo de autenticação corrompido: {msg}")]
    Corrupt { path: PathBuf, msg: String },
}

impl AuthError {
    /// Erros que dizem respeito à credencial apresentada (viram HTTP 401).
    pub fn is_credential_error(&self) -> bool {
        matches!(
            self,
            AuthError::Missing
                | AuthError::Malformed
                | AuthError::Invalid
                | AuthError::Revoked
                | AuthError::Expired
                | AuthError::UserDisabled
        )
    }
}

/// Nome de usuário: `[a-z][a-z0-9_-]{0,31}`.
pub fn validate_user_name(name: &str) -> Result<(), AuthError> {
    let ok = !name.is_empty()
        && name.len() <= 32
        && name.as_bytes()[0].is_ascii_lowercase()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(AuthError::BadInput(format!(
            "nome de usuário inválido {name:?}: use de 1 a 32 caracteres [a-z0-9_-], começando por letra"
        )))
    }
}

fn random_hex(bytes: usize) -> Result<String, AuthError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|e| AuthError::Io { path: PathBuf::from("getrandom"), msg: e.to_string() })?;
    Ok(hex::encode(buf))
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn encode_hash(h: &[u8; 32]) -> String {
    format!("sha256:{}", hex::encode(h))
}

fn decode_hash(s: &str) -> Option<[u8; 32]> {
    let hex_part = s.strip_prefix("sha256:")?;
    let v = hex::decode(hex_part).ok()?;
    v.try_into().ok()
}

/// Separa `plk_<id>_<segredo>` e devolve o id.
pub fn parse_token(token: &str) -> Result<&str, AuthError> {
    let rest = token.strip_prefix(TOKEN_PREFIX).ok_or(AuthError::Malformed)?;
    if rest.len() != KEY_ID_HEX + 1 + SECRET_HEX || rest.as_bytes()[KEY_ID_HEX] != b'_' {
        return Err(AuthError::Malformed);
    }
    let (id, secret) = (&rest[..KEY_ID_HEX], &rest[KEY_ID_HEX + 1..]);
    let is_hex = |s: &str| s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !is_hex(id) || !is_hex(secret) {
        return Err(AuthError::Malformed);
    }
    Ok(id)
}

/// Identidade do arquivo, pra saber se outro processo o reescreveu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
}

impl Stamp {
    fn of(meta: &fs::Metadata) -> Stamp {
        Stamp { dev: meta.dev(), ino: meta.ino(), len: meta.len(), mtime: meta.mtime(), mtime_nsec: meta.mtime_nsec() }
    }
}

struct Cache {
    data: AuthData,
    stamp: Option<Stamp>,
}

/// A loja de usuários e chaves.
pub struct AuthStore {
    path: PathBuf,
    lock_path: PathBuf,
    cache: RwLock<Cache>,
    /// Último uso de cada chave, ainda não gravado (gravar a cada requisição seria uma escrita por
    /// chamada); [`AuthStore::flush_last_used`] grava em lote.
    pending_last_used: Mutex<BTreeMap<String, u64>>,
}

impl std::fmt::Debug for AuthStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthStore").field("path", &self.path).finish_non_exhaustive()
    }
}

/// Resultado de criar uma chave: o registro e o token, que só aparece aqui.
#[derive(Debug)]
pub struct NewKey {
    pub record: KeyRecord,
    pub token: String,
}

impl AuthStore {
    /// Abre (e cria, se preciso) a loja em `path`. O diretório pai é criado com modo 0700.
    pub fn open(path: impl Into<PathBuf>) -> Result<AuthStore, AuthError> {
        let path = path.into();
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        if !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)
                .map_err(|e| AuthError::Io { path: dir.clone(), msg: io_msg(&e) })?;
        }
        let lock_path = path.with_extension("lock");
        let store = AuthStore {
            path,
            lock_path,
            cache: RwLock::new(Cache { data: AuthData { version: FORMAT_VERSION, ..Default::default() }, stamp: None }),
            pending_last_used: Mutex::new(BTreeMap::new()),
        };
        store.refresh()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_disk(&self) -> Result<(AuthData, Option<Stamp>), AuthError> {
        let io = |e: std::io::Error| AuthError::Io { path: self.path.clone(), msg: io_msg(&e) };
        let mut file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((AuthData { version: FORMAT_VERSION, ..Default::default() }, None));
            }
            Err(e) => return Err(io(e)),
        };
        let meta = file.metadata().map_err(io)?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut file, &mut text).map_err(io)?;
        let data: AuthData = serde_json::from_str(&text)
            .map_err(|e| AuthError::Corrupt { path: self.path.clone(), msg: e.to_string() })?;
        if data.version != FORMAT_VERSION {
            return Err(AuthError::Corrupt {
                path: self.path.clone(),
                msg: format!("versão {} desconhecida (esperada {FORMAT_VERSION})", data.version),
            });
        }
        Ok((data, Some(Stamp::of(&meta))))
    }

    /// Recarrega do disco se o arquivo mudou desde a última leitura.
    pub fn refresh(&self) -> Result<(), AuthError> {
        let current = match fs::metadata(&self.path) {
            Ok(m) => Some(Stamp::of(&m)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(AuthError::Io { path: self.path.clone(), msg: io_msg(&e) }),
        };
        if self.cache.read().stamp == current && current.is_some() {
            return Ok(());
        }
        let (data, stamp) = self.read_disk()?;
        *self.cache.write() = Cache { data, stamp };
        Ok(())
    }

    /// Cópia do conteúdo atual (recarregado se mudou).
    pub fn snapshot(&self) -> Result<AuthData, AuthError> {
        self.refresh()?;
        Ok(self.cache.read().data.clone())
    }

    /// Autentica um token. A ordem das checagens é: formato, existência e hash (mesmo erro pros dois,
    /// pra não revelar quais ids existem), e só então revogação, expiração e usuário desativado.
    pub fn authenticate(&self, token: &str, now: u64) -> Result<Principal, AuthError> {
        let id = parse_token(token)?;
        self.refresh()?;
        let presented = hash_token(token);
        let cache = self.cache.read();
        let key = cache.data.key(id).ok_or(AuthError::Invalid)?;
        let stored = decode_hash(&key.hash).ok_or(AuthError::Invalid)?;
        if !bool::from(stored.ct_eq(&presented)) {
            return Err(AuthError::Invalid);
        }
        match key.state(now) {
            KeyState::Revoked => return Err(AuthError::Revoked),
            KeyState::Expired => return Err(AuthError::Expired),
            KeyState::Active => {}
        }
        let user = cache.data.user(&key.user).ok_or(AuthError::Invalid)?;
        if user.disabled {
            return Err(AuthError::UserDisabled);
        }
        let principal =
            Principal { user: user.name.clone(), role: user.role, key_id: key.id.clone(), quota: user.quota.clone() };
        drop(cache);
        self.pending_last_used.lock().insert(principal.key_id.clone(), now);
        Ok(principal)
    }

    /// Aplica uma mudança com o arquivo travado: relê, muda, grava atômico.
    pub fn update<R>(&self, f: impl FnOnce(&mut AuthData) -> Result<R, AuthError>) -> Result<R, AuthError> {
        let io = |p: &Path, e: std::io::Error| AuthError::Io { path: p.to_path_buf(), msg: io_msg(&e) };
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&self.lock_path)
            .map_err(|e| io(&self.lock_path, e))?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)
            .map_err(|e| io(&self.lock_path, e.into()))?;
        let (mut data, _) = self.read_disk()?;
        let out = f(&mut data)?;
        self.write_atomic(&data)?;
        let stamp = fs::metadata(&self.path).ok().map(|m| Stamp::of(&m));
        *self.cache.write() = Cache { data, stamp };
        drop(lock);
        Ok(out)
    }

    fn write_atomic(&self, data: &AuthData) -> Result<(), AuthError> {
        let io = |p: &Path, e: std::io::Error| AuthError::Io { path: p.to_path_buf(), msg: io_msg(&e) };
        let mut text = serde_json::to_string_pretty(data).expect("AuthData sempre serializa");
        text.push('\n');
        let tmp = self.path.with_extension(format!("json.tmp.{}", std::process::id()));
        let result = (|| {
            let mut f = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|e| io(&tmp, e))?;
            f.write_all(text.as_bytes()).map_err(|e| io(&tmp, e))?;
            f.sync_all().map_err(|e| io(&tmp, e))?;
            fs::rename(&tmp, &self.path).map_err(|e| io(&self.path, e))?;
            if let Some(dir) = self.path.parent() {
                let d = File::open(if dir.as_os_str().is_empty() { Path::new(".") } else { dir })
                    .map_err(|e| io(dir, e))?;
                d.sync_all().map_err(|e| io(dir, e))?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Grava o último uso acumulado das chaves (o daemon chama periodicamente e ao sair).
    pub fn flush_last_used(&self) -> Result<usize, AuthError> {
        let pending = std::mem::take(&mut *self.pending_last_used.lock());
        if pending.is_empty() {
            return Ok(0);
        }
        let n = pending.len();
        let result = self.update(|d| {
            for k in &mut d.keys {
                if let Some(&t) = pending.get(&k.id) {
                    k.last_used_at = Some(k.last_used_at.map_or(t, |old| old.max(t)));
                }
            }
            Ok(())
        });
        if let Err(e) = result {
            // Devolve o que não foi gravado, pra próxima tentativa.
            let mut p = self.pending_last_used.lock();
            for (k, t) in pending {
                let e = p.entry(k).or_insert(t);
                *e = (*e).max(t);
            }
            return Err(e);
        }
        Ok(n)
    }

    pub fn create_user(&self, name: &str, role: Role, quota: QuotaOverride, now: u64) -> Result<UserRecord, AuthError> {
        validate_user_name(name)?;
        self.update(|d| {
            if d.user(name).is_some() {
                return Err(AuthError::UserExists(name.to_string()));
            }
            let u = UserRecord { name: name.to_string(), role, created_at: now, disabled: false, quota };
            d.users.push(u.clone());
            Ok(u)
        })
    }

    /// Muda papel, estado e quota de um usuário; `quota` é juntada por cima da sobrescrita atual.
    pub fn update_user(
        &self,
        name: &str,
        role: Option<Role>,
        disabled: Option<bool>,
        quota: Option<&QuotaOverride>,
        reset_quota: bool,
    ) -> Result<UserRecord, AuthError> {
        self.update(|d| {
            let u = d.user_mut(name)?;
            if let Some(r) = role {
                u.role = r;
            }
            if let Some(dis) = disabled {
                u.disabled = dis;
            }
            if reset_quota {
                u.quota = QuotaOverride::default();
            }
            if let Some(q) = quota {
                u.quota.merge(q);
            }
            Ok(u.clone())
        })
    }

    /// Remove o usuário e revoga todas as chaves dele (os registros das chaves ficam, pra auditoria).
    pub fn remove_user(&self, name: &str, now: u64) -> Result<usize, AuthError> {
        self.update(|d| {
            let before = d.users.len();
            d.users.retain(|u| u.name != name);
            if d.users.len() == before {
                return Err(AuthError::NoSuchUser(name.to_string()));
            }
            let mut revoked = 0;
            for k in d.keys.iter_mut().filter(|k| k.user == name && k.revoked_at.is_none()) {
                k.revoked_at = Some(now);
                revoked += 1;
            }
            Ok(revoked)
        })
    }

    pub fn create_key(&self, user: &str, label: &str, expires_at: Option<u64>, now: u64) -> Result<NewKey, AuthError> {
        if label.len() > 64 || label.chars().any(char::is_control) {
            return Err(AuthError::BadInput("rótulo inválido: até 64 caracteres, sem controle".into()));
        }
        if expires_at.is_some_and(|e| e <= now) {
            return Err(AuthError::BadInput("a expiração precisa estar no futuro".into()));
        }
        self.update(|d| {
            if d.user(user).is_none() {
                return Err(AuthError::NoSuchUser(user.to_string()));
            }
            let id = loop {
                let id = random_hex(KEY_ID_HEX / 2)?;
                if d.key(&id).is_none() {
                    break id;
                }
            };
            let token = format!("{TOKEN_PREFIX}{id}_{}", random_hex(SECRET_HEX / 2)?);
            let record = KeyRecord {
                id,
                user: user.to_string(),
                label: label.to_string(),
                hash: encode_hash(&hash_token(&token)),
                created_at: now,
                expires_at,
                revoked_at: None,
                last_used_at: None,
            };
            d.keys.push(record.clone());
            Ok(NewKey { record, token })
        })
    }

    /// Revoga uma chave. Revogar de novo não muda a data original.
    pub fn revoke_key(&self, id: &str, now: u64) -> Result<KeyRecord, AuthError> {
        self.update(|d| {
            let k = d.keys.iter_mut().find(|k| k.id == id).ok_or_else(|| AuthError::NoSuchKey(id.to_string()))?;
            if k.revoked_at.is_none() {
                k.revoked_at = Some(now);
            }
            Ok(k.clone())
        })
    }

    /// Apaga registros de chaves revogadas ou expiradas há mais de `older_than` segundos.
    pub fn prune_keys(&self, now: u64, older_than: u64) -> Result<usize, AuthError> {
        self.update(|d| {
            let before = d.keys.len();
            d.keys.retain(|k| {
                let dead_since = match (k.revoked_at, k.expires_at) {
                    (Some(r), _) => Some(r),
                    (None, Some(e)) if e <= now => Some(e),
                    _ => None,
                };
                dead_since.is_none_or(|t| now.saturating_sub(t) < older_than)
            });
            Ok(before - d.keys.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, AuthStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = AuthStore::open(dir.path().join("data/auth.json")).unwrap();
        (dir, s)
    }

    #[test]
    fn token_format_and_authentication() {
        let (_d, s) = store();
        let now = 1_000_000;
        s.create_user("alice", Role::User, QuotaOverride::default(), now).unwrap();
        let k = s.create_key("alice", "laptop", Some(now + 3600), now).unwrap();
        assert!(k.token.starts_with("plk_"));
        assert_eq!(k.token.len(), 4 + 16 + 1 + 64);
        assert_eq!(parse_token(&k.token).unwrap(), k.record.id);
        let p = s.authenticate(&k.token, now + 10).unwrap();
        assert_eq!(p.user, "alice");
        assert_eq!(p.role, Role::User);
        // Segredo trocado no último dígito: inválida, mesmo erro de id inexistente.
        let mut bad = k.token.clone();
        let last = bad.pop().unwrap();
        bad.push(if last == '0' { '1' } else { '0' });
        assert!(matches!(s.authenticate(&bad, now), Err(AuthError::Invalid)));
        let unknown = format!("plk_{}_{}", "0".repeat(16), "0".repeat(64));
        assert!(matches!(s.authenticate(&unknown, now), Err(AuthError::Invalid)));
        assert!(matches!(s.authenticate("Bearer x", now), Err(AuthError::Malformed)));
        assert!(matches!(s.authenticate(&k.token.to_uppercase(), now), Err(AuthError::Malformed)));
        // Expira.
        assert!(matches!(s.authenticate(&k.token, now + 3600), Err(AuthError::Expired)));
        // O arquivo não guarda o segredo.
        let text = fs::read_to_string(s.path()).unwrap();
        assert!(!text.contains(&k.token[21..]));
        let mode = fs::metadata(s.path()).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn revocation_and_disabled_user() {
        let (_d, s) = store();
        let now = 5;
        s.create_user("bob", Role::Admin, QuotaOverride::default(), now).unwrap();
        let k1 = s.create_key("bob", "", None, now).unwrap();
        let k2 = s.create_key("bob", "ci", None, now).unwrap();
        assert!(s.authenticate(&k1.token, now).unwrap().is_admin());
        s.revoke_key(&k1.record.id, 7).unwrap();
        assert!(matches!(s.authenticate(&k1.token, 8), Err(AuthError::Revoked)));
        assert_eq!(s.revoke_key(&k1.record.id, 9).unwrap().revoked_at, Some(7));
        assert!(s.authenticate(&k2.token, 8).is_ok());
        s.update_user("bob", None, Some(true), None, false).unwrap();
        assert!(matches!(s.authenticate(&k2.token, 8), Err(AuthError::UserDisabled)));
        s.update_user("bob", None, Some(false), None, false).unwrap();
        assert!(s.authenticate(&k2.token, 8).is_ok());
        assert_eq!(s.remove_user("bob", 10).unwrap(), 1);
        assert!(matches!(s.authenticate(&k2.token, 11), Err(AuthError::Revoked)));
        assert!(matches!(s.create_key("bob", "", None, 11), Err(AuthError::NoSuchUser(_))));
    }

    #[test]
    fn other_process_changes_are_seen() {
        let (d, s) = store();
        // Uma segunda loja no mesmo arquivo faz o papel do comando de admin rodando em paralelo.
        let admin = AuthStore::open(d.path().join("data/auth.json")).unwrap();
        admin.create_user("carol", Role::User, QuotaOverride::default(), 1).unwrap();
        let k = admin.create_key("carol", "", None, 1).unwrap();
        assert_eq!(s.authenticate(&k.token, 2).unwrap().user, "carol");
        admin.revoke_key(&k.record.id, 3).unwrap();
        assert!(matches!(s.authenticate(&k.token, 4), Err(AuthError::Revoked)));
    }

    #[test]
    fn concurrent_writers_do_not_lose_updates() {
        let (d, s) = store();
        s.create_user("dave", Role::User, QuotaOverride::default(), 1).unwrap();
        let path = d.path().join("data/auth.json");
        std::thread::scope(|sc| {
            for _ in 0..4 {
                let p = path.clone();
                sc.spawn(move || {
                    let st = AuthStore::open(p).unwrap();
                    for _ in 0..10 {
                        st.create_key("dave", "", None, 1).unwrap();
                    }
                });
            }
        });
        assert_eq!(s.snapshot().unwrap().keys.len(), 40);
    }

    #[test]
    fn last_used_is_flushed_in_batches() {
        let (_d, s) = store();
        s.create_user("erin", Role::User, QuotaOverride::default(), 1).unwrap();
        let k = s.create_key("erin", "", None, 1).unwrap();
        s.authenticate(&k.token, 100).unwrap();
        s.authenticate(&k.token, 200).unwrap();
        assert_eq!(s.snapshot().unwrap().key(&k.record.id).unwrap().last_used_at, None);
        assert_eq!(s.flush_last_used().unwrap(), 1);
        assert_eq!(s.snapshot().unwrap().key(&k.record.id).unwrap().last_used_at, Some(200));
        assert_eq!(s.flush_last_used().unwrap(), 0);
    }

    #[test]
    fn user_names_and_duplicates() {
        let (_d, s) = store();
        assert!(s.create_user("Alice", Role::User, QuotaOverride::default(), 1).is_err());
        assert!(s.create_user("9x", Role::User, QuotaOverride::default(), 1).is_err());
        assert!(s.create_user("", Role::User, QuotaOverride::default(), 1).is_err());
        s.create_user("a-b_c9", Role::User, QuotaOverride::default(), 1).unwrap();
        assert!(matches!(
            s.create_user("a-b_c9", Role::User, QuotaOverride::default(), 1),
            Err(AuthError::UserExists(_))
        ));
    }

    #[test]
    fn corrupt_file_is_reported_not_overwritten() {
        let (d, s) = store();
        s.create_user("frank", Role::User, QuotaOverride::default(), 1).unwrap();
        let path = d.path().join("data/auth.json");
        fs::write(&path, b"{ not json").unwrap();
        assert!(matches!(s.snapshot(), Err(AuthError::Corrupt { .. })));
        assert!(matches!(
            s.create_user("gina", Role::User, QuotaOverride::default(), 1),
            Err(AuthError::Corrupt { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), b"{ not json");
    }

    #[test]
    fn prune_removes_only_old_dead_keys() {
        let (_d, s) = store();
        s.create_user("hal", Role::User, QuotaOverride::default(), 1).unwrap();
        let live = s.create_key("hal", "", None, 1).unwrap();
        let old = s.create_key("hal", "", None, 1).unwrap();
        let recent = s.create_key("hal", "", None, 1).unwrap();
        s.revoke_key(&old.record.id, 10).unwrap();
        s.revoke_key(&recent.record.id, 1000).unwrap();
        assert_eq!(s.prune_keys(1100, 500).unwrap(), 1);
        let d = s.snapshot().unwrap();
        assert!(d.key(&live.record.id).is_some());
        assert!(d.key(&recent.record.id).is_some());
        assert!(d.key(&old.record.id).is_none());
    }
}
