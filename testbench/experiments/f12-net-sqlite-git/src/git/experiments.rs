//! Medições do H36: conformidade contra o golden de git, validação no oráculo com `git fsck --strict`,
//! leitura de um repositório com packfile criado pelo git real, linhas por comando, gix alto nível.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use harness::{Case, Entry, MemTree, Oracle, Outcome, paths};
use serde_json::{Value as Json, json};

use super::cmd::{SOURCES, loc};
use super::store::{HASH, Repo, gx};
use crate::shell::{Ctx, Programs, run_script};

pub fn load_cases() -> Result<Vec<(Case, Outcome)>> {
    let (cases, missing) = paths::load_tool("git")?;
    anyhow::ensure!(missing == 0, "{missing} casos de git sem golden; rode `cargo run -p oracle -- gen --tool git`");
    Ok(cases)
}

struct Git;

impl Programs for Git {
    fn run(&mut self, argv: &[String], ctx: &mut Ctx<'_>) -> Option<i32> {
        (argv[0] == "git").then(|| super::cmd::run_git(argv, ctx))
    }
}

pub fn git_env() -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = harness::BASE_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for who in ["AUTHOR", "COMMITTER"] {
        env.insert(format!("GIT_{who}_NAME"), "Agent".into());
        env.insert(format!("GIT_{who}_EMAIL"), "agent@example.com".into());
        env.insert(format!("GIT_{who}_DATE"), "1768478400 +0000".into());
    }
    env
}

fn env_exports() -> String {
    git_env()
        .iter()
        .filter(|(k, _)| k.starts_with("GIT_"))
        .map(|(k, v)| format!("export {k}='{v}'\n"))
        .collect()
}

/// Comandos rodados dos dois lados (protótipo e git real) sobre o repositório exportado.
const CHECKS: &[&str] = &[
    "git log --format='%H %P %T %an <%ae> %at %s'",
    "git log --format='%H %s' plumbing",
    "git ls-files -s",
    "git status --porcelain",
    "git cat-file -p HEAD",
    "git ls-tree -r HEAD",
    "git rev-parse HEAD feature plumbing",
    "git diff",
];

/// Constrói um repositório com o protótipo, exporta e valida no git real.
pub fn fsck_validation(oracle: &Oracle) -> Result<Json> {
    let mut fs = MemTree::new();
    fs.insert("a.txt", Entry::file(b"hello\n".to_vec(), 0o644));
    fs.insert("b.txt", Entry::file(b"to be removed\n".to_vec(), 0o644));
    fs.insert("run.sh", Entry::file(b"#!/bin/sh\necho hi\n".to_vec(), 0o755));
    fs.insert("bin.dat", Entry::file(vec![0, 1, 2, 3, 255, 0, 10], 0o644));
    fs.insert("link", Entry::symlink("a.txt"));
    let build = "git init -q
git add .
git commit -q -m 'first commit'
printf 'changed\\n' >> a.txt
printf 'fn main() {}\\n' > src/deep/main.rs
git add -A
git commit -q -m 'second commit'
git update-ref refs/heads/feature HEAD
tree=$(git write-tree)
c=$(git commit-tree $tree -p HEAD -m 'plumbing commit')
git update-ref refs/heads/plumbing $c
rm b.txt
git commit -q -am 'third: delete b'
printf 'dirty\\n' >> a.txt
";
    let built = run_script(build, &mut fs, &git_env(), b"", &mut Git).map_err(anyhow::Error::msg)?;
    // Mesmos comandos aqui e no oráculo, separados por marcadores numerados (sem aspas do comando).
    let checks: String = CHECKS.iter().enumerate().map(|(i, c)| format!("echo '== {i}'\n{c}\n")).collect();
    let ours = run_script(&checks, &mut fs.clone(), &git_env(), b"", &mut Git).map_err(anyhow::Error::msg)?;
    let script = format!(
        "{}cd /work/case\ngit fsck --strict --no-dangling > /tmp/fsck.out 2>&1; echo \"fsck_exit=$?\"; cat /tmp/fsck.out\ngit count-objects -v\necho '#### checks'\n{checks}",
        env_exports()
    );
    let outcome = oracle.run_script("git-fsck", &script, fs.clone())?;
    let theirs = String::from_utf8_lossy(&outcome.stdout.0).into_owned();
    let (head, their_checks) = theirs.split_once("#### checks\n").unwrap_or((&theirs, ""));
    let fsck_exit: i32 = head.lines().find_map(|l| l.strip_prefix("fsck_exit=")).and_then(|v| v.parse().ok()).unwrap_or(-1);
    let our_text = String::from_utf8_lossy(&ours.stdout).into_owned();
    let split = |t: &str| -> BTreeMap<String, String> {
        t.split("== ").skip(1).map(|s| {
            let (cmd, body) = s.split_once('\n').unwrap_or((s, ""));
            (cmd.to_string(), body.to_string())
        }).collect()
    };
    let (mine, real) = (split(&our_text), split(their_checks));
    let mut per_check = serde_json::Map::new();
    let mut equal = 0;
    for (i, c) in CHECKS.iter().enumerate() {
        let key = i.to_string();
        let (a, b) = (mine.get(&key), real.get(&key));
        // Um marcador ausente dos dois lados não pode contar como igual.
        let same = a.is_some() && a == b;
        equal += same as usize;
        per_check.insert(c.to_string(), json!({"same": same, "ours": a, "git": b}));
    }
    Ok(json!({
        "build_status": built.status,
        "build_stderr": String::from_utf8_lossy(&built.stderr),
        "fsck_strict_exit": fsck_exit,
        "fsck_and_count_objects": head,
        "oracle_stderr": String::from_utf8_lossy(&outcome.stderr.0),
        "checks_total": CHECKS.len(),
        "checks_equal": equal,
        "checks": per_check,
    }))
}

