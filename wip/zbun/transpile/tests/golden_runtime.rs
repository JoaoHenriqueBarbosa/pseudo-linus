//! Golden byte a byte do transpilador do runtime contra `bun build --no-bundle --target=bun --minify-syntax` do bun
//! 1.4.2 (as opções que `bun run` liga para o alvo `bun`): cada entrada de `tests/golden` tem o `x.expected.runtime`
//! que `scripts/gen_golden.sh` gera no oráculo. Uma entrada em `KNOWN_DIVERGENCES` continua rodando; se passar a
//! bater, o teste pede a remoção da linha.

use std::path::Path;

/// `(entrada, causa medida no oráculo)`.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[(
    "using.js",
    "`using c = null, d = undefined` vira `let` sob minify_syntax e o oráculo inlina os dois no `console.log` seguinte (`null, void 0`); o porte deixa o `let c = null, d = void 0;`: o `mangle_stmts` não funde o `let` vindo de `using` com o uso seguinte, backlog do zbun",
)];

fn first_difference(expected: &str, actual: &str) -> String {
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    for i in 0..e.len().max(a.len()) {
        if e.get(i) != a.get(i) {
            return format!(
                "linha {}:\n  esperado {}\n  obtido   {}",
                i + 1,
                e.get(i).copied().unwrap_or("<fim>"),
                a.get(i).copied().unwrap_or("<fim>")
            );
        }
    }
    "só o final do texto difere".into()
}

#[test]
fn runtime_transpile_matches_bun_build_target_bun() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/golden")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| dir.join(format!("{n}.expected.runtime")).is_file())
        .collect();
    names.sort();
    assert!(names.len() >= 50, "o corpus runtime tem só {} entradas (rode scripts/gen_golden.sh)", names.len());
    let mut failures = Vec::new();
    let mut passed = 0;
    for name in &names {
        let source = std::fs::read(dir.join(name)).expect("entrada");
        let expected = std::fs::read(dir.join(format!("{name}.expected.runtime"))).expect("esperado");
        let known = KNOWN_DIVERGENCES.iter().find(|(n, _)| n == name);
        let outcome = bun_transpile::transpile_with(&format!("/w/{name}"), &source, bun_transpile::Config { like_build: true });
        let matches = matches!(&outcome, Ok(done) if done.text == expected);
        match (matches, known) {
            (true, None) => passed += 1,
            (true, Some((_, cause))) => failures.push(format!("{name}: passou a bater, tire de KNOWN_DIVERGENCES ({cause})")),
            (false, Some(_)) => {}
            (false, None) => failures.push(match outcome {
                Ok(done) => format!("{name}: {}", first_difference(&String::from_utf8_lossy(&expected), &String::from_utf8_lossy(&done.text))),
                Err(e) => format!("{name}: {e}"),
            }),
        }
    }
    assert!(failures.is_empty(), "{passed}/{} batem; divergem:\n{}", names.len(), failures.join("\n"));
}
