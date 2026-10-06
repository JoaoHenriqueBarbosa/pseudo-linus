//! `hostname` do pacote hostname 3.25 do Debian 13 (não é do coreutils: tem outras opções, outras
//! mensagens e outro código de saída que o `hostname` do uutils).
//!
//! Comportamento medido no oráculo (Debian 13, C.UTF-8, sem rede):
//!
//! - Sem argumentos escreve o nome do `uname`. `-s` corta no primeiro ponto, sem consultar nada.
//! - `-f`, `-d`, `-i` e `-a` consultam o nome como a glibc com `hosts: files dns` e sem rede: só o
//!   `/etc/hosts` (várias linhas com o nome se juntam, como no `multi on`); sem linha, a mensagem é
//!   `Temporary failure in name resolution` e o código é 1. `-I` e `-A` olham as interfaces de
//!   rede e, sem nenhuma além do loopback, escrevem uma linha vazia. `-y` sem domínio NIS diz
//!   `Local domain name not set`. O último dos `-a -A -d -f -i -I -s -y` vence.
//! - Com um nome (ou `-F arquivo`) ele troca o nome da máquina: nome inválido é `the specified
//!   hostname is invalid`, falta de privilégio é `you must be root to change the host name`.
//! - Erro de opção (no formato do `getopt_long` da glibc, com o `argv[0]` como veio), nome a mais
//!   ou nome junto de uma opção de exibição escrevem o texto de uso no stderr e saem com 255.
//!   `-h` escreve o uso no stdout e também sai com 255. `-V` escreve `hostname 3.25`.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use sysabi::Ctx;

/// O texto de uso, igual nos três usos (erro, `-h` e nome a mais).
const USAGE: &str = concat!(
    "Usage: hostname [-b] {hostname|-F file}         set host name (from file)\n",
    "       hostname [-a|-A|-d|-f|-i|-I|-s|-y]       display formatted name\n",
    "       hostname                                 display host name\n",
    "\n",
    "       {yp,nis,}domainname {nisdomain|-F file}  set NIS domain name (from file)\n",
    "       {yp,nis,}domainname                      display NIS domain name\n",
    "\n",
    "       dnsdomainname                            display dns domain name\n",
    "\n",
    "       hostname -V|--version|-h|--help          print info and exit\n",
    "\n",
    "Program name:\n",
    "       {yp,nis,}domainname=hostname -y\n",
    "       dnsdomainname=hostname -d\n",
    "\n",
    "Program options:\n",
    "    -a, --alias            alias names\n",
    "    -A, --all-fqdns        all long host names (FQDNs)\n",
    "    -b, --boot             set default hostname if none available\n",
    "    -d, --domain           DNS domain name\n",
    "    -f, --fqdn, --long     long host name (FQDN)\n",
    "    -F, --file             read host name or NIS domain name from given file\n",
    "    -i, --ip-address       addresses for the host name\n",
    "    -I, --all-ip-addresses all addresses for the host\n",
    "    -s, --short            short host name\n",
    "    -y, --yp, --nis        NIS/YP domain name\n",
    "\n",
    "Description:\n",
    "   This command can get or set the host name or the NIS domain name. You can\n",
    "   also get the DNS domain or the FQDN (fully qualified domain name).\n",
    "   Unless you are using bind or NIS for host lookups you can change the\n",
    "   FQDN (Fully Qualified Domain Name) and the DNS domain name (which is\n",
    "   part of the FQDN) in the /etc/hosts file.\n",
);

/// O que as opções pedem para mostrar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Display {
    Plain,
    Alias,
    AllFqdns,
    Domain,
    Fqdn,
    Ip,
    AllIp,
    Short,
    Yp,
}

