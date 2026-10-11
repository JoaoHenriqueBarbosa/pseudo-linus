//! `bun run SCRIPT`: roda um script do `package.json` num `bash -c`, como o bun 1.4.2 (tudo medido no oráculo).
//!
//! O script roda no diretório do pacote, com `pre<nome>` antes e `post<nome>` depois (cada um se existe), o `$ comando`
//! ecoado no stderr (menos com `--silent`), os argumentos extras anexados só ao script chamado e o ambiente que o bun
//! monta: `npm_*`, `NODE` e o `PATH` com o diretório do `node` e os `node_modules/.bin`.

use sysabi::{sys, Fd, KillTarget, Signal};
use sysio::process::Command;
use zjsc::runtime::process_shape::{BUN_REVISION, BUN_VERSION, EXEC_PATH, NODE_VERSION};

use crate::help;
use crate::package_json::Package;

/// Os caracteres que obrigam o argumento a ir entre aspas duplas na linha do script.
const NEEDS_QUOTES: &str = " \t\n\"'$\\`*?~[]{}()<>|&;,=#";

/// Entre aspas duplas, com `"`, `\`, `$` e `` ` `` escapados.
fn append_argument(command: &mut String, argument: &str) {
    command.push(' ');
    if !argument.is_empty() && !argument.chars().any(|c| NEEDS_QUOTES.contains(c)) {
        command.push_str(argument);
        return;
    }
    command.push('"');
    for c in argument.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            command.push('\\');
        }
        command.push(c);
    }
    command.push('"');
}

/// O texto que `bun run` sem script mostra depois da ajuda: a lista dos scripts do pacote.
pub fn listing(package: Option<&Package>) -> String {
    let mut text = format!("{}\n", help::RUN);
    match package {
        None => text.push_str("No package.json found.\n"),
        Some(package) if package.scripts.is_empty() => text.push_str("No \"scripts\" found in package.json.\n"),
        Some(package) => {
            text.push_str(&format!("package.json scripts ({} found):\n", package.scripts.len()));
            for (name, body) in &package.scripts {
                text.push_str(&format!("  $ bun run {name}\n    {body}\n\n"));
            }
        }
    }
    text
}

/// A descrição do sinal na mensagem de morte do script (a tabela do bun; `None` para o que ele não nomeia).
fn describe(signal: i32) -> Option<&'static str> {
    Some(match signal {
        1 => "Terminal hung up",
        4 => "Illegal instruction",
        5 => "Trace or breakpoint trap",
        6 => "Abort",
        7 => "Misaligned address error",
        8 => "Floating point exception",
        9 => "Forced quit",
        10 => "User defined signal 1",
        11 => "Address boundary error",
        12 => "User defined signal 2",
        13 => "Broken pipe",
        14 => "Timer expired",
        15 => "Polite quit request",
        24 => "CPU time limit exceeded",
        25 => "File size limit exceeded",
        26 => "Virtual timefr expired",
        27 => "Profiling timer expired",
        29 => "I/O on asynchronous file descriptor is possible",
        30 => "Power failure",
        31 => "Bad system call",
        _ => return None,
    })
}

fn write_stderr(text: &str) {
    let _ = sys::write_all(Fd::STDERR, text.as_bytes());
}

/// O diretório com `node` e `bun` apontando para o executável (`/tmp/bun-node-<revisão>`), criado se falta.
fn node_directory() -> String {
    let dir = format!("/tmp/bun-node-{}", &BUN_REVISION[..9]);
    if !sysio::fs::exists(&dir) {
        let _ = sysio::fs::DirBuilder::new().mode(0o700).create(&dir);
    }
    for name in ["node", "bun"] {
        let link = format!("{dir}/{name}");
        if sysio::fs::symlink_metadata(&link).is_err() {
            let _ = sysio::fs::symlink(EXEC_PATH, &link);
        }
    }
    dir
}

