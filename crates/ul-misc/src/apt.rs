//! `apt`, `apt-get`, `apt-cache`, `apt-config`, `apt-mark` e `apt-cdrom` do apt 3.0.3 (Debian 13).
//!
//! Cobre `--help`/`--version`, a análise de opções no estilo do `CommandLine` do apt (erro `E: Command
//! line option ... is not understood`), `apt-config dump`/`shell` sobre a configuração padrão do
//! Debian 13, `apt-cache` sobre um cache vazio, `apt-mark show*` sobre `/var/lib/dpkg/status` e
//! `extended_states`, e os erros de trava e de pacote que o original dá sem rede nem listas. Nada é
//! baixado nem instalado: não há rede no sandbox.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;

const APT_VERSION: &str = "3.0.3";
const ARCH: &str = "amd64";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Apt,
    AptGet,
    AptCache,
    AptConfig,
    AptMark,
    AptCdrom,
}

impl Tool {
    fn bool_shorts(self) -> &'static str {
        match self {
            Tool::Apt | Tool::AptGet => "hvqyfmubdsV",
            Tool::AptCache => "hvqifagn",
            Tool::AptConfig => "h",
            Tool::AptMark => "hv",
            Tool::AptCdrom => "hrmfa",
        }
    }

    fn arg_shorts(self) -> &'static str {
        match self {
            Tool::Apt | Tool::AptGet => "cota",
            Tool::AptCache => "cops",
            Tool::AptConfig => "co",
            Tool::AptMark => "cof",
            Tool::AptCdrom => "cod",
        }
    }
}

const HELP_APT_GET: &str = "Usage: apt-get [options] command
       apt-get [options] install|remove pkg1 [pkg2 ...]
       apt-get [options] source pkg1 [pkg2 ...]

apt-get is a command line interface for retrieval of packages
and information about them from authenticated sources and
for installation, upgrade and removal of packages together
with their dependencies.

Most used commands:
  update - Retrieve new lists of packages
  upgrade - Perform an upgrade
  install - Install new packages (pkg is libc6 not libc6.deb)
  reinstall - Reinstall packages (pkg is libc6 not libc6.deb)
  remove - Remove packages
  purge - Remove packages and config files
  autoremove - Remove automatically all unused packages
  dist-upgrade - Distribution upgrade, see apt-get(8)
  dselect-upgrade - Follow dselect selections
  build-dep - Configure build-dependencies for source packages
  satisfy - Satisfy dependency strings
  clean - Erase downloaded archive files
  autoclean - Erase old downloaded archive files
  check - Verify that there are no broken dependencies
  source - Download source archives
  download - Download the binary package into the current directory
  changelog - Download and display the changelog for the given package

See apt-get(8) for more information about the available commands.
Configuration options and syntax are detailed in apt.conf(5).
Information about how to configure sources can be found in sources.list(5).
Package and version choices can be influenced using apt_preferences(5).
Security details are available in apt-secure(8).
                                        This APT has Super Cow Powers.
";

const HELP_APT: &str = "Usage: apt [options] command

apt is a commandline package manager and provides commands for
searching and managing as well as querying information about packages.
It provides the same functionality as the specialized APT tools,
like apt-get and apt-cache, but enables options more suitable for
interactive use by default.

Most used commands:
  list - list packages based on package names
  search - search in package descriptions
  show - show package details
  install - install packages
  reinstall - reinstall packages
  remove - remove packages
  autoremove - automatically remove all unused packages
  update - update list of available packages
  upgrade - upgrade the system by installing/upgrading packages
  full-upgrade - upgrade the system by removing/installing/upgrading packages
  edit-sources - edit the source information file
  satisfy - satisfy dependency strings

See apt(8) for more information about the available commands.
Configuration options and syntax are detailed in apt.conf(5).
Information about how to configure sources can be found in sources.list(5).
Package and version choices can be influenced using apt_preferences(5).
Security details are available in apt-secure(8).
                                        This APT has Super Cow Powers.
";

const HELP_APT_CACHE: &str = "Usage: apt-cache [options] command
       apt-cache [options] show pkg1 [pkg2 ...]

apt-cache queries and displays available information about installed
and installable packages. It works exclusively on the data acquired
into the local cache via the 'update' command of e.g. apt-get. The
displayed information may therefore be outdated if the last update was
too long ago, but in exchange apt-cache works independently of the
availability of the configured sources (e.g. offline).

Most used commands:
  showsrc - Show source records
  search - Search the package list for a regex pattern
  depends - Show raw dependency information for a package
  rdepends - Show reverse dependency information for a package
  show - Show a readable record for the package
  pkgnames - List the names of all packages in the system
  policy - Show policy settings

See the apt-cache(8) manual page for more information about the available commands.
Configuration options and syntax are detailed in apt.conf(5).
Information about how to configure sources can be found in sources.list(5).
Package and version choices can be influenced using apt_preferences(5).
Security details are available in apt-secure(8).
";

const HELP_APT_CONFIG: &str = "Usage: apt-config [options] command

apt-config is an interface to the configuration settings used by
all APT tools, mainly it is used for debugging and shell scripting.

Commands:
   shell - Shell mode
   dump - Show the configuration

Options:
  -h  This help text.
  -c=? Read this configuration file
  -o=? Set an arbitrary configuration option, eg -o dir::cache=/tmp
See the apt-config(8) manual page for more information.
";

