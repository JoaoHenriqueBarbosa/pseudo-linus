//! H36: git sobre o FS do sandbox.

pub mod cmd;
pub mod experiments;
pub mod gix_high;
pub mod store;
pub mod textdiff;

use harness::{Candidate, Invocation, Outcome};

use crate::shell::{Ctx, Programs, run_script};

/// O protótipo `gix-*` como candidato do harness (casos `script`).
pub struct GitCandidate;

struct GitPrograms;

impl Programs for GitPrograms {
    fn run(&mut self, argv: &[String], ctx: &mut Ctx<'_>) -> Option<i32> {
        (argv[0] == "git").then(|| cmd::run_git(argv, ctx))
    }
}

impl Candidate for GitCandidate {
    fn name(&self) -> String {
        "gix-* 0.65 (object) / 0.75 (pack) / 0.56 (index) + CLI nosso".to_string()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let env = inv.full_env();
        let mut fs = inv.files.clone();
        let mut ctx_out = Vec::new();
        let mut ctx_err = Vec::new();
        if let Some(script) = &inv.script {
            return match run_script(script, &mut fs, &env, &inv.stdin, &mut GitPrograms) {
                Ok(o) => Outcome::exited(o.stdout, o.stderr, o.status, fs),
                Err(e) => Outcome::unsupported(format!("mini-shell: {e}")),
            };
        }
        let code = {
            let mut ctx = Ctx { fs: &mut fs, env: &env, stdin: &inv.stdin, stdout: &mut ctx_out, stderr: &mut ctx_err };
            cmd::run_git(&inv.argv, &mut ctx)
        };
        Outcome::exited(ctx_out, ctx_err, code, fs)
    }
}
