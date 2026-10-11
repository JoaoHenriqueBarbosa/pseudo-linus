//! `bun` dentro do kernel de teste do sysabi: o motor lê o VFS, o ambiente, o diretório de trabalho e o pid do
//! pseudo-processo, nunca os da máquina que roda o teste.

use sysabi::testkit::TestKit;
use sysabi::WaitStatus;

fn kit() -> TestKit {
    TestKit::new().programs(crate::programs()).dir("/app", 0o755).cwd("/app")
}

#[test]
fn eval_prints_to_the_process_stdout() {
    let result = kit().run(&["bun", "-e", "console.log(1+1)"], b"");
    assert_eq!(result.stdout_str(), "2\n");
    assert_eq!(result.stderr_str(), "");
    assert_eq!(result.status, WaitStatus::Exited(0));
}

#[test]
fn version_matches_the_engine_reference() {
    let result = kit().run(&["bun", "--version"], b"");
    assert_eq!(result.stdout_str(), "1.4.2\n");
}

#[test]
fn run_reads_the_script_and_its_imports_from_the_sandbox_vfs() {
    let kit = kit()
        .file("/app/main.mjs", "import { answer } from './dep.mjs'; console.log('answer', answer, process.argv.slice(2).join(','));", 0o644)
        .file("/app/dep.mjs", "export const answer = 6 * 7;", 0o644);
    let result = kit.run(&["bun", "run", "main.mjs", "x", "y"], b"");
    assert_eq!(result.stdout_str(), "answer 42 x,y\n", "stderr: {}", result.stderr_str());
    assert_eq!(result.status, WaitStatus::Exited(0));
}

#[test]
fn require_reads_the_sandbox_vfs() {
    let kit = kit().file("/app/main.cjs", "console.log(require('./dep.cjs').value)", 0o644).file("/app/dep.cjs", "module.exports = { value: 'from the vfs' }", 0o644);
    let result = kit.run(&["bun", "main.cjs"], b"");
    assert_eq!(result.stdout_str(), "from the vfs\n", "stderr: {}", result.stderr_str());
}

#[test]
fn environment_cwd_and_pid_are_the_sandbox_ones() {
    let kit = kit().env("PLINUS_PROBE", "inside");
    let result = kit.run(&["bun", "-e", "console.log(process.env.PLINUS_PROBE, process.cwd(), typeof process.pid)"], b"");
    assert_eq!(result.stdout_str(), "inside /app number\n", "stderr: {}", result.stderr_str());
}

#[test]
fn missing_script_fails_like_bun() {
    let result = kit().run(&["bun", "run", "nope.js"], b"");
    assert_eq!(result.stderr_str(), "error: Module not found \"nope.js\"\n");
    assert_eq!(result.status, WaitStatus::Exited(1));
}

#[test]
fn uncaught_error_exits_with_one() {
    let result = kit().run(&["bun", "-e", "throw new Error('boom')"], b"");
    assert_eq!(result.status, WaitStatus::Exited(1));
    assert!(result.stderr_str().contains("error: boom"), "stderr: {}", result.stderr_str());
}