const HELP_APT_MARK: &str = "Usage: apt-mark [options] {auto|manual} pkg1 [pkg2 ...]

apt-mark is a simple command line interface for marking packages
as manually or automatically installed. It can also list marks.

Commands:
   auto - Mark the given packages as automatically installed
   manual - Mark the given packages as manually installed
   minimize-manual - Mark all dependencies of meta packages as automatically installed.
   showauto - Print the list of automatically installed packages
   showmanual - Print the list of manually installed packages
   hold - Mark a package as held back
   unhold - Unset a package set as held back
   showhold - Print the list of packages on hold
   install - Mark a package as selected for installation
   remove - Mark a package as selected for removal
   purge - Mark a package as selected for removal including configuration files
   showinstall - Print the list of packages selected for installation
   showremove - Print the list of packages selected for removal
   showpurge - Print the list of packages selected for removal including configuration files

Options:
  -h  This help text.
  -f  read/write auto/manual marking in the given file
  -c=? Read this configuration file
  -o=? Set an arbitrary configuration option, eg -o dir::cache=/tmp
See the apt-mark(8) and apt.conf(5) manual pages for more information.
";

const HELP_APT_CDROM: &str = "Usage: apt-cdrom [options] command

apt-cdrom is used to add CDROM's to APT's source list. The
mount point and device information is taken from apt.conf
and /etc/fstab.

Commands:
   add - Add a CDROM
   ident - Report the identity of a CDROM

Options:
  -h   This help text
  -d   CD-ROM mount point
  -r   Rename a recognized CD-ROM
  -m   No mounting
  -f   Fast mode, don't check package files
  -a   Thorough scan mode
  -c=? Read this configuration file
  -o=? Set an arbitrary configuration option, eg -o dir::cache=/tmp
See fstab(5)
";

const COW: &str = "         (__)
         (oo)
   /------\\/
  / |    ||
 *  /\\---/\\
    ~~   ~~
...\"Have you mooed today?\"...
";