/// As opções longas, na ordem da tabela do programa (a ordem aparece na mensagem de abreviação
/// ambígua): nome, letra da opção curta equivalente e se leva argumento.
const LONG_OPTIONS: &[(&str, u8, bool)] = &[
    ("all-fqdns", b'A', false),
    ("alias", b'a', false),
    ("boot", b'b', false),
    ("domain", b'd', false),
    ("file", b'F', true),
    ("fqdn", b'f', false),
    ("help", b'h', false),
    ("long", b'f', false),
    ("ip-address", b'i', false),
    ("all-ip-addresses", b'I', false),
    ("short", b's', false),
    ("version", b'V', false),
    ("yp", b'y', false),
    ("nis", b'y', false),
];

/// O que a linha de comando pediu.
struct Request {
    display: Option<Display>,
    boot: bool,
    file: Option<OsString>,
    operands: Vec<OsString>,
}

/// Fim antecipado da análise das opções: o código de saída.
enum Stop {
    /// Já escreveu tudo o que tinha pra escrever (`-V`, `-h`).
    Done(i32),
    /// Erro de opção: a mensagem do getopt e depois o uso, no stderr, com 255.
    Usage(String),
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let args = args.to_vec();
    sysio::run(move || run(&args))
}

/// Aplica uma opção (a letra da curta) ao pedido. `-h` e `-V` escrevem e terminam.
fn apply(letter: u8, value: Option<OsString>, req: &mut Request) -> Result<(), Stop> {
    match letter {
        b'a' => req.display = Some(Display::Alias),
        b'A' => req.display = Some(Display::AllFqdns),
        b'b' => req.boot = true,
        b'd' => req.display = Some(Display::Domain),
        b'f' => req.display = Some(Display::Fqdn),
        b'F' => req.file = value,
        b'i' => req.display = Some(Display::Ip),
        b'I' => req.display = Some(Display::AllIp),
        b's' => req.display = Some(Display::Short),
        b'y' => req.display = Some(Display::Yp),
        b'h' => {
            write_stdout(USAGE.as_bytes());
            return Err(Stop::Done(255));
        }
        b'V' => {
            write_stdout(b"hostname 3.25\n");
            return Err(Stop::Done(0));
        }
        _ => {}
    }
    Ok(())
}

/// A opção longa de `--nome[=valor]` (já sem o `--`) pelo nome exato ou por abreviação única, nas
/// mensagens do `getopt_long` da glibc. Devolve a entrada da tabela.
fn find_long(prog: &str, typed: &str, name: &str) -> Result<(&'static str, u8, bool), Stop> {
    if let Some(entry) = LONG_OPTIONS.iter().find(|(n, _, _)| *n == name) {
        return Ok(*entry);
    }
    let candidates: Vec<&(&str, u8, bool)> =
        LONG_OPTIONS.iter().filter(|(n, _, _)| n.starts_with(name)).collect();
    match candidates.as_slice() {
        [] => Err(Stop::Usage(format!("{prog}: unrecognized option '{typed}'"))),
        [first, rest @ ..] => {
            // Candidatas que dão na mesma opção (mesma letra e mesmo argumento) não são ambíguas.
            if rest.iter().all(|c| c.1 == first.1 && c.2 == first.2) {
                Ok(**first)
            } else {
                let mut list = String::new();
                for (n, _, _) in &candidates {
                    list.push_str(&format!(" '--{n}'"));
                }
                Err(Stop::Usage(format!(
                    "{prog}: option '{typed}' is ambiguous; possibilities:{list}"
                )))
            }
        }
    }
}