/// Repositório criado pelo git real com `git gc` (packfile com deltas, packed-refs, tag anotada),
/// lido aqui a partir dos bytes capturados.
pub fn read_real_packed_repo(oracle: &Oracle) -> Result<Json> {
    let script = format!(
        "{}set -e
cd /work/case
git init -q
seq 1 3000 > big.txt
printf 'x\\n' > small.txt
mkdir -p d/e && printf 'deep\\n' > d/e/f.txt
git add -A && git commit -q -m c1
for i in 2 3 4 5 6 7; do sed -i \"${{i}}00s/.*/changed $i/\" big.txt; printf \"line $i\\n\" >> small.txt; git add -A; git commit -q -m \"c$i\"; done
git tag v1 HEAD~2
git tag -a v2 -m 'annotated tag' HEAD
git gc -q --aggressive
mkdir -p expected
git cat-file --batch-all-objects --batch-check > expected/list.txt
for o in $(git cat-file --batch-all-objects --batch-check='%(objectname)'); do git cat-file -p $o > expected/$o; done
git log --format='%H %P %s' > expected/log.txt
git ls-tree -r HEAD > expected/ls-tree.txt
git status --porcelain > expected/status.txt
git rev-parse v1 v2 > expected/tags.txt
git verify-pack -v .git/objects/pack/*.idx | awk 'NF == 7 {{ n++ }} END {{ print n + 0 }}' > expected/deltas.txt
find .git -type f | sort > expected/files.txt
",
        env_exports()
    );
    let outcome = oracle.run_script("git-packed", &script, MemTree::new())?;
    anyhow::ensure!(outcome.exit == Some(0), "script do oráculo falhou: {}", String::from_utf8_lossy(&outcome.stderr.0));
    let mut fs = outcome.files.clone();
    let files_list = String::from_utf8_lossy(fs.read("expected/files.txt").unwrap_or_default()).into_owned();
    let list = String::from_utf8_lossy(fs.read("expected/list.txt").unwrap_or_default()).into_owned();
    let loose_files = files_list.lines().filter(|l| l.starts_with(".git/objects/") && !l.contains("/pack/") && !l.contains("/info/")).count();
    // 1) Todo objeto: tipo, tamanho e `cat-file -p` iguais.
    let mut objects = 0;
    let mut objects_equal = 0;
    let mut mismatches = Vec::new();
    let mut deltas_seen = 0;
    {
        let repo = Repo { fs: &mut fs };
        for line in list.lines() {
            let parts: Vec<&str> = line.split(' ').collect();
            let [oid, kind, size] = parts[..] else { continue };
            objects += 1;
            let id = gx(gix_hash::ObjectId::from_hex(oid.as_bytes()))?;
            let ok = match repo.read_object(&id) {
                Ok((k, data)) => {
                    let pretty = super::cmd::cat_file::pretty(&repo, k, &data)?;
                    let expected = repo.fs.read(&format!("expected/{oid}")).unwrap_or_default();
                    k.to_string() == kind && data.len().to_string() == size && pretty == expected
                }
                Err(e) => {
                    mismatches.push(format!("{oid}: {e}"));
                    false
                }
            };
            objects_equal += ok as usize;
            if !ok && mismatches.len() < 5 {
                mismatches.push(oid.to_string());
            }
        }
        // Quantas entradas do pack são deltas (contadas pelo gix-pack).
        for k in repo.fs.entries.keys().filter(|k| k.starts_with(".git/objects/pack/") && k.ends_with(".idx")) {
            let idx = repo.fs.read(k).unwrap_or_default().to_vec();
            let pack_path = k.replace(".idx", ".pack");
            let pack = repo.fs.read(&pack_path).unwrap_or_default().to_vec();
            let index = gx(gix_pack::index::File::from_data(idx, PathBuf::from(k), HASH))?;
            let data = gx(gix_pack::data::File::from_data(pack, PathBuf::from(&pack_path), HASH))?;
            for i in 0..index.num_objects() {
                let e = gx(data.entry(index.pack_offset_at_index(i)))?;
                if matches!(e.header, gix_pack::data::entry::Header::OfsDelta { .. } | gix_pack::data::entry::Header::RefDelta { .. }) {
                    deltas_seen += 1;
                }
            }
        }
    }
    // 2) Porcelana sobre o repositório empacotado (refs em packed-refs, índice escrito pelo git).
    let mut porcelain = serde_json::Map::new();
    for (cmd, file) in [
        ("git log --format='%H %P %s'", "log.txt"),
        ("git ls-tree -r HEAD", "ls-tree.txt"),
        ("git status --porcelain", "status.txt"),
        ("git rev-parse v1 v2", "tags.txt"),
    ] {
        let mut copy = fs.clone();
        // O worktree do oráculo tem o diretório `expected/`, que o git real viu como não rastreado.
        let o = run_script(cmd, &mut copy, &git_env(), b"", &mut Git).map_err(anyhow::Error::msg)?;
        let expected = fs.read(&format!("expected/{file}")).unwrap_or_default().to_vec();
        let ours = o.stdout.clone();
        porcelain.insert(
            cmd.to_string(),
            json!({"same": ours == expected, "ours": String::from_utf8_lossy(&ours), "git": String::from_utf8_lossy(&expected), "stderr": String::from_utf8_lossy(&o.stderr)}),
        );
    }
    let deltas_git: usize = String::from_utf8_lossy(fs.read("expected/deltas.txt").unwrap_or_default()).trim().parse().unwrap_or(0);
    Ok(json!({
        "objects_total": objects,
        "objects_equal_type_size_and_cat_file_p": objects_equal,
        "mismatches": mismatches,
        "pack_delta_entries_seen_by_gix_pack": deltas_seen,
        "pack_delta_entries_reported_by_git_verify_pack": deltas_git,
        "loose_object_files_left": loose_files,
        "porcelain_on_packed_repo": porcelain,
        "git_dir_files": files_list.lines().collect::<Vec<_>>(),
    }))
}

/// Linhas de código por comando do protótipo.
pub fn lines_per_command() -> Json {
    let mut out = serde_json::Map::new();
    let mut total = 0;
    for (name, src) in SOURCES {
        let n = loc(src);
        total += n;
        out.insert(name.to_string(), json!(n));
    }
    out.insert("total".into(), json!(total));
    Json::Object(out)
}

/// Repositório pequeno feito pelo protótipo, pro teste do `gix` alto nível.
pub fn sample_repo() -> Result<MemTree> {
    let mut fs = MemTree::new();
    fs.insert("a.txt", Entry::file(b"hello\n".to_vec(), 0o644));
    run_script("git init -q && git add . && git commit -q -m 'made by the prototype'", &mut fs, &git_env(), b"", &mut Git)
        .map_err(anyhow::Error::msg)?;
    Ok(fs)
}