/// Configuração padrão do Debian 13 como o `apt-config dump` a imprime: ordenada por componente,
/// sem diferenciar maiúsculas, com cada pai antes dos filhos. Uma chave terminada em `::` é um item
/// de lista.
const DEFAULT_CONFIG: &[(&str, &str)] = &[
    ("Acquire", ""),
    ("Acquire::AllowInsecureRepositories", "false"),
    ("Acquire::AllowReleaseInfoChange", ""),
    ("Acquire::AllowReleaseInfoChange::Suite", "true"),
    ("Acquire::CompressionTypes", ""),
    ("Acquire::CompressionTypes::Order", ""),
    ("Acquire::CompressionTypes::Order::", "gz"),
    ("Acquire::CompressionTypes::Order::", "lz4"),
    ("Acquire::CompressionTypes::Order::", "xz"),
    ("Acquire::CompressionTypes::Order::", "zst"),
    ("Acquire::CompressionTypes::Order::", "bz2"),
    ("Acquire::CompressionTypes::Order::", "lzma"),
    ("Acquire::Languages", ""),
    ("Acquire::Languages::", "environment"),
    ("Acquire::Languages::", "en"),
    ("Acquire::PDiffs", "true"),
    ("Acquire::Queue-Mode", "host"),
    ("Acquire::Retries", "3"),
    ("Acquire::http", ""),
    ("Acquire::http::Proxy-Auto-Detect", ""),
    ("Acquire::http::Timeout", "120"),
    ("Acquire::https", ""),
    ("Acquire::https::Timeout", "120"),
    ("APT", ""),
    ("APT::Architecture", "amd64"),
    ("APT::Architectures", ""),
    ("APT::Architectures::", "amd64"),
    ("APT::Authentication", ""),
    ("APT::Authentication::TrustCDROM", "true"),
    ("APT::AutoRemove", ""),
    ("APT::AutoRemove::RecommendsImportant", "true"),
    ("APT::AutoRemove::SuggestsImportant", "true"),
    ("APT::Build-Essential", ""),
    ("APT::Build-Essential::", "build-essential"),
    ("APT::Color", ""),
    ("APT::Cache-Limit", "0"),
    ("APT::Compressor", ""),
    ("APT::Default-Release", ""),
    ("APT::Get", ""),
    ("APT::Get::Fix-Broken", "false"),
    ("APT::Get::Show-Versions", "false"),
    ("APT::Install-Recommends", "true"),
    ("APT::Install-Suggests", "false"),
    ("APT::NeverAutoRemove", ""),
    ("APT::NeverAutoRemove::", "^firmware-linux.*"),
    ("APT::NeverAutoRemove::", "^linux-firmware$"),
    ("APT::NeverAutoRemove::", "^linux-image-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^kfreebsd-image-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-restricted-modules-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-signed-image-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-image-extra-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-modules-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-modules-extra-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-backports-modules-.*-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-tools-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^linux-headers-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^gnumach-image-[0-9]*.*"),
    ("APT::NeverAutoRemove::", "^.*-modules-[0-9]+\\.[0-9]+\\.[0-9]+.*"),
    ("APT::Never-MarkAuto-Sections", ""),
    ("APT::Never-MarkAuto-Sections::", "metapackages"),
    ("APT::Never-MarkAuto-Sections::", "contrib/metapackages"),
    ("APT::Never-MarkAuto-Sections::", "non-free/metapackages"),
    ("APT::Never-MarkAuto-Sections::", "restricted/metapackages"),
    ("APT::Never-MarkAuto-Sections::", "universe/metapackages"),
    ("APT::Never-MarkAuto-Sections::", "multiverse/metapackages"),
    ("APT::Sandbox", ""),
    ("APT::Sandbox::User", "_apt"),
    ("APT::Solver", "internal"),
    ("APT::Update", ""),
    ("APT::Update::Post-Invoke-Success", ""),
    ("APT::Update::Post-Invoke-Success::", "/usr/bin/test -e /usr/share/dbus-1/system-services/org.freedesktop.PackageKit.service && /usr/bin/test -S /var/run/dbus/system_bus_socket && /usr/bin/gdbus call --system --dest org.freedesktop.PackageKit --object-path /org/freedesktop/PackageKit --timeout 4 --method org.freedesktop.PackageKit.StateHasChanged cache-update > /dev/null; /bin/echo > /dev/null"),
    ("APT::VersionedKernelPackages", ""),
    ("APT::VersionedKernelPackages::", "linux-.*"),
    ("APT::VersionedKernelPackages::", "kfreebsd-.*"),
    ("APT::VersionedKernelPackages::", "gnumach-.*"),
    ("APT::VersionedKernelPackages::", ".*-modules"),
    ("APT::VersionedKernelPackages::", ".*-kernel"),
    ("Binary", ""),
    ("Binary::apt", ""),
    ("Binary::apt::APT", ""),
    ("Binary::apt::APT::Color", "true"),
    ("Binary::apt::APT::Get", ""),
    ("Binary::apt::APT::Get::Upgrade-Allow-New", "true"),
    ("Binary::apt::APT::Keep-Downloaded-Packages", "false"),
    ("Binary::apt::APT::Cmd", ""),
    ("Binary::apt::APT::Cmd::Show-Update-Stats", "true"),
    ("Binary::apt::DPkg", ""),
    ("Binary::apt::DPkg::Progress-Fancy", "1"),
    ("Binary::apt::Dir", ""),
    ("Binary::apt::Dir::Cache", ""),
    ("Binary::apt-get", ""),
    ("Binary::apt-get::Acquire", ""),
    ("Binary::apt-get::Acquire::AllowInsecureRepositories", "false"),
    ("CommandLine", ""),
    ("CommandLine::AsString", "apt-config dump"),
    ("Debug", ""),
    ("Debug::NoLocking", "false"),
    ("Debug::pkgDepCache", ""),
    ("Debug::pkgDepCache::AutoInstall", "false"),
    ("Dir", "/"),
    ("Dir::Aptitude", ""),
    ("Dir::Aptitude::state", "var/lib/aptitude"),
    ("Dir::Bin", ""),
    ("Dir::Bin::apt-key", "/usr/bin/apt-key"),
    ("Dir::Bin::dpkg", "/usr/bin/dpkg"),
    ("Dir::Bin::methods", "/usr/lib/apt/methods"),
    ("Dir::Bin::planners::", "/usr/lib/apt/planners"),
    ("Dir::Bin::solvers::", "/usr/lib/apt/solvers"),
    ("Dir::Cache", "var/cache/apt/"),
    ("Dir::Cache::archives", "archives/"),
    ("Dir::Cache::pkgcache", "pkgcache.bin"),
    ("Dir::Cache::srcpkgcache", "srcpkgcache.bin"),
    ("Dir::Etc", "etc/apt/"),
    ("Dir::Etc::main", "apt.conf"),
    ("Dir::Etc::netrc", "auth.conf"),
    ("Dir::Etc::netrcparts", "auth.conf.d"),
    ("Dir::Etc::parts", "apt.conf.d"),
    ("Dir::Etc::preferences", "preferences"),
    ("Dir::Etc::preferencesparts", "preferences.d"),
    ("Dir::Etc::sourceparts", "sources.list.d"),
    ("Dir::Etc::sourcelist", "sources.list"),
    ("Dir::Etc::trusted", "trusted.gpg"),
    ("Dir::Etc::trustedparts", "trusted.gpg.d"),
    ("Dir::Ignore-Files-Silently", ""),
    ("Dir::Ignore-Files-Silently::", "~$"),
    ("Dir::Ignore-Files-Silently::", "\\.disabled$"),
    ("Dir::Ignore-Files-Silently::", "\\.bak$"),
    ("Dir::Ignore-Files-Silently::", "\\.dpkg-[a-z]+$"),
    ("Dir::Ignore-Files-Silently::", "\\.ucf-[a-z]+$"),
    ("Dir::Ignore-Files-Silently::", "\\.save$"),
    ("Dir::Ignore-Files-Silently::", "\\.orig$"),
    ("Dir::Log", "var/log/apt"),
    ("Dir::Log::history", "history.log"),
    ("Dir::Log::planner", "eipp.log.xz"),
    ("Dir::Log::terminal", "term.log"),
    ("Dir::Media", ""),
    ("Dir::Media::MountPath", "/media/apt"),
    ("Dir::State", "var/lib/apt/"),
    ("Dir::State::cdroms", "cdroms.list"),
    ("Dir::State::extended_states", "extended_states"),
    ("Dir::State::lists", "lists/"),
    ("Dir::State::mirrors", "mirrors/"),
    ("Dir::State::status", "/var/lib/dpkg/status"),
    ("Dir::Strange-Lock", ""),
    ("DPkg", ""),
    ("DPkg::Options", ""),
    ("DPkg::Options::", "--force-confold"),
    ("DPkg::Pre-Install-Pkgs", ""),
    ("DPkg::Pre-Install-Pkgs::", "/usr/sbin/dpkg-preconfigure --apt || true"),
    ("DPkg::Post-Invoke", ""),
    ("DPkg::Tools", ""),
    ("DPkg::Tools::Options", ""),
    ("DPkg::Tools::Options::/usr/sbin/dpkg-preconfigure", ""),
    ("DPkg::Tools::Options::/usr/sbin/dpkg-preconfigure::Version", "3"),
    ("Pkg", ""),
    ("Pkg::Config", "false"),
];

