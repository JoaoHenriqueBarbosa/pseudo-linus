//! Configuração do NSS do `getent`: leitura de `/etc/nsswitch.conf` como a `nss_database.c` da glibc
//! 2.41 (`process_line` e `__nss_action_parse`), os padrões de cada banco, a troca por `-s` e a
//! regra de ações por status (`[NOTFOUND=return]` etc.).
//!
//! No sandbox só existem os módulos de arquivo: `files` (e `compat`, que lê os mesmos arquivos e
//! ignora as entradas `+`/`-` do NIS). Os demais (`db`, `dns`, `nis`, `systemd`...) respondem como
//! módulo indisponível, o que o NSS trata como "continue" por padrão.

use std::collections::HashMap;

use sysabi::sys;

use super::inet::is_space;

/// Resultado de uma consulta a um módulo (`enum nss_status` sem o `RETURN`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Success,
    NotFound,
    Unavail,
    TryAgain,
}

impl Status {
    fn index(self) -> usize {
        match self {
            Status::Success => 0,
            Status::NotFound => 1,
            Status::Unavail => 2,
            Status::TryAgain => 3,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Action {
    Return,
    Continue,
    Merge,
}

/// Uma fonte da lista de um banco (`files`, `dns [!UNAVAIL=return]`...).
#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    actions: [Action; 4],
}

impl Source {
    fn action(&self, st: Status) -> Action {
        self.actions[st.index()]
    }

    /// `true` se este módulo existe no sandbox e lê os arquivos de `/etc`.
    pub fn is_files(&self) -> bool {
        self.name.eq_ignore_ascii_case("files") || self.name.eq_ignore_ascii_case("compat")
    }

    /// `true` se a ação desta fonte para o status é `return` (a busca para aqui).
    pub fn stops_on(&self, st: Status) -> bool {
        self.action(st) == Action::Return
    }
}

pub type SourceList = Vec<Source>;

/// Bancos que o `nsswitch.conf` conhece (`databases.def`).
const DATABASES: &[&str] = &[
    "aliases",
    "ethers",
    "group",
    "group_compat",
    "gshadow",
    "hosts",
    "initgroups",
    "netgroup",
    "networks",
    "passwd",
    "passwd_compat",
    "protocols",
    "publickey",
    "rpc",
    "services",
    "shadow",
    "shadow_compat",
];

/// `__nss_action_parse`: `None` em erro de sintaxe (a lista vira vazia, como um módulo ausente).
pub fn parse_actions(line: &[u8]) -> Option<SourceList> {
    let n = line.len();
    let mut i = 0usize;
    let mut out: SourceList = Vec::new();
    let skip_ws = |i: &mut usize| {
        while *i < n && is_space(line[*i]) {
            *i += 1;
        }
    };
    loop {
        skip_ws(&mut i);
        if i >= n {
            return Some(out);
        }
        let start = i;
        while i < n && !is_space(line[i]) && line[i] != b'[' {
            i += 1;
        }
        if start == i {
            return Some(out);
        }
        let name = String::from_utf8_lossy(&line[start..i]).into_owned();
        // Padrão: tudo continua, exceto SUCCESS e RETURN.
        let mut actions = [Action::Continue; 4];
        actions[Status::Success.index()] = Action::Return;
        skip_ws(&mut i);
        if i < n && line[i] == b'[' {
            i += 1;
            skip_ws(&mut i);
            loop {
                let not = i < n && line[i] == b'!';
                if not {
                    i += 1;
                }
                let s = i;
                while i < n && !is_space(line[i]) && line[i] != b'=' && line[i] != b']' {
                    i += 1;
                }
                let status = match line[s..i].to_ascii_lowercase().as_slice() {
                    b"success" => Status::Success,
                    b"unavail" => Status::Unavail,
                    b"notfound" => Status::NotFound,
                    b"tryagain" => Status::TryAgain,
                    _ => return None,
                };
                skip_ws(&mut i);
                if i >= n || line[i] != b'=' {
                    return None;
                }
                i += 1;
                skip_ws(&mut i);
                let s = i;
                while i < n && !is_space(line[i]) && line[i] != b'=' && line[i] != b']' {
                    i += 1;
                }
                let action = match line[s..i].to_ascii_lowercase().as_slice() {
                    b"return" => Action::Return,
                    b"continue" => Action::Continue,
                    b"merge" => Action::Merge,
                    _ => return None,
                };
                if not {
                    // Todos os outros status recebem a ação; este mantém a que tinha.
                    let save = actions[status.index()];
                    actions = [action; 4];
                    actions[status.index()] = save;
                } else {
                    actions[status.index()] = action;
                }
                skip_ws(&mut i);
                if i >= n {
                    // `]` ausente: o laço de C leria além do fim; trata como erro de sintaxe.
                    return None;
                }
                if line[i] == b']' {
                    break;
                }
            }
            i += 1;
        }
        out.push(Source { name, actions });
    }
}

/// Configuração carregada: o que `/etc/nsswitch.conf` diz mais as trocas do `-s`.
pub struct NssConf {
    explicit: HashMap<String, SourceList>,
    overrides: HashMap<String, SourceList>,
}

impl NssConf {
    /// Lê `/etc/nsswitch.conf` (arquivo ausente ou ilegível vale como arquivo vazio).
    pub fn load() -> NssConf {
        let mut explicit: HashMap<String, SourceList> = HashMap::new();
        if let Ok(data) = sys::read_file(b"/etc/nsswitch.conf") {
            for raw in data.split(|b| *b == b'\n') {
                let mut line = raw;
                while let Some((&c, rest)) = line.split_first() {
                    if is_space(c) {
                        line = rest;
                    } else {
                        break;
                    }
                }
                let mut p = 0usize;
                while p < line.len() && !is_space(line[p]) && line[p] != b':' {
                    p += 1;
                }
                if p >= line.len() || p == 0 {
                    continue;
                }
                let name = String::from_utf8_lossy(&line[..p]).into_owned();
                while p < line.len() && (is_space(line[p]) || line[p] == b':') {
                    p += 1;
                }
                if !DATABASES.contains(&name.as_str()) {
                    continue;
                }
                // Erro de sintaxe deixa a lista vazia (o `__nss_action_parse` devolve NULL e o
                // banco cai no padrão; aqui vira lista vazia, que é o efeito prático).
                explicit.insert(name, parse_actions(&line[p..]).unwrap_or_default());
            }
        }
        NssConf { explicit, overrides: HashMap::new() }
    }

    /// `-s CONFIG`: troca a lista de um banco (`__nss_configure_lookup`, que ignora nomes que o NSS
    /// não conhece).
    pub fn configure(&mut self, db: &str, service_line: &[u8]) {
        if is_nss_database(db) {
            if let Some(list) = parse_actions(service_line) {
                self.overrides.insert(db.to_string(), list);
            }
        }
    }

    fn default_for(db: &str) -> SourceList {
        let line: &[u8] = match db {
            "group" | "passwd" | "shadow" => b"compat [NOTFOUND=return] files",
            "gshadow" => b"files",
            "hosts" | "networks" => b"files dns",
            "initgroups" => b"",
            _ => b"nis [NOTFOUND=return] files",
        };
        parse_actions(line).unwrap_or_default()
    }

    /// A lista efetiva de um banco, com os padrões de dependência da glibc (`shadow` segue `passwd`,
    /// `gshadow` segue `group`).
    pub fn sources(&self, db: &str) -> SourceList {
        if let Some(l) = self.overrides.get(db) {
            return l.clone();
        }
        if let Some(l) = self.explicit.get(db) {
            return l.clone();
        }
        let follow = match db {
            "shadow" => Some("passwd"),
            "gshadow" => Some("group"),
            _ => None,
        };
        if let Some(parent) = follow {
            if let Some(l) = self.overrides.get(parent).or_else(|| self.explicit.get(parent)) {
                return l.clone();
            }
        }
        NssConf::default_for(db)
    }

    /// Consulta pontual: percorre as fontes até uma ação `return` (o `DB_LOOKUP` do NSS). `f` recebe
    /// a fonte e devolve o status e, no sucesso, o valor.
    pub fn lookup<T>(&self, db: &str, mut f: impl FnMut(&Source) -> (Status, Option<T>)) -> Option<T> {
        for src in self.sources(db) {
            let (st, val) = if src.is_files() { f(&src) } else { (Status::Unavail, None) };
            if st == Status::Success {
                if let Some(v) = val {
                    return Some(v);
                }
            }
            if src.action(st) == Action::Return {
                return None;
            }
        }
        None
    }

    /// Enumeração (`setXXent`/`getXXent`): junta as entradas de cada fonte, parando na primeira cuja
    /// ação para o status final (`NOTFOUND` ao fim dos dados) seja `return`.
    pub fn enumerate<T>(&self, db: &str, mut f: impl FnMut(&Source) -> (Status, Vec<T>)) -> Vec<T> {
        let mut out = Vec::new();
        for src in self.sources(db) {
            let (st, mut items) = if src.is_files() { f(&src) } else { (Status::Unavail, Vec::new()) };
            out.append(&mut items);
            // Ao esgotar os dados de uma fonte o status é NOTFOUND (ou UNAVAIL se ela não abriu).
            let end = if st == Status::Success { Status::NotFound } else { st };
            if src.action(end) == Action::Return {
                break;
            }
        }
        out
    }
}

/// Nomes de banco do NSS (`databases.def`); os do `getent` que não estão aqui (`ahosts`...) não têm
/// configuração própria e o `-s` os ignora.
pub fn is_nss_database(name: &str) -> bool {
    DATABASES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_parse() {
        let l = parse_actions(b"files [NOTFOUND=return] dns").unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].name, "files");
        assert_eq!(l[0].action(Status::NotFound), Action::Return);
        assert_eq!(l[0].action(Status::Unavail), Action::Continue);
        assert_eq!(l[1].action(Status::Success), Action::Return);
        let l = parse_actions(b"dns [!UNAVAIL=return] files").unwrap();
        assert_eq!(l[0].action(Status::Unavail), Action::Continue);
        assert_eq!(l[0].action(Status::NotFound), Action::Return);
        assert!(parse_actions(b"files [BOGUS=return]").is_none());
    }
}