/// A análise das opções como o `getopt_long` da glibc: aglomerados de curtas, longas abreviadas,
/// `--` e operandos entre as opções.
fn parse(argv: &[OsString]) -> Result<Request, Stop> {
    let prog = argv
        .first()
        .map_or_else(|| "hostname".to_string(), |a| a.to_string_lossy().into_owned());
    let mut req = Request {
        display: None,
        boot: false,
        file: None,
        operands: Vec::new(),
    };
    let mut only_operands = false;
    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        let bytes = arg.as_bytes();
        if only_operands || bytes == b"-" || !bytes.starts_with(b"-") {
            req.operands.push(arg.clone());
            continue;
        }
        if bytes == b"--" {
            only_operands = true;
            continue;
        }
        if let Some(body) = bytes.strip_prefix(b"--") {
            let typed = format!("--{}", String::from_utf8_lossy(body));
            let (name, inline_value) = match body.iter().position(|&b| b == b'=') {
                Some(eq) => (&body[..eq], Some(&body[eq + 1..])),
                None => (body, None),
            };
            let name = String::from_utf8_lossy(name).into_owned();
            let (full, letter, takes_arg) = find_long(&prog, &typed, &name)?;
            let value = if takes_arg {
                match inline_value {
                    Some(v) => Some(OsString::from(std::ffi::OsStr::from_bytes(v))),
                    None if i < argv.len() => {
                        i += 1;
                        Some(argv[i - 1].clone())
                    }
                    None => {
                        return Err(Stop::Usage(format!(
                            "{prog}: option '--{full}' requires an argument"
                        )));
                    }
                }
            } else if inline_value.is_some() {
                return Err(Stop::Usage(format!(
                    "{prog}: option '--{full}' doesn't allow an argument"
                )));
            } else {
                None
            };
            apply(letter, value, &mut req)?;
            continue;
        }
        // Aglomerado de opções curtas; a letra do -F leva o resto do aglomerado (ou o próximo
        // argumento) como valor.
        let cluster = &bytes[1..];
        for (k, &letter) in cluster.iter().enumerate() {
            match letter {
                b'a' | b'A' | b'b' | b'd' | b'f' | b'h' | b'i' | b'I' | b's' | b'V' | b'y' => {
                    apply(letter, None, &mut req)?;
                }
                b'F' => {
                    let rest = &cluster[k + 1..];
                    let value = if !rest.is_empty() {
                        OsString::from(std::ffi::OsStr::from_bytes(rest))
                    } else if i < argv.len() {
                        i += 1;
                        argv[i - 1].clone()
                    } else {
                        return Err(Stop::Usage(format!(
                            "{prog}: option requires an argument -- 'F'"
                        )));
                    };
                    apply(b'F', Some(value), &mut req)?;
                    break;
                }
                other => {
                    return Err(Stop::Usage(format!(
                        "{prog}: invalid option -- '{}'",
                        String::from_utf8_lossy(&[other])
                    )));
                }
            }
        }
    }
    Ok(req)
}

fn write_stdout(bytes: &[u8]) {
    use sysio::io::Write as _;
    let _ = sysio::io::stdout().write_all(bytes);
}

fn fail(message: &str) -> i32 {
    sysio::eprintln!("{}: {message}", prog_name());
    1
}

/// O nome com que o programa foi chamado, sem diretório: é o prefixo das mensagens de erro. Por
/// thread, porque todos os programas dividem o mesmo processo hospedeiro.
thread_local! {
    static PROG: std::cell::RefCell<String> = std::cell::RefCell::new("hostname".to_string());
}

fn prog_name() -> String {
    PROG.with(|p| p.borrow().clone())
}

/// O papel que o nome do link dá ao programa (`{yp,nis,}domainname` é o `-y`, `dnsdomainname` é
/// o `-d`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Host,
    Nis,
    Dns,
}

fn mode_of(argv0: &[u8]) -> Mode {
    let base = argv0.rsplit(|&b| b == b'/').next().unwrap_or(argv0);
    match base {
        b"domainname" | b"nisdomainname" | b"ypdomainname" => Mode::Nis,
        b"dnsdomainname" => Mode::Dns,
        _ => Mode::Host,
    }
}

fn usage_error(message: Option<&str>) -> i32 {
    if let Some(message) = message {
        sysio::eprintln!("{message}");
    }
    sysio::eprint!("{USAGE}");
    255
}