fn out(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}

/// Escreve no stderr depois de descarregar o stdout: no apt o `cerr` está amarrado ao `cout`.
fn err(s: &str) {
    let _ = io::flush_stdout();
    io::eprint(s);
}

fn exists(path: &str) -> bool {
    sys::stat(path.as_bytes()).is_ok()
}

fn version_line() -> String {
    format!("apt {APT_VERSION} ({ARCH})\n")
}

// ---------------------------------------------------------------------------------------------
// Análise de opções
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Cli {
    /// Opções longas normalizadas (`simulate`, `help`, `version`, ...).
    flags: Vec<String>,
    config: Vec<(String, String)>,
    quiet: u32,
    operands: Vec<String>,
}

impl Cli {
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

const BOOL_LONGS: &[&str] = &[
    "help",
    "version",
    "quiet",
    "silent",
    "yes",
    "assume-yes",
    "assume-no",
    "install-recommends",
    "install-suggests",
    "simulate",
    "just-print",
    "dry-run",
    "recon",
    "no-act",
    "download-only",
    "fix-broken",
    "ignore-missing",
    "fix-missing",
    "download",
    "show-upgraded",
    "show-versions",
    "upgrade",
    "only-upgrade",
    "allow-downgrades",
    "allow-remove-essential",
    "allow-change-held-packages",
    "with-new-pkgs",
    "auto-remove",
    "autoremove",
    "purge",
    "print-uris",
    "reinstall",
    "list-cleanup",
    "trivial-only",
    "force-yes",
    "allow-unauthenticated",
    "remove",
    "ignore-hold",
    "fix-policy",
    "all-versions",
    "full",
    "names-only",
    "installed",
    "upgradeable",
    "manual-installed",
    "all",
    "generate",
    "important",
    "implicit",
    "recurse",
    "build",
    "compile",
    "diff-only",
    "tar-only",
    "dsc-only",
    "arch-only",
    "indep-only",
    "build-dep",
    "host-architecture-only",
    "reinstall-recommends",
    "color",
    "show-progress",
    "installed-only",
    "mark-auto",
    "release-info-change",
    "allow-insecure-repositories",
    "allow-weak-repositories",
    "allow-releaseinfo-change",
    "verbose",
    "show-user-simulation-note",
    "error-on-any",
    "ignore-errors",
    "print-architecture",
    "with-source",
    "fast",
    "thorough",
    "no-mount",
    "rename",
    "no-changelog",
    "no-remove",
    "no-install-recommends",
    "no-install-suggests",
];

const ARG_LONGS: &[&str] = &[
    "option",
    "config-file",
    "target-release",
    "default-release",
    "host-architecture",
    "pkg-cache",
    "src-cache",
    "cdrom",
    "build-profiles",
    "solver",
    "error-on",
    "mount",
    "file",
];

fn normalize_flag(name: &str) -> String {
    match name {
        "just-print" | "dry-run" | "recon" | "no-act" => "simulate".to_string(),
        "assume-yes" => "yes".to_string(),
        "silent" => "quiet".to_string(),
        other => other.to_string(),
    }
}

fn not_understood(shown: &str, from: &str) -> String {
    format!(
        "Command line option '{shown}' [from {from}] is not understood in combination with the other options."
    )
}

fn parse_cli(tool: Tool, args: &[String]) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let bools = tool.bool_shorts();
    let argsh = tool.arg_shorts();
    let mut i = 0;
    let mut only_operands = false;
    let mut add_config = |cli: &mut Cli, kv: &str| {
        if let Some((k, v)) = kv.split_once('=') {
            cli.config.push((k.to_string(), v.to_string()));
        }
    };
    while i < args.len() {
        let a = args[i].clone();
        i += 1;
        if only_operands || a == "-" || !a.starts_with('-') {
            cli.operands.push(a);
            continue;
        }
        if a == "--" {
            only_operands = true;
            continue;
        }
        if let Some(body) = a.strip_prefix("--") {
            let (name, val) = match body.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (body, None),
            };
            if name.contains("::") {
                cli.config
                    .push((name.to_string(), val.unwrap_or_else(|| "true".to_string())));
                continue;
            }
            if ARG_LONGS.contains(&name) {
                let v = match val {
                    Some(v) => v,
                    None => {
                        if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return Err(not_understood(&format!("--{name}"), &a));
                        }
                    }
                };
                if name == "option" {
                    add_config(&mut cli, &v);
                }
                continue;
            }
            let base = name
                .strip_prefix("no-")
                .filter(|b| BOOL_LONGS.contains(b))
                .unwrap_or(name);
            if BOOL_LONGS.contains(&base) || BOOL_LONGS.contains(&name) {
                let flag = normalize_flag(base);
                if flag == "quiet" {
                    cli.quiet += 1;
                }
                if name == base {
                    cli.flags.push(flag);
                }
                continue;
            }
            return Err(not_understood(&format!("--{name}"), &a));
        }
        let chars: Vec<char> = a[1..].chars().collect();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            k += 1;
            if argsh.contains(c) {
                let rest: String = chars[k..].iter().collect();
                let v = if !rest.is_empty() {
                    rest
                } else if i < args.len() {
                    i += 1;
                    args[i - 1].clone()
                } else {
                    return Err(not_understood(&c.to_string(), &a));
                };
                if c == 'o' {
                    add_config(&mut cli, &v);
                }
                break;
            }
            if bools.contains(c) {
                let flag = match c {
                    'h' => "help",
                    'v' => "version",
                    's' => "simulate",
                    'y' => "yes",
                    'q' => {
                        cli.quiet += 1;
                        "quiet"
                    }
                    'd' => "download-only",
                    'f' => "fix-broken",
                    'm' => "ignore-missing",
                    'u' => "show-upgraded",
                    'b' => "build",
                    'V' => "show-versions",
                    'i' => "important",
                    'a' => "all-versions",
                    'g' => "generate",
                    'n' => "names-only",
                    'r' => "rename",
                    other => {
                        cli.flags.push(other.to_string());
                        continue;
                    }
                };
                cli.flags.push(flag.to_string());
                continue;
            }
            return Err(not_understood(&c.to_string(), &a));
        }
    }
    Ok(cli)
}

