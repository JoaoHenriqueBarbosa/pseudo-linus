//! Testes do mini-shell de teste (sem git): cada construção que os casos do corpus usam.

#[path = "support/mod.rs"]
mod support;

use sysabi::testkit::{RunResult, TestKit};

fn kit() -> TestKit {
    TestKit::new().programs(support::minish::programs())
}

fn sh(k: &TestKit, script: &str) -> RunResult {
    k.run(&["bash", "-c", script], b"")
}

fn check(script: &str, stdout: &str, stderr: &str, code: i32) {
    let k = kit();
    let r = sh(&k, script);
    assert_eq!(r.stdout_str(), stdout, "stdout de {script:?} (stderr: {:?})", r.stderr_str());
    assert_eq!(r.stderr_str(), stderr, "stderr de {script:?}");
    assert_eq!(r.code(), code, "status de {script:?}");
}

#[test]
fn lists_and_status() {
    check("echo a; echo b", "a\nb\n", "", 0);
    check("false || echo ok", "ok\n", "", 0);
    check("true && echo yes && false; echo $?", "yes\n1\n", "", 0);
    check("false && echo no", "", "", 1);
    check("! false; echo $?", "0\n", "", 0);
    check("echo one\necho two", "one\ntwo\n", "", 0);
    check("exit 3; echo nope", "", "", 3);
}

#[test]
fn command_not_found() {
    check("nope; echo $?", "127\n", "bash: line 1: nope: command not found\n", 0);
    check("echo x\nmissing-cmd arg", "x\n", "bash: line 2: missing-cmd: command not found\n", 127);
}

#[test]
fn pipes_files_and_redirections() {
    check("printf 'a\\nb\\n' > f && cat f | wc -l", "2\n", "", 0);
    check("echo hi >> g; echo there >> g; cat g", "hi\nthere\n", "", 0);
    check("echo a 2>&1 >/dev/null", "", "", 0);
    check("cat nofile 2>&1 | head -n 1", "cat: nofile: No such file or directory\n", "", 0);
    check("cat nofile 2>/dev/null; echo $?", "1\n", "", 0);
    check("{ echo a; echo b; } > g; cat g", "a\nb\n", "", 0);
    check("echo err >&2", "", "err\n", 0);
    check("ls nofile &> out; cat out", "ls: cannot access 'nofile': No such file or directory\n", "", 0);
    check("cat < missing", "", "bash: line 1: missing: No such file or directory\n", 1);
    check("printf 'x\\ny\\nz\\n' | grep -v y | sort -r", "z\nx\n", "", 0);
    check("echo abc | tr a-z A-Z", "ABC\n", "", 0);
}

#[test]
fn heredocs() {
    check("cat <<EOF > f\nline $((1+1))\nEOF\ncat f", "line 2\n", "", 0);
    check("x=val; cat <<'EOF'\n$x stays\nEOF", "$x stays\n", "", 0);
    check("x=val; cat <<EOF\n$x expands\nEOF", "val expands\n", "", 0);
    check("cat <<-EOF\n\tindented\n\tEOF\necho after", "indented\nafter\n", "", 0);
    check("msg=\"$(cat <<'EOF'\nsubject (with parens)\n\nbody's line\nEOF\n)\"; echo \"$msg\"", "subject (with parens)\n\nbody's line\n", "", 0);
    check("cat <<< 'here string'", "here string\n", "", 0);
}

#[test]
fn quoting_and_expansion() {
    check("x=$(echo hi); echo \"$x!\"", "hi!\n", "", 0);
    check("x='a  b'; echo $x; echo \"$x\"", "a b\na  b\n", "", 0);
    check("echo 'single $x' \"double \\\"q\\\"\" back\\ slash", "single $x double \"q\" back slash\n", "", 0);
    check("echo ${UNSET:-default} ${HOME}", "default /root\n", "", 0);
    check("echo `echo tick`", "tick\n", "", 0);
    check("A=1 env | grep '^A='", "A=1\n", "", 0);
    check("export B=2; env | grep '^B='", "B=2\n", "", 0);
    check("C=3; env | grep -c '^C='", "0\n", "", 1);
    check("echo $'a\\tb'", "a\tb\n", "", 0);
    check("echo ~/x", "/root/x\n", "", 0);
    check("n=$(printf 'a\\nb\\n' | wc -l); echo \"[$n]\"", "[2]\n", "", 0);
}

