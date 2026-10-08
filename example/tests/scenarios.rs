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
             `hello from the web box`, then start `python3 -m http.server 8080 --directory /srv/www` in the \
             background in that same session. Then, in a DIFFERENT session named \"client\", fetch \
             http://127.0.0.1:8080/index.html with curl and tell me the body you got.",
        )
        .await?;
    assert!(!t.is_error, "agente terminou com erro: {}", t.final_text);
    assert!(t.sessions().len() >= 2, "o modelo usou só as sessões {:?}", t.sessions());
    let body = s.sandbox.exec("check", "curl -s http://127.0.0.1:8080/index.html", Some(10_000)).await?;
    assert_eq!(body.stdout.trim(), "hello from the web box", "o servidor não está de pé: {body:?}");
    assert!(t.final_text.contains("hello from the web box"), "resposta final: {}", t.final_text);
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

/// Um processo pendurado em segundo plano: o modelo precisa achar e matar, e o processo some mesmo.
#[tokio::test(flavor = "multi_thread")]
async fn find_and_kill_runaway_process() -> anyhow::Result<()> {
    let s = Scenario::start("kill_runaway").await?;
    s.sandbox.exec("setup", "nohup sh -c 'while :; do sleep 1; done' >/dev/null 2>&1 & echo $! > /tmp/runaway.pid", None).await?;
    let t = s
        .agent(
            "main",
            "Some shell loop running `sleep 1` forever was left in the background on this machine. Find it with \
             ps, kill it (and only it), and prove it is gone.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let alive = s.sandbox.exec("check", "kill -0 $(cat /tmp/runaway.pid) 2>/dev/null && echo vivo || echo morto", None).await?;
    assert_eq!(alive.stdout.trim(), "morto", "comandos: {:?}", t.commands());
    s.finish().await
}

/// Um comando que estoura o timeout: a sessão é reiniciada mantendo cwd e `export`, e o modelo segue.
#[tokio::test(flavor = "multi_thread")]
async fn timeout_resets_session_and_agent_recovers() -> anyhow::Result<()> {
    let s = Scenario::start("timeout_recovery").await?;
    let t = s
        .agent(
            "main",
            "In session \"main\": run `cd /var/tmp && export MARK=kept`, then run `sleep 120` with timeout_ms set to \
             2000 (it will time out). After that, in the same session, print `pwd` and `$MARK` and tell me whether \
             they survived the timeout.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    assert!(t.tool_calls.iter().any(|c| c.output.contains("[timed out]")), "nenhum timeout: {:?}", t.commands());
    let after = t.tool_calls.iter().rev().find(|c| c.output.contains("/var/tmp")).map(|c| c.output.clone());
    assert!(after.as_deref().is_some_and(|o| o.contains("kept")), "{:?}", t.tool_calls);
    s.finish().await
}

/// Dois sandboxes do mesmo usuário em paralelo: o que um agente escreve o outro não vê.
#[tokio::test(flavor = "multi_thread")]
async fn parallel_sandboxes_are_isolated() -> anyhow::Result<()> {
    let s = Scenario::start("parallel_sandboxes").await?;
    let other = pl_agent_harness::sandbox::Sandbox::create(&s.daemon.base_url, &s.daemon.token, "other").await?;
    let a = pl_agent_harness::agent::run(
        &s.sandbox,
        &s.cassette,
        "one",
        "Write the text `belongs-to-one` to /root/secret.txt and show the file.",
    );
    let b = pl_agent_harness::agent::run(
        &other,
        &s.cassette,
        "two",
        "Check whether the file /root/secret.txt exists. Report exactly `EXISTS` or `MISSING`, and the hostname.",
    );
    let (a, b) = tokio::join!(a, b);
    let (a, b) = (a?, b?);
    assert!(!a.is_error && !b.is_error);
    assert_eq!(s.sandbox.read_file("/root/secret.txt").await?.trim(), "belongs-to-one");
    let check = other.exec("check", "test -e /root/secret.txt && echo sim || echo nao; hostname", None).await?;
    assert_eq!(check.stdout, "nao\nother\n");
    assert!(b.final_text.contains("MISSING") && b.final_text.contains("other"), "{}", b.final_text);
    other.destroy().await?;
    s.finish().await
}

/// Git de verdade: repositório, commits, branch e merge, conferidos pelo harness.
#[tokio::test(flavor = "multi_thread")]
async fn git_branch_and_merge() -> anyhow::Result<()> {
    let s = Scenario::start("git_workflow").await?;
    let t = s
        .agent(
            "main",
            "Create a git repository in /repo (set user.name to `Bot` and user.email to `bot@example.com`). Commit a \
             README.md with the line `v1`. Create a branch `feature`, add a file feature.txt with `done` and commit it. \
             Go back to the default branch and merge `feature`. Finally show `git log --oneline` and list the files.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s.sandbox.exec("check", "cd /repo && git log --oneline | wc -l && cat feature.txt README.md && git branch --list feature", None).await?;
    let lines: Vec<&str> = r.stdout.lines().map(str::trim).collect();
    assert_eq!(lines.first().copied(), Some("2"), "{r:?}");
    assert!(lines.contains(&"done") && lines.contains(&"v1") && lines.contains(&"feature"), "{r:?}");
    s.finish().await
}

/// Arquivo grande e pipeline com sort/uniq: saída truncada não pode confundir o resultado final.
#[tokio::test(flavor = "multi_thread")]
async fn large_file_statistics() -> anyhow::Result<()> {
    let s = Scenario::start("large_file").await?;
    s.sandbox
        .exec("setup", "seq 1 200000 | awk '{print \"user\" ($1 % 37) \",\" $1}' > /data.csv", Some(60_000))
        .await?;
    let t = s
        .agent(
            "main",
            "The file /data.csv has lines `name,value`. Without printing the whole file, tell me how many lines it \
             has, how many distinct names there are, and which name has the largest sum of values.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let want = s
        .sandbox
        .exec("check", "awk -F, '{s[$1]+=$2} END {for (k in s) print s[k], k}' /data.csv | sort -n | tail -1 | cut -d' ' -f2", None)
        .await?;
    let txt = t.final_text.replace(',', "");
    assert!(txt.contains("200000") && txt.contains("37"), "{}", t.final_text);
    assert!(t.final_text.contains(want.stdout.trim()), "esperado {} em: {}", want.stdout.trim(), t.final_text);
    s.finish().await
}

/// Script bash escrito pelo modelo com função, trap EXIT, `while read` e `set -euo pipefail`.
#[tokio::test(flavor = "multi_thread")]
async fn agent_writes_and_runs_bash_script() -> anyhow::Result<()> {
    let s = Scenario::start("bash_script").await?;
    s.sandbox.exec("setup", "mkdir -p /in && printf 'alice 30\\nbob 25\\ncarol 41\\n' > /in/people.txt", None).await?;
    let t = s
        .agent(
            "main",
            "The file /in/people.txt already exists with real data: do not modify it. Write /usr/local/bin/report.sh, \
             a bash script with `set -euo pipefail` that: defines a function `log` \
             writing to stderr; installs `trap` on EXIT that writes `cleanup` to /tmp/trap.log; reads /in/people.txt \
             with a `while read -r name age` loop; prints `NAME is AGE` for each line with the name uppercased; and \
             finally prints the average age. Make it executable, run it, and show its stdout.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    assert_eq!(s.sandbox.read_file("/in/people.txt").await?, "alice 30\nbob 25\ncarol 41\n", "o modelo mexeu nos dados");
    let r = s.sandbox.exec("check", "rm -f /tmp/trap.log; report.sh 2>/dev/null; cat /tmp/trap.log", None).await?;
    assert!(r.stdout.contains("ALICE is 30") && r.stdout.contains("CAROL is 41"), "{r:?}");
    assert!(r.stdout.contains("32"), "média 32: {r:?}");
    assert!(r.stdout.trim_end().ends_with("cleanup"), "{r:?}");
    s.finish().await
}

/// Python no sandbox: script com json, collections e argparse, rodado pelo modelo.
#[tokio::test(flavor = "multi_thread")]
async fn python_data_processing() -> anyhow::Result<()> {
    let s = Scenario::start("python_data").await?;
    s.sandbox
        .exec(
            "setup",
            "mkdir -p /in && printf '%s\\n' '{\"city\":\"Recife\",\"temp\":31}' '{\"city\":\"Curitiba\",\"temp\":14}' \
             '{\"city\":\"Recife\",\"temp\":29}' '{\"city\":\"Curitiba\",\"temp\":18}' > /in/readings.jsonl",
            None,
        )
        .await?;
    let t = s
        .agent(
            "main",
            "Do not modify /in/readings.jsonl. Write /opt/agg.py, a python3 script using argparse (a positional input \
             path and an optional --out path) that reads the JSON Lines file, computes the mean temperature per city \
             with collections.defaultdict, and writes a JSON object {city: mean} sorted by city to --out. Run it with \
             --out /opt/means.json and show the output file.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let v: serde_json::Value = serde_json::from_str(&s.sandbox.read_file("/opt/means.json").await?)?;
    assert_eq!(v, serde_json::json!({ "Curitiba": 16.0, "Recife": 30.0 }));
    let r = s.sandbox.exec("check", "python3 /opt/agg.py --help | head -1", None).await?;
    assert!(r.stdout.starts_with("usage: agg.py"), "{r:?}");
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
             /project/fib.tar.gz containing fib.c and fib. Show `tar tzf` of the archive at the end. If a needed tool \
             is not installed on this machine, do not spend long hunting for it: deliver the closest working result \
             and say clearly what was missing.",
        )
        .await?;
    // Um Debian mínimo não tem gcc: os dois desfechos honestos valem, desde que o modelo não tenha
    // visto nada que denuncie a simulação (o `Scenario::agent` já confere isso).
    assert!(!t.is_error, "{}", t.final_text);
    let run = s.sandbox.exec("check", "/project/fib 10 | tail -1; tar tzf /project/fib.tar.gz | sort", None).await?;
    let lines: Vec<&str> = run.stdout.lines().collect();
    let gcc = s.sandbox.exec("check", "command -v gcc cc", None).await?;
    if gcc.stdout.trim().is_empty() {
        assert!(t.final_text.to_lowercase().contains("gcc"), "o modelo não disse que faltou o gcc: {}", t.final_text);
        s.finish().await?;
        return Ok(());
    }
    assert_eq!(lines.first().copied(), Some("34"), "{run:?}");
    assert!(lines.iter().any(|l| l.ends_with("fib.c")) && lines.iter().any(|l| l.ends_with("fib")), "{run:?}");
    s.finish().await
}

/// `pip install` num venv (pelo espelho do PyPI), API Flask em segundo plano e cliente com requests.
#[tokio::test(flavor = "multi_thread")]
async fn venv_flask_api_and_requests_client() -> anyhow::Result<()> {
    let s = Scenario::start("venv_flask_api").await?;
    let t = s
        .agent(
            "main",
            "Create a Python virtual environment in /opt/api/venv and install flask and requests into it with pip. \
             Write /opt/api/app.py, a Flask app with GET /health returning {\"status\": \"ok\"} and POST /sum that takes \
             a JSON body {\"numbers\": [...]} and returns {\"sum\": <total>}. Start it with the venv's python on port 5000 \
             in the background in a session named \"server\". Then, in a session named \"client\", write /opt/api/client.py \
             that uses requests to call /health and to POST {\"numbers\": [4, 8, 15, 16, 23, 42]} to /sum, printing both \
             JSON responses, and run it with the venv's python.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s
        .sandbox
        .exec("check", "curl -s -X POST -H 'Content-Type: application/json' -d '{\"numbers\":[1,2,3]}' http://127.0.0.1:5000/sum", Some(10_000))
        .await?;
    let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).map_err(|e| anyhow::anyhow!("{e}: {r:?}"))?;
    assert_eq!(v["sum"], 6, "{r:?}");
    let pip = s.sandbox.exec("check", "/opt/api/venv/bin/pip show flask requests | grep ^Name", None).await?;
    assert!(pip.stdout.contains("Flask") && pip.stdout.contains("requests"), "{pip:?}");
    assert!(t.final_text.contains("108"), "o modelo não mostrou a soma: {}", t.final_text);
    s.finish().await
}

/// Gráfico com Pillow a partir de um CSV: o PNG tem de existir, com as dimensões pedidas.
#[tokio::test(flavor = "multi_thread")]
async fn pillow_bar_chart_from_csv() -> anyhow::Result<()> {
    let s = Scenario::start("pillow_chart").await?;
    s.sandbox
        .exec("setup", "mkdir -p /in && printf 'month,sales\\njan,120\\nfeb,95\\nmar,160\\napr,140\\n' > /in/sales.csv", None)
        .await?;
    let t = s
        .agent(
            "main",
            "Using Python with Pillow (PIL), read /in/sales.csv and draw a bar chart of sales per month into \
             /out/sales.png, exactly 640x400 pixels, white background, one bar per month with the month name written \
             under each bar. Then, with a second Python command, open the PNG and print its size and mode.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s
        .sandbox
        .exec("check", "python3 -c 'from PIL import Image; im = Image.open(\"/out/sales.png\"); print(im.format, im.size)'", None)
        .await?;
    assert_eq!(r.stdout.trim(), "PNG (640, 400)", "{r:?}");
    s.finish().await
}

/// Backup com zip, conferência com unzip e sha256sum, restauração em outro diretório.
#[tokio::test(flavor = "multi_thread")]
async fn zip_backup_verify_and_restore() -> anyhow::Result<()> {
    let s = Scenario::start("zip_backup").await?;
    s.sandbox
        .exec("setup", "mkdir -p /srv/site/css /srv/site/img && echo '<h1>hi</h1>' > /srv/site/index.html && echo 'body{}' > /srv/site/css/a.css && head -c 50000 /dev/urandom > /srv/site/img/logo.bin", None)
        .await?;
    let t = s
        .agent(
            "main",
            "Back up /srv/site into /backup/site.zip with zip (recursively, keeping the paths relative to /srv). Save a \
             sha256 checksum of the zip in /backup/site.zip.sha256 in the format sha256sum produces, and verify it with \
             sha256sum -c. List the archive with unzip -l, test it with unzip -t, then restore it into /restore and prove \
             the restored tree is identical to the original with diff -r.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s
        .sandbox
        .exec("check", "cd /backup && sha256sum -c site.zip.sha256 && diff -r /srv/site /restore/site && echo SAME", None)
        .await?;
    assert!(r.stdout.contains("site.zip: OK") && r.stdout.trim_end().ends_with("SAME"), "{r:?}");
    s.finish().await
}

/// Git do dia a dia: trabalho guardado no stash, busca com git grep, e o stash de volta.
#[tokio::test(flavor = "multi_thread")]
async fn git_stash_and_grep() -> anyhow::Result<()> {
    let s = Scenario::start("git_stash_grep").await?;
    s.sandbox
        .exec(
            "setup",
            "mkdir /code && cd /code && git init -q && git config user.name Dev && git config user.email dev@example.com && \
             printf 'def load():\\n    # TODO: cache\\n    return 1\\n' > io.py && printf 'def run():\\n    return 2  # TODO: retry\\n' > job.py && \
             git add . && git commit -qm init && printf 'def load():\\n    return 3\\n' > io.py",
            None,
        )
        .await?;
    let t = s
        .agent(
            "main",
            "In the git repository /code there is uncommitted work in io.py. Save it with git stash (with the message \
             `wip load`), then use git grep with line numbers to list every TODO in the committed code and tell me how \
             many there are. Finally bring the stashed work back with git stash pop and show git status --short.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s.sandbox.exec("check", "cd /code && git stash list | wc -l && cat io.py && git status --short", None).await?;
    let lines: Vec<&str> = r.stdout.lines().map(str::trim).collect();
    assert_eq!(lines.first().copied(), Some("0"), "o stash não voltou: {r:?}");
    assert!(lines.contains(&"return 3") && lines.contains(&"M io.py"), "{r:?}");
    assert!(t.final_text.contains('2'), "{}", t.final_text);
    s.finish().await
}

/// CSV de vendas que o usuário mandou: o harness grava o arquivo como se fosse upload.
const SALES_CSV: &str = "date,region,product,units,unit_price\n\
2026-01-05,North,Widget,12,19.90\n2026-01-18,South,Gadget,5,49.00\n2026-01-27,North,Gadget,3,49.00\n\
2026-02-02,East,Widget,20,19.90\n2026-02-14,South,Widget,8,19.90\n2026-02-21,East,Gizmo,4,99.50\n\
2026-03-03,North,Gizmo,2,99.50\n2026-03-11,South,Gadget,9,49.00\n2026-03-29,East,Widget,15,19.90\n\
2026-04-08,North,Widget,18,19.90\n2026-04-16,South,Gizmo,6,99.50\n2026-04-30,East,Gadget,7,49.00\n";

async fn upload_sales(s: &Scenario) -> anyhow::Result<()> {
    s.sandbox.write_file("/home/user/uploads/sales_2026.csv", SALES_CSV).await
}

/// Pedido típico de usuário: relatório em PDF com gráficos a partir do CSV que ele mandou.
#[tokio::test(flavor = "multi_thread")]
async fn report_pdf_with_charts_from_uploaded_csv() -> anyhow::Result<()> {
    let s = Scenario::start("report_pdf_charts").await?;
    upload_sales(&s).await?;
    let t = s
        .agent(
            "main",
            "I uploaded my sales spreadsheet to /home/user/uploads/sales_2026.csv. Please make me a report from it: a \
             bar chart of revenue (units times unit_price) per month, a horizontal bar chart of revenue per region, and \
             a page with a small table of the totals per product. Use Python with Pillow, save each chart as PNG in \
             /home/user/report/ and put everything together, one page each, in /home/user/report/sales_report.pdf. \
             Tell me the total revenue and the best month.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s
        .sandbox
        .exec(
            "check",
            "head -c 5 /home/user/report/sales_report.pdf; echo; grep -ac '/Type */Page[^s]' /home/user/report/sales_report.pdf; \
             ls /home/user/report/*.png | wc -l",
            None,
        )
        .await?;
    let lines: Vec<&str> = r.stdout.lines().map(str::trim).collect();
    assert_eq!(lines.first().copied(), Some("%PDF-"), "{r:?}");
    assert!(lines.get(1).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n >= 3), "páginas: {r:?}");
    assert!(lines.get(2).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n >= 2), "PNGs: {r:?}");
    // Receita total 3822.70; melhor mês abril (1298.20).
    let txt = t.final_text.replace(',', "");
    assert!(txt.contains("3822.7") || txt.contains("3823"), "{}", t.final_text);
    assert!(t.final_text.contains("April") || t.final_text.contains("2026-04") || t.final_text.contains("Apr"), "{}", t.final_text);
    s.finish().await
}

/// O mesmo CSV vira um relatório HTML com os gráficos em PNG ao lado, servido para o usuário abrir.
#[tokio::test(flavor = "multi_thread")]
async fn report_html_dashboard_from_uploaded_csv() -> anyhow::Result<()> {
    let s = Scenario::start("report_html_dashboard").await?;
    upload_sales(&s).await?;
    let t = s
        .agent(
            "main",
            "Here is my sales export: /home/user/uploads/sales_2026.csv. Build a small dashboard in /home/user/dashboard: \
             a line chart of units sold per month and a pie-style chart (Pillow pieslice) of the revenue share per \
             product, both drawn with Pillow as PNG files, plus an index.html that shows the two images and a table with \
             revenue per region. Serve the folder with python3 -m http.server on port 8000 in the background so I can \
             open it, and check with curl that index.html and both images are served.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s
        .sandbox
        .exec(
            "check",
            "cd /home/user/dashboard && for f in $(grep -o 'src=\"[^\"]*\\.png\"' index.html | cut -d'\"' -f2); do \
             curl -s http://127.0.0.1:8000/$f | head -c 8 | od -An -c | tr -d ' '; echo; done",
            Some(10_000),
        )
        .await?;
    let pngs: Vec<&str> = r.stdout.lines().filter(|l| l.contains("PNG")).collect();
    assert!(pngs.len() >= 2, "as imagens não são servidas: {r:?}");
    let html = s.sandbox.read_file("/home/user/dashboard/index.html").await?;
    for region in ["North", "South", "East"] {
        assert!(html.contains(region), "falta {region} na tabela");
    }
    s.finish().await
}

/// Planilha com openpyxl (instalado pelo pip) gerada a partir de JSON, relida para conferência.
#[tokio::test(flavor = "multi_thread")]
async fn openpyxl_report_from_json() -> anyhow::Result<()> {
    let s = Scenario::start("openpyxl_report").await?;
    s.sandbox
        .exec("setup", "mkdir -p /in && echo '[{\"sku\":\"A1\",\"qty\":3,\"price\":9.5},{\"sku\":\"B2\",\"qty\":10,\"price\":1.25},{\"sku\":\"C3\",\"qty\":1,\"price\":120}]' > /in/orders.json", None)
        .await?;
    let t = s
        .agent(
            "main",
            "Install openpyxl with pip for the system python3 (use whatever flag pip asks for on this Debian). Then write \
             a script that reads /in/orders.json and creates /out/orders.xlsx with a sheet named `Orders`, a header row \
             sku, qty, price, total, one row per order where total is a formula =B*C, and a final row with the grand \
             total as a SUM formula. Reopen the file with openpyxl and print every row.",
        )
        .await?;
    assert!(!t.is_error, "{}", t.final_text);
    let r = s
        .sandbox
        .exec(
            "check",
            "python3 -c 'import openpyxl; ws = openpyxl.load_workbook(\"/out/orders.xlsx\")[\"Orders\"]; print([c.value for c in ws[1]]); print(ws.max_row)'",
            None,
        )
        .await?;
    let lines: Vec<&str> = r.stdout.lines().collect();
    assert_eq!(lines.first().copied(), Some("['sku', 'qty', 'price', 'total']"), "{r:?}");
    assert_eq!(lines.get(1).copied(), Some("5"), "{r:?}");
    s.finish().await
}