fn run(argv: &[OsString]) -> i32 {
    let argv0 = argv.first().map(|a| a.as_bytes().to_vec()).unwrap_or_default();
    let base = argv0.rsplit(|&b| b == b'/').next().unwrap_or(&argv0);
    PROG.with(|p| *p.borrow_mut() = String::from_utf8_lossy(base).into_owned());
    let mode = mode_of(&argv0);
    let req = match parse(argv) {
        Ok(req) => req,
        Err(Stop::Done(code)) => return code,
        Err(Stop::Usage(message)) => return usage_error(Some(message.as_str())),
    };
    // Nome a mais, ou nome junto de uma opção de exibição ou de -F: o texto de uso. Como
    // `domainname`, o `-d` também é uso errado.
    if req.operands.len() > 1
        || (!req.operands.is_empty() && (req.display.is_some() || req.file.is_some()))
        || (req.file.is_some() && req.display.is_some())
        || (mode == Mode::Nis && req.display == Some(Display::Domain))
    {
        return usage_error(None);
    }
    if mode == Mode::Nis {
        if let Some(file) = &req.file {
            let data = match sysio::fs::read(std::path::Path::new(file)) {
                Ok(data) => data,
                Err(e) if e.raw_os_error() == Some(sysio::errno::EISDIR) => Vec::new(),
                Err(e) => return fail(&sysio::errno::strerror(&e)),
            };
            return set_domain(&first_name(&data));
        }
        if let Some(name) = req.operands.first() {
            return set_domain(name.as_bytes());
        }
        // Sem opção, o `domainname` mostra o domínio como o kernel guarda, `(none)` incluído (o
        // `nisdomainname` e o `ypdomainname` são o `-y`).
        if req.display.is_none() && prog_name() == "domainname" {
            let mut domain = sysio::unistd::uname().domainname;
            domain.push(b'\n');
            write_stdout(&domain);
            return 0;
        }
    }
    if let Some(file) = &req.file {
        return set_from_file(std::path::Path::new(file));
    }
    if let Some(name) = req.operands.first() {
        return set_name(name.as_bytes());
    }
    let current = sysio::unistd::gethostname();
    if req.boot && (current.is_empty() || current == b"(none)") {
        return set_default_name();
    }
    let default = match mode {
        Mode::Dns => Display::Domain,
        Mode::Nis if prog_name() != "domainname" => Display::Yp,
        _ => Display::Plain,
    };
    show(req.display.unwrap_or(default), &current)
}

/// `{yp,nis,}domainname nome`: troca o domínio NIS.
fn set_domain(name: &[u8]) -> i32 {
    match sysio::unistd::setdomainname(name) {
        Ok(()) => 0,
        Err(e) => match e.raw_os_error() {
            Some(n) if n == sysio::errno::EPERM => fail("you must be root to change the domain name"),
            Some(n) if n == sysio::errno::EINVAL => fail("name too long"),
            _ => fail(&sysio::errno::strerror(&e)),
        },
    }
}

/// O nome da primeira linha útil de um arquivo de nome: sem as linhas vazias nem as de comentário
/// (`#`), com os espaços das pontas cortados. Sem nenhuma, vazio.
fn first_name(data: &[u8]) -> Vec<u8> {
    for line in data.split(|&b| b == b'\n') {
        let line = line.trim_ascii();
        if !line.is_empty() && !line.starts_with(b"#") {
            return line.to_vec();
        }
    }
    Vec::new()
}

/// `-F arquivo`: o nome vem do arquivo. Diretório conta como arquivo sem linhas (o `fgets` do
/// original falha e nada é lido); os outros erros de abertura saem como o strerror.
fn set_from_file(path: &std::path::Path) -> i32 {
    let data = match sysio::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.raw_os_error() == Some(sysio::errno::EISDIR) => Vec::new(),
        Err(e) => return fail(&sysio::errno::strerror(&e)),
    };
    set_name(&first_name(&data))
}

/// `-b` sem nome e com a máquina sem nome: o do `/etc/hostname`, ou `localhost`.
fn set_default_name() -> i32 {
    let from_file = sysio::fs::read("/etc/hostname")
        .map(|data| first_name(&data))
        .unwrap_or_default();
    if from_file.is_empty() {
        set_name(b"localhost")
    } else {
        set_name(&from_file)
    }
}

