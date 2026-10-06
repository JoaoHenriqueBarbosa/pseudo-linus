//! Cenários com IA real (ou com a fita gravada): o modelo só tem o `osh`, e cada teste confere o
//! estado do sandbox por conta própria, sem confiar no que o modelo disse.
//!
//! Gravar: `ANTHROPIC_BASE_URL=... ANTHROPIC_API_KEY=... PL_CASSETTE=record cargo test`.
//! Reproduzir: `PL_CASSETTE=replay cargo test` (sem rede, sem chave).

use pl_agent_harness::Scenario;

/// Servidor HTTP em segundo plano numa sessão, cliente `curl` em outra.
#[tokio::test(flavor = "multi_thread")]
async fn background_server_and_curl_from_other_session() -> anyhow::Result<()> {
    let s = Scenario::start("background_server_and_curl").await?;
    let t = s
        .agent(
            "main",
            "In the shell session named \"server\", create /srv/www/index.html containing exactly the text \
             `hello from pseudo-linus`, then start `python3 -m http.server 8080 --directory /srv/www` in the \
             background in that same session. Then, in a DIFFERENT session named \"client\", fetch \
             http://127.0.0.1:8080/index.html with curl and tell me the body you got.",
        )
        .await?;
    assert!(!t.is_error, "agente terminou com erro: {}", t.final_text);
    assert!(t.sessions().len() >= 2, "o modelo usou só as sessões {:?}", t.sessions());
    let body = s.sandbox.exec("check", "curl -s http://127.0.0.1:8080/index.html", Some(10_000)).await?;
    assert_eq!(body.stdout.trim(), "hello from pseudo-linus", "o servidor não está de pé: {body:?}");
    assert!(t.final_text.contains("hello from pseudo-linus"), "resposta final: {}", t.final_text);
    s.finish().await
}

/// O modelo estraga a máquina, o harness restaura o snapshot e o modelo confirma que voltou.
#[tokio::test(flavor = "multi_thread")]
async fn snapshot_recovery() -> anyhow::Result<()> {
    let s = Scenario::start("snapshot_recovery").await?;
    s.sandbox.exec("setup", "mkdir -p /data && seq 1 100 > /data/numbers.txt && echo ok", None).await?;
    let snap = s.sandbox.snapshot("before-damage").await?;

    let t = s
        .agent(
            "damage",
            "Delete the directory /data recursively and replace /etc/hostname with the text `broken`. \
             Confirm both changes with commands.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let gone = s.sandbox.exec("check", "test -e /data/numbers.txt; echo $?", None).await?;
    assert_eq!(gone.stdout.trim(), "1", "o modelo não apagou /data");

    s.sandbox.restore(&snap).await?;

    let t = s
        .agent(
            "verify",
            "Check the file /data/numbers.txt: how many lines does it have and what is the sum of the numbers \
             in it? Also print the contents of /etc/hostname. Answer with the line count, the sum and the hostname.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    assert_eq!(s.sandbox.read_file("/data/numbers.txt").await?.lines().count(), 100);
    assert!(t.final_text.contains("5050"), "o modelo não somou o arquivo restaurado: {}", t.final_text);
    assert!(!s.sandbox.read_file("/etc/hostname").await?.contains("broken"));
    s.finish().await
}

/// Dois agentes ao mesmo tempo no mesmo sandbox, cada um nas suas sessões, sem pisar um no outro.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_agents_same_sandbox() -> anyhow::Result<()> {
    let s = Scenario::start("concurrent_agents").await?;
    let a = s.agent(
        "alpha",
        "Use only the session named \"alpha\". Create the directory /work/alpha, cd into it, and write the \
         squares of 1 to 20 (one per line) to squares.txt. Then print the sum of the file using awk.",
    );
    let b = s.agent(
        "beta",
        "Use only the session named \"beta\". Create the directory /work/beta, cd into it, and write the words \
         of the sentence `the quick brown fox jumps over the lazy dog` one per line, sorted alphabetically, \
         into words.txt. Then print the number of unique words.",
    );
    let (a, b) = tokio::join!(a, b);
    let (a, b) = (a?, b?);
    assert!(!a.is_error && !b.is_error);
    let sq = s.sandbox.exec("check", "awk '{s+=$1} END {print s}' /work/alpha/squares.txt", None).await?;
    assert_eq!(sq.stdout.trim(), "2870");
    let w = s.sandbox.exec("check", "sort -u /work/beta/words.txt | wc -l", None).await?;
    assert_eq!(w.stdout.trim(), "8");
    assert!(a.final_text.contains("2870"), "{}", a.final_text);
    assert!(b.final_text.contains('8'), "{}", b.final_text);
    s.finish().await
}

/// Estado de sessão: cwd, variável exportada e job em segundo plano persistem entre chamadas.
#[tokio::test(flavor = "multi_thread")]
async fn session_state_persists() -> anyhow::Result<()> {
    let s = Scenario::start("session_state").await?;
    let t = s
        .agent(
            "main",
            "Do these as SEPARATE osh calls in the session \"main\": (1) `cd /tmp && mkdir -p stage && cd stage`; \
             (2) `export STAGE_TOKEN=pl-$(( 6 * 7 ))`; (3) start `sleep 300 &` in the background; \
             (4) print `pwd`, `$STAGE_TOKEN` and the output of `jobs`. Report the three values.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    assert!(t.tool_calls.len() >= 4, "o modelo juntou as chamadas: {:?}", t.commands());
    let last = &t.tool_calls.last().unwrap().output;
    assert!(last.contains("/tmp/stage") && last.contains("pl-42") && last.contains("sleep"), "{last}");
    s.finish().await
}

/// Exercício livre: o modelo monta e roda um pipeline de verdade (compilar C, testar, empacotar).
#[tokio::test(flavor = "multi_thread")]
async fn build_test_package_pipeline() -> anyhow::Result<()> {
    let s = Scenario::start("build_pipeline").await?;
    let t = s
        .agent(
            "main",
            "In /project, write a C program fib.c that prints the first N Fibonacci numbers (N from argv, one per \
             line, starting 0 1 1 2), compile it with gcc to ./fib, check that `./fib 10` ends with 34, then create \
             /project/fib.tar.gz containing fib.c and fib. Show `tar tzf` of the archive at the end.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let run = s.sandbox.exec("check", "/project/fib 10 | tail -1; tar tzf /project/fib.tar.gz | sort", None).await?;
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.first().copied(), Some("34"), "{run:?}");
    assert!(lines.iter().any(|l| l.ends_with("fib.c")) && lines.iter().any(|l| l.ends_with("fib")), "{run:?}");
    s.finish().await
}
