//! O que o plano v2 pretende ter embutido. Serve pra medir quanto do trabalho real de agentes rodaria
//! no pseudo-linus e o que mais faz falta.

/// Builtins e palavras do shell.
pub const SHELL_BUILTINS: &[&str] = &[
    "cd", "pwd", "echo", "printf", "read", "test", "[", "exit", "return", "break", "continue", "eval",
    "source", ".", "exec", "command", "type", "hash", "alias", "unalias", "wait", "jobs", "fg", "bg",
    "kill", "umask", "ulimit", "getopts", "mapfile", "readarray", "pushd", "popd", "dirs", "true",
    "false", ":", "set", "unset", "export", "local", "declare", "typeset", "readonly", "shift", "trap",
    "let", "shopt", "builtin", "enable", "help", "history", "times", "disown", "compgen", "complete",
    "caller", "bash", "sh",
];

/// coreutils (lista do GNU coreutils 9.x).
pub const COREUTILS: &[&str] = &[
    "arch", "b2sum", "base32", "base64", "basename", "basenc", "cat", "chgrp", "chmod", "chown",
    "cksum", "comm", "cp", "csplit", "cut", "date", "dd", "df", "dir", "dircolors", "dirname", "du",
    "env", "expand", "expr", "factor", "fmt", "fold", "groups", "head", "hostid", "id", "install",
    "join", "link", "ln", "logname", "ls", "md5sum", "mkdir", "mkfifo", "mknod", "mktemp", "mv",
    "nice", "nl", "nohup", "nproc", "numfmt", "od", "paste", "pathchk", "pr", "printenv", "ptx",
    "readlink", "realpath", "rm", "rmdir", "seq", "sha1sum", "sha224sum", "sha256sum", "sha384sum",
    "sha512sum", "shred", "shuf", "sleep", "sort", "split", "stat", "stdbuf", "stty", "sum", "sync",
    "tac", "tail", "tee", "timeout", "touch", "tr", "truncate", "tsort", "tty", "uname", "unexpand",
    "uniq", "unlink", "users", "vdir", "wc", "who", "whoami", "yes",
];

/// Demais ferramentas planejadas no design v2.
pub const OTHER_PLANNED: &[&str] = &[
    "find", "xargs", "grep", "egrep", "fgrep", "zgrep", "rg", "sed", "awk", "gawk", "mawk", "jq", "yq",
    "diff", "cmp", "diff3", "sdiff", "patch", "tar", "gzip", "gunzip", "zcat", "bzip2", "bunzip2",
    "bzcat", "xz", "unxz", "xzcat", "zstd", "unzstd", "zip", "unzip", "file", "bc", "git", "sqlite3",
    "curl", "wget", "ps", "top", "free", "pkill", "pgrep", "uptime", "watch", "column", "tree", "less",
    "more", "which", "hostname", "xxd", "hexdump", "strings", "envsubst", "time", "clear", "tput",
    "iconv", "lsof", "killall", "sudo",
];

pub fn is_planned(name: &str) -> bool {
    SHELL_BUILTINS.contains(&name) || COREUTILS.contains(&name) || OTHER_PLANNED.contains(&name)
}

/// Nomes que não são "um comando que falta", mas categorias do nosso normalizador.
pub fn is_meta(name: &str) -> bool {
    name.starts_with('<')
}

/// Grupos de ferramentas ausentes por natureza (o plano diz que não existem).
pub fn absent_group(name: &str) -> &'static str {
    match name {
        "python" | "python3" | "pip" | "pip3" | "uv" | "uvx" | "pytest" | "poetry" => "python",
        "node" | "npm" | "npx" | "pnpm" | "yarn" | "bun" | "bunx" | "deno" | "tsc" => "javascript",
        "cargo" | "rustc" | "rustup" | "rustfmt" | "clippy-driver" | "dx" => "rust",
        "go" | "gofmt" => "go",
        "java" | "javac" | "mvn" | "gradle" | "./gradlew" | "kotlin" | "kotlinc" => "jvm",
        "flutter" | "dart" => "flutter",
        "docker" | "podman" | "docker-compose" | "kubectl" | "helm" => "containers",
        "systemctl" | "journalctl" | "systemd-run" | "loginctl" => "systemd",
        "apt" | "apt-get" | "dpkg" | "apt-cache" | "snap" | "flatpak" => "pacotes",
        "ssh" | "scp" | "rsync" | "sshpass" | "nc" | "netcat" | "ping" | "dig" | "nslookup" | "host"
        | "getent" | "ip" | "ss" | "netstat" | "nmap" => "rede",
        "gh" | "glab" => "forges",
        "make" | "cmake" | "gcc" | "g++" | "cc" | "clang" | "ld" | "ninja" => "build C",
        "adb" | "emulator" | "sdkmanager" | "avdmanager" | "aapt" | "apksigner" | "zipalign" => "android",
        "psql" | "mysql" | "redis-cli" | "mongosh" | "pg_dump" => "bancos",
        "ffmpeg" | "ffprobe" | "convert" | "magick" | "identify" | "pdftotext" | "pdftoppm" | "qpdf" => "mídia",
        "chromium" | "google-chrome" | "xdg-open" | "xdotool" | "wmctrl" | "i3-msg" | "notify-send" => "desktop",
        "lp" | "lpr" | "lpstat" | "cancel" | "scanimage" => "impressão",
        _ => "outros",
    }
}
