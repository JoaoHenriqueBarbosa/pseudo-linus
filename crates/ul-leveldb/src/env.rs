//! A saída do banco: o leveldb escreve e lê arquivos por este trait, de modo que a mesma crate roda sobre o sistema
//! de arquivos do pseudo-processo (quem a usa fornece a implementação sobre o `sysabi`), sobre a memória (os testes
//! de unidade) e sobre um diretório do hospedeiro (a conferência contra os bancos que o Chromium do oráculo escreveu,
//! só nos testes).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::{Error, Result};

/// Os arquivos de um diretório de banco. Os caminhos são textos com `/`; o diretório do banco é o prefixo.
pub trait Env {
    /// Cria o diretório e os pais que faltam; não falha se ele já existe.
    fn create_dir_all(&self, path: &str) -> Result<()>;
    fn exists(&self, path: &str) -> bool;
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    /// Cria o arquivo (ou esvazia o que existe) e grava `data`.
    fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    /// Acrescenta `data` ao fim do arquivo, criando-o se faltar.
    fn append_file(&self, path: &str, data: &[u8]) -> Result<()>;
    fn file_size(&self, path: &str) -> Result<u64>;
    fn rename(&self, from: &str, to: &str) -> Result<()>;
    fn remove_file(&self, path: &str) -> Result<()>;
    /// Os nomes (sem o diretório) dos arquivos de um diretório.
    fn list_dir(&self, path: &str) -> Result<Vec<String>>;
    /// O `LOCK` do banco: falha se outro `Db` aberto já o segura, cria o arquivo vazio se faltar.
    fn lock_file(&self, path: &str) -> Result<()>;
    fn unlock_file(&self, path: &str);
    /// O relógio do texto do `LOG`: microssegundos desde 1970 (UTC).
    fn now_micros(&self) -> u64;
}

/// Um sistema de arquivos em memória, para os testes.
#[derive(Clone, Default)]
pub struct MemEnv {
    state: Rc<RefCell<MemState>>,
}

#[derive(Default)]
struct MemState {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
    locked: BTreeSet<String>,
    clock: u64,
}

impl MemEnv {
    pub fn new() -> MemEnv {
        MemEnv::default()
    }

    /// Fixa o relógio (e a cada leitura ele anda um milissegundo, para o `LOG` ter horas crescentes).
    pub fn set_clock(&self, micros: u64) {
        self.state.borrow_mut().clock = micros;
    }

    /// Uma cópia de todos os arquivos (nome completo para conteúdo), para comparar.
    pub fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        self.state.borrow().files.clone()
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn not_found(path: &str) -> Error {
    Error::Io(format!("{path}: No such file or directory"))
}

impl Env for MemEnv {
    fn create_dir_all(&self, path: &str) -> Result<()> {
        self.state.borrow_mut().dirs.insert(path.to_owned());
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        let state = self.state.borrow();
        state.files.contains_key(path) || state.dirs.contains(path)
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.state.borrow().files.get(path).cloned().ok_or_else(|| not_found(path))
    }

    fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.state.borrow_mut().files.insert(path.to_owned(), data.to_vec());
        Ok(())
    }

    fn append_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.state.borrow_mut().files.entry(path.to_owned()).or_default().extend_from_slice(data);
        Ok(())
    }

    fn file_size(&self, path: &str) -> Result<u64> {
        self.state.borrow().files.get(path).map(|data| data.len() as u64).ok_or_else(|| not_found(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let data = state.files.remove(from).ok_or_else(|| not_found(from))?;
        state.files.insert(to.to_owned(), data);
        Ok(())
    }

    fn remove_file(&self, path: &str) -> Result<()> {
        self.state.borrow_mut().files.remove(path).map(|_| ()).ok_or_else(|| not_found(path))
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let state = self.state.borrow();
        Ok(state.files.keys().filter(|name| parent(name) == path).map(|name| name[path.len() + 1..].to_owned()).collect())
    }

    fn lock_file(&self, path: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if !state.locked.insert(path.to_owned()) {
            return Err(Error::Io(format!("lock {path}: already held by process")));
        }
        state.files.entry(path.to_owned()).or_default();
        Ok(())
    }

    fn unlock_file(&self, path: &str) {
        self.state.borrow_mut().locked.remove(path);
    }

    fn now_micros(&self) -> u64 {
        let mut state = self.state.borrow_mut();
        state.clock += 1000;
        state.clock
    }
}
