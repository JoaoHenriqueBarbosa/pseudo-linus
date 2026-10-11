//! `python3 -u` e `PYTHONUNBUFFERED`: stdout sem buffer (a escrita sai na hora, mesmo para arquivo ou pipe) e
//! `sys.stdout.write_through` verdadeiro, como no CPython.

mod common;

use common::*;
use serde_json::json;

#[test]
fn python_unbuffered_option_and_environment_variable() {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let script = "python3 -u -c 'import time;print(1);time.sleep(3)' > /tmp/f 2>&1 & sleep 1; echo A; cat /tmp/f; echo B; \
python3 -u -c 'import sys;print(sys.stdout.write_through)'; \
PYTHONUNBUFFERED=1 python3 -c 'import sys;print(sys.stdout.write_through)'; \
python3 -c 'import sys;print(sys.stdout.write_through)'; \
python3 -c 'import time;print(2);time.sleep(3)' > /tmp/g 2>&1 & sleep 1; echo C; cat /tmp/g; echo D";
    let r = exec(&c, &sb, script);
    assert_eq!(r["exit_code"], 0, "{r}");
    assert_eq!(r["stdout"].as_str().unwrap(), "A\n1\nB\nTrue\nTrue\nFalse\nC\nD\n", "{r}");
}