/// O `PATH` do script: o diretório do `node`, o `.bin` do pacote, o `.bin` de cada diretório de `cwd` até a raiz, e o `PATH` herdado.
fn script_path(package: &Package, node_dir: &str, cwd: &str) -> String {
    let bin = |dir: &str| format!("{}/node_modules/.bin", dir.trim_end_matches('/'));
    let mut entries = vec![node_dir.to_string(), bin(&package.dir)];
    let mut dir = cwd.trim_end_matches('/').to_string();
    loop {
        entries.push(bin(&dir));
        if dir.is_empty() {
            break;
        }
        dir.truncate(dir.rfind('/').unwrap_or(0));
    }
    if let Some(inherited) = sysio::env::var_os("PATH") {
        entries.push(inherited.to_string_lossy().into_owned());
    }
    entries.join(":")
}

/// Roda `body` (o script `name`) no `bash -c`; `Err(código)` é o que `bun run` devolve ao falhar.
fn execute(package: &Package, name: &str, body: &str, command: &str, silent: bool) -> Result<(), i32> {
    if !silent {
        write_stderr(&format!("$ {command}\n"));
    }
    let node_dir = node_directory();
    let cwd = sysio::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_else(|_| String::from("/"));
    let shell = ["/usr/bin/bash", "/bin/bash", "/bin/sh"].into_iter().find(|path| sysio::fs::exists(path)).unwrap_or("/bin/sh");
    let mut process = Command::new(shell);
    process
        .arg("-c")
        .arg(command)
        .current_dir(&package.dir)
        .env("PATH", script_path(package, &node_dir, &cwd))
        .env("NODE", &node_dir)
        .env("npm_command", "run-script")
        .env("npm_config_local_prefix", &package.dir)
        .env("npm_config_user_agent", format!("bun/{BUN_VERSION} npm/? node/v{NODE_VERSION} linux x64"))
        .env("npm_execpath", EXEC_PATH)
        .env("npm_lifecycle_event", name)
        .env("npm_lifecycle_script", body)
        .env("npm_node_execpath", &node_dir)
        .env("npm_package_json", format!("{}/package.json", package.dir.trim_end_matches('/')));
    if let Some(value) = &package.name {
        process.env("npm_package_name", value);
    }
    if let Some(value) = &package.version {
        process.env("npm_package_version", value);
    }
    let status = match process.status() {
        Ok(status) => status.wait_status(),
        Err(error) => {
            write_stderr(&format!("error: Failed to run script \"{name}\" due to error {error}\n"));
            return Err(1);
        }
    };
    match status {
        sysabi::WaitStatus::Exited(0) => Ok(()),
        sysabi::WaitStatus::Exited(code) => {
            // O bun não explica o código 2 (medido): sai com ele em silêncio.
            if code != 2 {
                write_stderr(&format!("error: script \"{name}\" exited with code {code}\n"));
            }
            Err(code)
        }
        sysabi::WaitStatus::Signaled { signal, .. } => {
            // SIGINT e SIGQUIT não têm mensagem; o bun morre pelo mesmo sinal depois de explicar o que viu.
            if !matches!(signal.0, 2 | 3) {
                let detail = match (signal.name(), describe(signal.0)) {
                    (Some(short), Some(text)) => format!("SIG{short} ({text})"),
                    _ => format!("code {}", signal.0),
                };
                write_stderr(&format!("error: script \"{name}\" was terminated by signal {detail}\n"));
            }
            let current = sys::current();
            let _ = current.kill(KillTarget::Pid(current.getpid()), Signal(signal.0));
            Err(128 + signal.0)
        }
        _ => Err(1),
    }
}

/// `bun run NAME ARGS...` para um script do pacote; devolve o código de saída.
pub fn run(package: &Package, name: &str, arguments: &[String], silent: bool) -> i32 {
    let mut steps = Vec::new();
    for hook in [format!("pre{name}"), name.to_string(), format!("post{name}")] {
        if let Some(body) = package.script(&hook) {
            let mut command = body.to_string();
            if hook == name {
                arguments.iter().for_each(|argument| append_argument(&mut command, argument));
            }
            steps.push((hook, body.to_string(), command));
        }
    }
    for (hook, body, command) in &steps {
        if let Err(code) = execute(package, hook, body, command, silent) {
            return code;
        }
    }
    0
}
