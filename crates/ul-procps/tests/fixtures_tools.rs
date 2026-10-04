//! Formatos com dados de processo (free, uptime, pgrep, pkill, pidwait, pidof, kill, killall, top)
//! comparados byte a byte com o procps real rodado sobre o mesmo `/proc` falso (ver
//! `tests/common/mod.rs`).

mod common;

fn starts(prefixes: &'static [&'static str]) -> impl Fn(&str) -> bool {
    move |cmd: &str| {
        let argv0 = common::split_cmd(cmd).1.first().cloned().unwrap_or_default();
        prefixes.contains(&argv0.as_str())
    }
}

#[test]
fn free_and_uptime_basic() {
    common::check("basic", "basic-tools", starts(&["free", "uptime"]), &[]).assert_ok("free/uptime (basic)");
}

#[test]
fn pgrep_family_basic() {
    common::check("basic", "basic-tools", starts(&["pgrep", "pkill", "pidwait"]), &[]).assert_ok("pgrep/pkill/pidwait (basic)");
}

#[test]
fn pgrep_family_multi() {
    common::check("multi", "multi", starts(&["pgrep", "pkill", "pidwait"]), &[]).assert_ok("pgrep/pkill/pidwait (multi)");
}

#[test]
fn pidof_basic_and_multi() {
    common::check("basic", "basic-tools", starts(&["pidof"]), &[]).assert_ok("pidof (basic)");
    common::check("multi", "multi", starts(&["pidof"]), &[]).assert_ok("pidof (multi)");
}

#[test]
fn kill_basic() {
    common::check("basic", "basic-tools", starts(&["kill"]), &[]).assert_ok("kill (basic)");
}