/// Um nome de máquina válido: rótulos separados por ponto, cada um não vazio, de letras, dígitos e
/// hífen, sem hífen nas pontas.
fn valid_hostname(name: &[u8]) -> bool {
    !name.is_empty()
        && name.split(|&b| b == b'.').all(|label| {
            !label.is_empty()
                && label.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-')
                && label[0] != b'-'
                && label[label.len() - 1] != b'-'
        })
}

fn set_name(name: &[u8]) -> i32 {
    if !valid_hostname(name) {
        return fail("the specified hostname is invalid");
    }
    match sysio::unistd::sethostname(name) {
        Ok(()) => 0,
        Err(e) => match e.raw_os_error() {
            Some(n) if n == sysio::errno::EPERM => fail("you must be root to change the host name"),
            Some(n) if n == sysio::errno::EINVAL => fail("name too long"),
            _ => fail(&sysio::errno::strerror(&e)),
        },
    }
}

/// O que o `gethostbyname` da glibc acha pro nome no `/etc/hosts`.
struct HostEntry {
    canonical: String,
    aliases: Vec<String>,
    addresses: Vec<String>,
}

/// Procura o nome no `/etc/hosts`: as linhas com o nome entre os nomes (sem distinguir caixa),
/// o primeiro nome da primeira é o canônico, e os demais nomes e os das outras linhas (`multi on`)
/// são apelidos; os endereços vêm de todas, na ordem do arquivo.
fn lookup(name: &[u8]) -> Option<HostEntry> {
    let name = String::from_utf8_lossy(name).into_owned();
    let hosts = sysio::fs::read("/etc/hosts").unwrap_or_default();
    let mut entry: Option<HostEntry> = None;
    for line in String::from_utf8_lossy(&hosts).lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut fields = line.split_whitespace();
        let Some(address) = fields.next() else { continue };
        let names: Vec<&str> = fields.collect();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let Ok(address) = address.parse::<std::net::IpAddr>() else { continue };
        match entry.as_mut() {
            None => {
                entry = Some(HostEntry {
                    canonical: names[0].to_string(),
                    aliases: names[1..].iter().map(|n| (*n).to_string()).collect(),
                    addresses: vec![address.to_string()],
                });
            }
            Some(found) => {
                found.aliases.extend(names.iter().map(|n| (*n).to_string()));
                found.addresses.push(address.to_string());
            }
        }
    }
    entry
}

/// Escreve o que a exibição pedida mostra do nome `current` da máquina.
fn show(display: Display, current: &[u8]) -> i32 {
    match display {
        Display::Plain => {
            write_stdout(current);
            write_stdout(b"\n");
            0
        }
        Display::Short => {
            let end = current.iter().position(|&b| b == b'.').unwrap_or(current.len());
            write_stdout(&current[..end]);
            write_stdout(b"\n");
            0
        }
        // Sem interface de rede além do loopback não há endereço pra listar.
        Display::AllIp | Display::AllFqdns => {
            write_stdout(b"\n");
            0
        }
        Display::Yp => {
            let domain = sysio::unistd::uname().domainname;
            if domain.is_empty() || domain == b"(none)" {
                // O original escreve este aviso no stdout, diferente dos outros.
                write_stdout(format!("{}: Local domain name not set\n", prog_name()).as_bytes());
                return 1;
            }
            write_stdout(&domain);
            write_stdout(b"\n");
            0
        }
        Display::Alias | Display::Domain | Display::Fqdn | Display::Ip => {
            let Some(entry) = lookup(current) else {
                return fail("Temporary failure in name resolution");
            };
            match display {
                Display::Fqdn => write_stdout(format!("{}\n", entry.canonical).as_bytes()),
                Display::Domain => {
                    if let Some(dot) = entry.canonical.find('.') {
                        write_stdout(format!("{}\n", &entry.canonical[dot + 1..]).as_bytes());
                    }
                }
                Display::Ip => write_stdout(format!("{}\n", entry.addresses.join(" ")).as_bytes()),
                _ => write_stdout(format!("{}\n", entry.aliases.join(" ")).as_bytes()),
            }
            0
        }
    }
}