#[test]
fn control_flow() {
    check("touch a.txt b.txt c.md; for f in *.txt; do echo $f; done", "a.txt\nb.txt\n", "", 0);
    check("for i in 1 2 3\ndo\n  echo n$i\ndone", "n1\nn2\nn3\n", "", 0);
    check("if [ -f nofile ]; then echo yes; elif true; then echo elif; else echo no; fi", "elif\n", "", 0);
    check("if test 1 -lt 2; then echo lt; fi", "lt\n", "", 0);
    check("i=0; while [ $i -lt 3 ]; do echo $i; i=$((i+1)); done", "0\n1\n2\n", "", 0);
    check("printf 'x\\ny\\n' | while read line; do echo \"got $line\"; done", "got x\ngot y\n", "", 0);
    check("set -e; false; echo never", "", "", 1);
    check("set -e; false || true; echo ok", "ok\n", "", 0);
}

#[test]
fn cwd_and_subshells() {
    check("mkdir sub && cd sub && pwd", "/work/sub\n", "", 0);
    check("mkdir -p a/b; (cd a/b && pwd); pwd", "/work/a/b\n/work\n", "", 0);
    check("(exit 4); echo $?", "4\n", "", 0);
    check("x=1; (x=2); echo $x", "1\n", "", 0);
    check("cd nowhere", "", "bash: line 1: cd: nowhere: No such file or directory\n", 1);
}

#[test]
fn file_utilities() {
    check("mkdir d; touch d/x; ls d; rm -r d; ls", "x\n", "", 0);
    check("printf '1\\n2\\n3\\n4\\n' > n; head -2 n; tail -n 1 n; tail -n +3 n", "1\n2\n4\n3\n4\n", "", 0);
    check("echo hello > a; cp a b; mv b c; cat c; ls", "hello\na\nc\n", "", 0);
    check("echo '#!/bin/sh' > s; chmod +x s; test -x s && echo exec", "exec\n", "", 0);
    check("echo t > a; ln -s a l; cat l", "t\n", "", 0);
    check("rm nofile", "", "rm: cannot remove 'nofile': No such file or directory\n", 1);
    check("mkdir x; mkdir x", "", "mkdir: cannot create directory \u{2018}x\u{2019}: File exists\n", 1);
    check("printf 'a b\\nc\\n' | wc -w", "3\n", "", 0);
    check("printf 'foo bar\\nbaz\\n' > f; sed -n '2p' f; sed 's/o/0/g' f", "baz\nf00 bar\nbaz\n", "", 0);
    check("printf 'v1\\n' > f; sed -i 's/v1/v2/' f; cat f", "v2\n", "", 0);
    check("printf '%s-%d\\n' a 1 b 2", "a-1\nb-2\n", "", 0);
    check("echo -n no-newline; echo", "no-newline\n", "", 0);
    check("printf 'b\\na\\nb\\n' | sort | uniq", "a\nb\n", "", 0);
    check("echo a:b:c | cut -d: -f2", "b\n", "", 0);
    check("printf 'x\\n' | grep -q x && echo found", "found\n", "", 0);
    check("printf 'Abc\\nxyz\\n' | grep -in abc", "1:Abc\n", "", 0);
    check("printf 'a1\\nb22\\nc\\n' | grep -E 'a[0-9]|2+$' | head -n 5", "a1\nb22\n", "", 0);
}

#[test]
fn syntax_error() {
    let k = kit();
    let r = sh(&k, "echo (");
    assert_eq!(r.code(), 2);
    assert!(r.stderr_str().contains("syntax error"), "{}", r.stderr_str());
}