// ---------------------------------------------------------------------------------------------
// Configuração (apt-config)
// ---------------------------------------------------------------------------------------------

fn key_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let ka: Vec<String> = a.split("::").map(str::to_ascii_lowercase).collect();
    let kb: Vec<String> = b.split("::").map(str::to_ascii_lowercase).collect();
    ka.cmp(&kb)
}

/// A tabela padrão com os `-o` aplicados: chave existente é substituída, nova entra na ordem.
fn effective_config(cli: &Cli) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = DEFAULT_CONFIG
        .iter()
        .map(|(k, val)| (k.to_string(), val.to_string()))
        .collect();
    for (k, val) in &cli.config {
        if k.ends_with("::") {
            let pos = v
                .iter()
                .rposition(|(kk, _)| key_cmp(kk, k).is_le())
                .map_or(0, |p| p + 1);
            v.insert(pos, (k.clone(), val.clone()));
            continue;
        }
        if let Some(e) = v.iter_mut().find(|(kk, _)| kk.eq_ignore_ascii_case(k)) {
            e.1 = val.clone();
        } else {
            let pos = v
                .iter()
                .rposition(|(kk, _)| key_cmp(kk, k).is_le())
                .map_or(0, |p| p + 1);
            v.insert(pos, (k.clone(), val.clone()));
        }
    }
    v
}

fn cfg_get(conf: &[(String, String)], key: &str) -> Option<String> {
    conf.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
}

/// `FindFile`/`FindDir`: valor relativo é resolvido contra o diretório do pai.
fn cfg_path(conf: &[(String, String)], key: &str, dir: bool) -> String {
    let v = cfg_get(conf, key).unwrap_or_default();
    let mut full = if v.starts_with('/') || key.eq_ignore_ascii_case("Dir") {
        if key.eq_ignore_ascii_case("Dir") && v.is_empty() {
            "/".to_string()
        } else {
            v
        }
    } else {
        match key.rsplit_once("::") {
            Some((parent, _)) => {
                let mut p = cfg_path(conf, parent, true);
                if !p.ends_with('/') {
                    p.push('/');
                }
                p.push_str(&v);
                p
            }
            None => v,
        }
    };
    if dir && !full.ends_with('/') {
        full.push('/');
    }
    full
}

fn apt_config(cli: &Cli) -> i32 {
    let conf = effective_config(cli);
    let Some(cmd) = cli.operands.first() else {
        out(&version_line());
        out(HELP_APT_CONFIG);
        return 0;
    };
    match cmd.as_str() {
        "dump" => {
            let mut o = String::new();
            let filter = cli.operands.get(1);
            let dump_cmd = effective_config_with_cmdline(&conf, cli);
            for (k, v) in &dump_cmd {
                if let Some(f) = filter {
                    let kl = k.to_ascii_lowercase();
                    let fl = f.to_ascii_lowercase();
                    if kl != fl && !kl.starts_with(&format!("{fl}::")) {
                        continue;
                    }
                }
                o.push_str(&format!("{k} \"{v}\";\n"));
            }
            out(&o);
            0
        }
        "shell" => {
            let rest = &cli.operands[1..];
            if rest.len() % 2 != 0 {
                // O original ignora o par incompleto.
            }
            let mut o = String::new();
            for pair in rest.chunks(2) {
                let [var, key] = pair else { break };
                let (name, flag) = match key.rsplit_once('/') {
                    Some((n, f)) => (n, f),
                    None => (key.as_str(), ""),
                };
                let found = cfg_get(&conf, name);
                let value = match flag {
                    "f" => Some(cfg_path(&conf, name, false)),
                    "d" => Some(cfg_path(&conf, name, true)),
                    "b" => Some(
                        match found.as_deref() {
                            Some("true" | "yes" | "1" | "on" | "with" | "enable") => "true",
                            _ => "false",
                        }
                        .to_string(),
                    ),
                    "i" => Some(found.unwrap_or_default().parse::<i64>().unwrap_or(0).to_string()),
                    _ => found.filter(|v| !v.is_empty()),
                };
                if let Some(v) = value {
                    o.push_str(&format!("{var}='{v}'\n"));
                }
            }
            out(&o);
            0
        }
        _ => {
            err(&format!("E: Invalid operation {cmd}\n"));
            100
        }
    }
}

