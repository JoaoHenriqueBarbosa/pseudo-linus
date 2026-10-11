//! O driver Node do Playwright (`package/cli.js` da wheel `playwright` 1.63.0) rodando no `bun` do pseudo-linus.
//!
//! O driver não é versionado: `PW_DRIVER_DIR` aponta o diretório `package` extraído da wheel (14 MB, copiado só para o
//! VFS do sandbox, em `/pw`). Sem a variável o teste não faz nada. `PW_ARGS` troca os argumentos depois do `cli.js`
//! (o padrão é `--version`); com `PW_ARGS` o teste só mostra o resultado, sem afirmar nada (é a sonda para iterar).

mod common;

use std::path::Path;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::WaitStatus;

/// Roda `bun /pw/package/cli.js ARGS...` e devolve `(stdout, stderr, status)`.
fn run_driver(driver: &Path, args: &[&str], stdin: &[u8]) -> (String, String, WaitStatus) {
    let kernel = Kernel::new(KernelConfig::default());
    let sandbox = kernel.create_sandbox(SandboxConfig { programs: common::programs(), cwd: b"/app".to_vec(), ..SandboxConfig::default() }).expect("sandbox");
    common::install(&sandbox.fs(), "/pw/package", &common::read_tree(driver));
    sandbox.fs().mkdir_all(b"/app", 0o755).expect("/app");
    let mut argv: Vec<Vec<u8>> = vec![b"bun".to_vec(), b"/pw/package/cli.js".to_vec()];
    argv.extend(args.iter().map(|arg| arg.as_bytes().to_vec()));
    let request = RunRequest {
        argv,
        cwd: Some(b"/app".to_vec()),
        env: Some(vec![b"PW_LANG_NAME=python".to_vec(), b"PATH=/usr/local/bin:/usr/bin:/bin".to_vec(), b"HOME=/root".to_vec()]),
        stdin: stdin.to_vec(),
        timeout: Some(Duration::from_secs(120)),
        ..RunRequest::default()
    };
    let result = sandbox.run(request).expect("execução");
    (String::from_utf8_lossy(&result.stdout).into_owned(), String::from_utf8_lossy(&result.stderr).into_owned(), result.status)
}

#[test]
fn driver_prints_its_version() {
    let Some(driver) = std::env::var_os("PW_DRIVER_DIR") else {
        eprintln!("PW_DRIVER_DIR não definida: teste do driver do Playwright ignorado");
        return;
    };
    let probe = std::env::var("PW_ARGS").ok().filter(|text| !text.is_empty());
    let owned: Vec<String> = probe.as_deref().map_or_else(|| vec![String::from("--version")], |text| text.split_whitespace().map(String::from).collect());
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let (stdout, stderr, status) = run_driver(Path::new(&driver), &args, b"");
    println!("--- stdout\n{stdout}--- stderr\n{stderr}--- status {status:?}");
    if probe.is_none() {
        assert_eq!(stdout, "Version 1.63.0\n");
        assert_eq!(status, WaitStatus::Exited(0));
    }
}