fn effective_config_with_cmdline(conf: &[(String, String)], _cli: &Cli) -> Vec<(String, String)> {
    conf.to_vec()
}

// ---------------------------------------------------------------------------------------------
// Estado do dpkg (para apt-mark e apt-cache)
// ---------------------------------------------------------------------------------------------

struct Pkg {
    name: String,
    want: String,
    state: String,
}

fn read_text(path: &str) -> Option<String> {
    io::read_path(path.as_bytes())
        .ok()
        .map(|d| String::from_utf8_lossy(&d).into_owned())
}

fn dpkg_packages() -> Vec<Pkg> {
    let Some(text) = read_text("/var/lib/dpkg/status") else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for para in text.split("\n\n") {
        let mut name = String::new();
        let mut status = String::new();
        for line in para.lines() {
            if let Some(n) = line.strip_prefix("Package:") {
                name = n.trim().to_string();
            } else if let Some(s) = line.strip_prefix("Status:") {
                status = s.trim().to_string();
            }
        }
        if name.is_empty() {
            continue;
        }
        let mut it = status.split_whitespace();
        let want = it.next().unwrap_or("unknown").to_string();
        let _ = it.next();
        let state = it.next().unwrap_or("not-installed").to_string();
        v.push(Pkg { name, want, state });
    }
    v
}

fn auto_installed(path: &str) -> Vec<String> {
    let Some(text) = read_text(path) else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for para in text.split("\n\n") {
        let mut name = "";
        let mut auto = false;
        for line in para.lines() {
            if let Some(n) = line.strip_prefix("Package:") {
                name = n.trim();
            } else if let Some(a) = line.strip_prefix("Auto-Installed:") {
                auto = a.trim() == "1";
            }
        }
        if auto && !name.is_empty() {
            v.push(name.to_string());
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------
// apt-mark
// ---------------------------------------------------------------------------------------------

fn apt_mark(cli: &Cli) -> i32 {
    let Some(cmd) = cli.operands.first() else {
        out(&version_line());
        out(HELP_APT_MARK);
        return 0;
    };
    let states = cli
        .config
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Dir::State::extended_states"))
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "/var/lib/apt/extended_states".to_string());
    let pkgs = dpkg_packages();
    let mut names: Vec<String> = match cmd.as_str() {
        "showmanual" | "showauto" => {
            let auto = auto_installed(&states);
            pkgs.iter()
                .filter(|p| p.state == "installed")
                .filter(|p| auto.contains(&p.name) == (cmd == "showauto"))
                .map(|p| p.name.clone())
                .collect()
        }
        "showhold" => pkgs.iter().filter(|p| p.want == "hold").map(|p| p.name.clone()).collect(),
        "showinstall" => pkgs.iter().filter(|p| p.want == "install" && p.state != "installed").map(|p| p.name.clone()).collect(),
        "showremove" | "showpurge" => {
            let w = if cmd == "showremove" { "deinstall" } else { "purge" };
            pkgs.iter().filter(|p| p.want == w && p.state != "not-installed").map(|p| p.name.clone()).collect()
        }
        "auto" | "manual" | "hold" | "unhold" | "install" | "remove" | "purge" | "minimize-manual" => {
            return apt_mark_modify(cmd, &cli.operands[1..], &pkgs);
        }
        "help" => {
            out(&version_line());
            out(HELP_APT_MARK);
            return 0;
        }
        _ => {
            err(&format!("E: Invalid operation {cmd}\n"));
            return 100;
        }
    };
    names.sort();
    names.dedup();
    let mut o = String::new();
    for n in names {
        o.push_str(&n);
        o.push('\n');
    }
    out(&o);
    0
}

fn apt_mark_modify(cmd: &str, names: &[String], pkgs: &[Pkg]) -> i32 {
    if names.is_empty() && cmd != "minimize-manual" {
        err("E: No packages found\n");
        return 100;
    }
    let mut rc = 0;
    for n in names {
        if !pkgs.iter().any(|p| p.name == *n) {
            err(&format!("E: Unable to locate package {n}\n"));
            rc = 100;
        }
    }
    if rc == 0 {
        // Gravar `extended_states` ou a seleção do dpkg exige o banco; sem ele o original falha na trava.
        err("E: Could not open lock file /var/lib/apt/extended_states - open (13: Permission denied)\n");
        return 100;
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// apt-cache (e os comandos de consulta do apt)
// ---------------------------------------------------------------------------------------------

fn cache_not_found(names: &[String], what_missing: &str) -> i32 {
    for n in names {
        err(&format!("N: Unable to locate package {n}\n"));
    }
    err(&format!("E: {what_missing}\n"));
    100
}

fn cache_policy(names: &[String]) -> i32 {
    let mut o = String::from("Package files:\n");
    if exists("/var/lib/dpkg/status") {
        o.push_str(" 100 /var/lib/dpkg/status\n     release a=now\n");
    }
    // Pacote inexistente não imprime nada no `policy`.
    let _ = names;
    out(&o);
    0
}

fn cache_stats() -> i32 {
    let pkgs = dpkg_packages();
    let n = pkgs.len();
    let o = format!(
        "Total package names: {n} (0 B)\n  Normal packages: {n}\n  Pure virtual packages: 0\n  Single virtual packages: 0\n  Mixed virtual packages: 0\n  Missing: 0\nTotal distinct versions: {n} (0 B)\nTotal distinct descriptions: {n} (0 B)\nTotal dependencies: 0 (0 B)\nTotal ver/file relations: {n} (0 B)\nTotal Desc/File relations: {n} (0 B)\nTotal Provides mappings: 0 (0 B)\nTotal globbed strings: 0 (0 B)\nTotal dependency version space: 0 B\nTotal slack space: 0 B\nTotal space accounted for: 0 B\n"
    );
    out(&o);
    0
}

fn apt_cache_cmd(cmd: &str, rest: &[String]) -> i32 {
    match cmd {
        "policy" => cache_policy(rest),
        "stats" => cache_stats(),
        "pkgnames" | "dumpavail" | "unmet" | "dump" | "gencaches" | "xvcg" | "dotty" => 0,
        "search" => {
            if rest.is_empty() {
                err("E: You must give at least one search pattern\n");
                return 100;
            }
            0
        }
        "madison" => 0,
        "show" | "showpkg" | "depends" | "rdepends" => {
            if rest.is_empty() {
                err("E: No packages found\n");
                return 100;
            }
            cache_not_found(rest, "No packages found")
        }
        "showsrc" => {
            if rest.is_empty() {
                err("E: Unable to find a source package for\n");
                return 100;
            }
            for n in rest {
                err(&format!("E: Unable to find a source package for {n}\n"));
            }
            100
        }
        "help" => {
            out(&version_line());
            out(HELP_APT_CACHE);
            0
        }
        _ => {
            err(&format!("E: Invalid operation {cmd}\n"));
            100
        }
    }
}

fn apt_cache(cli: &Cli) -> i32 {
    match cli.operands.first() {
        None => {
            out(&version_line());
            out(HELP_APT_CACHE);
            0
        }
        Some(cmd) => apt_cache_cmd(cmd, &cli.operands[1..]),
    }
}

// ---------------------------------------------------------------------------------------------
// apt-get e apt
// ---------------------------------------------------------------------------------------------

const PROGRESS: &str = "Reading package lists...\nBuilding dependency tree...\nReading state information...\n";

fn progress(cli: &Cli) {
    if cli.quiet < 2 {
        out(PROGRESS);
    }
}

fn frontend_lock_error() -> i32 {
    err("E: Could not open lock file /var/lib/dpkg/lock-frontend - open (2: No such file or directory)\nE: Unable to acquire the dpkg frontend lock (/var/lib/dpkg/lock-frontend), is another process using it?\n");
    100
}

fn root_lock_ok(cli: &Cli) -> bool {
    cli.has("simulate") || exists("/var/lib/dpkg")
}

fn unlocatable(names: &[String]) -> i32 {
    let mut rc = 0;
    for n in names {
        err(&format!("E: Unable to locate package {n}\n"));
        rc = 100;
    }
    rc
}

fn update_cmd(cli: &Cli) -> i32 {
    if !cli.has("simulate") && !exists("/var/lib/apt/lists") {
        err("E: Could not open lock file /var/lib/apt/lists/lock - open (2: No such file or directory)\nE: Unable to lock directory /var/lib/apt/lists/\n");
        return 100;
    }
    if cli.quiet < 2 {
        out("Reading package lists...\n");
    }
    0
}

fn clean_cmd(cli: &Cli) -> i32 {
    if !cli.has("simulate") && !exists("/var/cache/apt/archives") {
        err("E: Could not open lock file /var/cache/apt/archives/lock - open (2: No such file or directory)\nE: Unable to lock directory /var/cache/apt/archives/\n");
        return 100;
    }
    0
}

/// Comandos de instalação/remoção: trava do dpkg, leitura do estado e pacotes pedidos.
fn modify_cmd(cmd: &str, rest: &[String], cli: &Cli) -> i32 {
    if !root_lock_ok(cli) {
        return frontend_lock_error();
    }
    progress(cli);
    match cmd {
        "install" | "reinstall" | "remove" | "purge" | "autopurge" => {
            let rc = unlocatable(rest);
            if rc != 0 {
                return rc;
            }
            out("0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n");
            0
        }
        "upgrade" | "dist-upgrade" | "full-upgrade" => {
            out("Calculating upgrade...\n0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n");
            0
        }
        "autoremove" => {
            out("0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n");
            0
        }
        "dselect-upgrade" => {
            out("0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n");
            0
        }
        "build-dep" | "satisfy" => {
            if rest.is_empty() {
                err(&format!(
                    "E: {}\n",
                    if cmd == "build-dep" {
                        "Must specify at least one package to check builddeps for"
                    } else {
                        "Must specify at least one dependency string"
                    }
                ));
                return 100;
            }
            if cmd == "build-dep" {
                err("E: You must put some 'deb-src' URIs in your sources.list\n");
                return 100;
            }
            unlocatable(rest)
        }
        _ => 0,
    }
}

fn run_get_like(tool: Tool, cmd: &str, rest: &[String], cli: &Cli) -> i32 {
    match cmd {
        "update" => update_cmd(cli),
        "install" | "reinstall" | "remove" | "purge" | "autopurge" | "upgrade" | "dist-upgrade"
        | "full-upgrade" | "autoremove" | "dselect-upgrade" | "build-dep" | "satisfy" => {
            if tool == Tool::AptGet && cmd == "full-upgrade" {
                // `full-upgrade` existe no apt-get também.
            }
            modify_cmd(cmd, rest, cli)
        }
        "auto-remove" => modify_cmd("autoremove", rest, cli),
        "clean" | "autoclean" | "auto-clean" => clean_cmd(cli),
        "check" => {
            progress(cli);
            0
        }
        "source" => {
            if rest.is_empty() {
                err("E: Must specify at least one package to fetch source for\n");
                return 100;
            }
            if cli.quiet < 2 {
                out("Reading package lists...\n");
            }
            err("E: You must put some 'deb-src' URIs in your sources.list\n");
            100
        }
        "download" | "changelog" => {
            if rest.is_empty() {
                err(&format!(
                    "E: {}\n",
                    if cmd == "download" {
                        "Must specify at least one package to download"
                    } else {
                        "Must specify at least one package to download changelog for"
                    }
                ));
                return 100;
            }
            progress(cli);
            unlocatable(rest)
        }
        "indextargets" => 0,
        "markauto" | "unmarkauto" => {
            progress(cli);
            unlocatable(rest)
        }
        "moo" => {
            out(COW);
            0
        }
        _ => {
            err(&format!("E: Invalid operation {cmd}\n"));
            100
        }
    }
}

fn apt_get(cli: &Cli) -> i32 {
    match cli.operands.first() {
        None => {
            out(&version_line());
            out(HELP_APT_GET);
            0
        }
        Some(c) if c == "help" => {
            out(&version_line());
            out(HELP_APT_GET);
            0
        }
        Some(cmd) => run_get_like(Tool::AptGet, cmd, &cli.operands[1..], cli),
    }
}

fn apt_cmd(cli: &Cli) -> i32 {
    let Some(cmd) = cli.operands.first() else {
        out(&version_line());
        out(HELP_APT);
        return 0;
    };
    if cmd == "help" {
        out(&version_line());
        out(HELP_APT);
        return 0;
    }
    if !io::stdout_is_tty() {
        err("\nWARNING: apt does not have a stable CLI interface. Use with caution in scripts.\n\n");
    }
    let rest = &cli.operands[1..];
    match cmd.as_str() {
        "list" => {
            if cli.quiet < 2 {
                out("Listing...\n");
            }
            0
        }
        "search" => {
            if rest.is_empty() {
                err("E: You must give at least one search pattern\n");
                return 100;
            }
            out("Sorting...\nFull Text Search...\n");
            0
        }
        "show" => {
            if rest.is_empty() {
                err("E: No packages found\n");
                return 100;
            }
            cache_not_found(rest, "No packages found")
        }
        "showsrc" | "policy" | "depends" | "rdepends" => apt_cache_cmd(cmd, rest),
        "edit-sources" => 0,
        other => run_get_like(Tool::Apt, other, rest, cli),
    }
}

// ---------------------------------------------------------------------------------------------
// apt-cdrom
// ---------------------------------------------------------------------------------------------

fn apt_cdrom(cli: &Cli) -> i32 {
    let Some(cmd) = cli.operands.first() else {
        out(&version_line());
        out(HELP_APT_CDROM);
        return 0;
    };
    match cmd.as_str() {
        "help" => {
            out(&version_line());
            out(HELP_APT_CDROM);
            0
        }
        "add" | "ident" => {
            let mount = cli
                .config
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("Acquire::cdrom::mount"))
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "/media/cdrom/".to_string());
            let mount = if mount.ends_with('/') { mount } else { format!("{mount}/") };
            out(&format!("Using CD-ROM mount point {mount}\nMounting CD-ROM...\n"));
            err(&format!("mount: {mount}: can't find in /etc/fstab.\nE: Failed to mount the cdrom.\n"));
            100
        }
        _ => {
            err(&format!("E: Invalid operation {cmd}\n"));
            100
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Entradas
// ---------------------------------------------------------------------------------------------

fn run(tool: Tool, args: &[OsString]) -> i32 {
    let argv: Vec<String> = io::args_bytes(args).iter().skip(1).map(|a| io::lossy(a)).collect();
    let cli = match parse_cli(tool, &argv) {
        Ok(c) => c,
        Err(m) => {
            err(&format!("E: {m}\n"));
            return 100;
        }
    };
    if cli.has("version") {
        out(&version_line());
        return 0;
    }
    if cli.has("help") {
        out(&version_line());
        out(match tool {
            Tool::Apt => HELP_APT,
            Tool::AptGet => HELP_APT_GET,
            Tool::AptCache => HELP_APT_CACHE,
            Tool::AptConfig => HELP_APT_CONFIG,
            Tool::AptMark => HELP_APT_MARK,
            Tool::AptCdrom => HELP_APT_CDROM,
        });
        return 0;
    }
    match tool {
        Tool::Apt => apt_cmd(&cli),
        Tool::AptGet => apt_get(&cli),
        Tool::AptCache => apt_cache(&cli),
        Tool::AptConfig => apt_config(&cli),
        Tool::AptMark => apt_mark(&cli),
        Tool::AptCdrom => apt_cdrom(&cli),
    }
}

pub fn apt_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(Tool::Apt, args))
}

pub fn apt_get_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(Tool::AptGet, args))
}

pub fn apt_cache_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(Tool::AptCache, args))
}

pub fn apt_config_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(Tool::AptConfig, args))
}

pub fn apt_mark_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(Tool::AptMark, args))
}

pub fn apt_cdrom_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(Tool::AptCdrom, args))
}
