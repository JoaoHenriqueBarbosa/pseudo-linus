//! `pstree` do psmisc 23.7 (src/pstree.c).
//!
//! Lê os processos do `/proc` (e as threads de `task/`), monta a árvore como o original (pai que não
//! existe vira o nó `?` de pid 0, filhos em ordem de nome ou de pid com `-n`) e desenha com os
//! símbolos ASCII (o padrão quando a saída não é um terminal UTF-8) ou UTF-8 com `-U`. Subárvores
//! iguais são compactadas em `N*[...]` (`-c`, `-p`, `-g` e `-a` desligam; com `-a` as threads ainda
//! compactam). Linhas cortadas em `COLUMNS` (sem ele, 132) com `+` na última coluna, menos com `-l`.
//!
//! Fora: realce de `-h`/`-H` (exige terminal com terminfo), cores de `-C`, desenho VT100 de `-G`
//! (cai no ASCII), namespaces de `-N`/`-S` e contexto do SELinux de `-Z` (aceitos e ignorados).

use std::ffi::OsString;

use sysabi::{Ctx, Pid};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, out};
use crate::procfs::{self, Want};

const USAGE: &str = "Usage: pstree [-acglpsStTuZ] [ -h | -H PID ] [ -n | -N type ]\n              [ -A | -G | -U ] [ PID | USER ]\n   or: pstree -V\n\nDisplay a tree of processes.\n\n  -a, --arguments     show command line arguments\n  -A, --ascii         use ASCII line drawing characters\n  -c, --compact-not   don't compact identical subtrees\n  -C, --color=TYPE    color process by attribute\n                      (age)\n  -g, --show-pgids    show process group ids; implies -c\n  -G, --vt100         use VT100 line drawing characters\n  -h, --highlight-all highlight current process and its ancestors\n  -H PID, --highlight-pid=PID\n                      highlight this process and its ancestors\n  -l, --long          don't truncate long lines\n  -n, --numeric-sort  sort output by PID\n  -N TYPE, --ns-sort=TYPE\n                      sort output by this namespace type\n                              (cgroup, ipc, mnt, net, pid, time, user, uts)\n  -p, --show-pids     show PIDs; implies -c\n  -s, --show-parents  show parents of the selected process\n  -S, --ns-changes    show namespace transitions\n  -t, --thread-names  show full thread names\n  -T, --hide-threads  hide threads, show only processes\n  -u, --uid-changes   show uid transitions\n  -U, --unicode       use UTF-8 (Unicode) line drawing characters\n  -V, --version       display version information\n  -Z, --security-context\n                      show security attributes\n\n  PID    start at this PID; default is 1 (init)\n  USER   show only trees rooted at processes of this user\n\n";

const VERSION: &str = "pstree (PSmisc) 23.7\nCopyright (C) 1993-2024 Werner Almesberger and Craig Small\n\nPSmisc comes with ABSOLUTELY NO WARRANTY.\nThis is free software, and you are welcome to redistribute it under\nthe terms of the GNU General Public License.\nFor more information about these matters, see the files named COPYING.\n";

const LONGS: &[LongOpt] = &[
    LongOpt::new("arguments", HasArg::No, 'a' as i32),
    LongOpt::new("ascii", HasArg::No, 'A' as i32),
    LongOpt::new("compact-not", HasArg::No, 'c' as i32),
    LongOpt::new("color", HasArg::Required, 'C' as i32),
    LongOpt::new("vt100", HasArg::No, 'G' as i32),
    LongOpt::new("highlight-all", HasArg::No, 'h' as i32),
    LongOpt::new("highlight-pid", HasArg::Required, 'H' as i32),
    LongOpt::new("long", HasArg::No, 'l' as i32),
    LongOpt::new("numeric-sort", HasArg::No, 'n' as i32),
    LongOpt::new("ns-sort", HasArg::Required, 'N' as i32),
    LongOpt::new("show-pids", HasArg::No, 'p' as i32),
    LongOpt::new("show-pgids", HasArg::No, 'g' as i32),
    LongOpt::new("show-parents", HasArg::No, 's' as i32),
    LongOpt::new("ns-changes", HasArg::No, 'S' as i32),
    LongOpt::new("thread-names", HasArg::No, 't' as i32),
    LongOpt::new("hide-threads", HasArg::No, 'T' as i32),
    LongOpt::new("uid-changes", HasArg::No, 'u' as i32),
    LongOpt::new("unicode", HasArg::No, 'U' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
    LongOpt::new("security-context", HasArg::No, 'Z' as i32),
];

const ROOT_PID: Pid = 1;

/// Os símbolos de desenho: vazio, ramo, vertical, último (2 colunas), único e primeiro (3).
struct Sym {
    empty_2: &'static str,
    branch_2: &'static str,
    vert_2: &'static str,
    last_2: &'static str,
    single_3: &'static str,
    first_3: &'static str,
}

const SYM_ASCII: Sym = Sym { empty_2: "  ", branch_2: "|-", vert_2: "| ", last_2: "`-", single_3: "---", first_3: "-+-" };
const SYM_UTF: Sym = Sym {
    empty_2: "  ",
    branch_2: "\u{251c}\u{2500}",
    vert_2: "\u{2502} ",
    last_2: "\u{2514}\u{2500}",
    single_3: "\u{2500}\u{2500}\u{2500}",
    first_3: "\u{2500}\u{252c}\u{2500}",
};

struct Node {
    pid: Pid,
    pgid: Pid,
    uid: u32,
    comm: Vec<u8>,
    /// Argumentos depois do `argv[0]`.
    args: Vec<Vec<u8>>,
    /// Linha de comando vazia (thread de kernel, zumbi): `-a` mostra entre parênteses.
    swapped: bool,
    thread: bool,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// Saída com o corte de linha do `out_char` do original.
struct Out {
    buf: Vec<u8>,
    cur_x: usize,
    width: usize,
    trunc: bool,
    last_char: Option<u8>,
    charlen: usize,
}

impl Out {
    fn char(&mut self, c: u8) {
        if self.charlen == 0 {
            self.charlen = if c & 0x80 == 0 {
                1
            } else if c & 0xe0 == 0xc0 {
                2
            } else if c & 0xf0 == 0xe0 {
                3
            } else {
                4
            };
        }
        self.charlen -= 1;
        if self.charlen == 0 {
            self.cur_x += 1;
        }
        if self.cur_x <= self.width || !self.trunc {
            self.buf.push(c);
        }
        if self.cur_x == self.width + 1 && self.trunc && (c & 0xc0) != 0x80 {
            if self.last_char.is_some() || (c & 0x80) != 0 {
                self.buf.push(b'+');
            } else {
                self.last_char = Some(c);
                self.cur_x -= 1;
            }
        }
    }

    fn string(&mut self, s: &[u8]) {
        for &c in s {
            self.char(c);
        }
    }

    fn int(&mut self, n: i64) -> usize {
        let s = n.to_string();
        self.string(s.as_bytes());
        s.len()
    }

    fn newline(&mut self) {
        if let Some(c) = self.last_char.take()
            && self.cur_x == self.width
        {
            self.buf.push(c);
        }
        self.buf.push(b'\n');
        self.cur_x = 1;
    }
}

struct Tree {
    nodes: Vec<Node>,
    by_pid: std::collections::HashMap<Pid, usize>,
}

struct Opts {
    print_args: bool,
    compact: bool,
    pids: bool,
    pgids: bool,
    user_change: bool,
    sort_by_pid: bool,
    sym: &'static Sym,
}

impl Tree {
    fn find(&self, pid: Pid) -> Option<usize> {
        self.by_pid.get(&pid).copied()
    }

    fn new_node(&mut self, pid: Pid, comm: Vec<u8>) -> usize {
        let i = self.nodes.len();
        self.nodes.push(Node {
            pid,
            pgid: 0,
            uid: 0,
            comm,
            args: Vec::new(),
            swapped: false,
            thread: false,
            parent: None,
            children: Vec::new(),
        });
        self.by_pid.insert(pid, i);
        i
    }

    /// `add_child`: insere em ordem (nome com `strcmp`, ou pid com `-n`), depois dos iguais.
    fn add_child(&mut self, parent: usize, child: usize, by_pid: bool) {
        let pos = self.nodes[parent]
            .children
            .iter()
            .position(|&w| {
                let (a, b) = (&self.nodes[w], &self.nodes[child]);
                if by_pid { a.pid > b.pid } else { a.comm > b.comm }
            })
            .unwrap_or(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(pos, child);
        self.nodes[child].parent = Some(parent);
    }

    fn tree_equal(&self, a: usize, b: usize, o: &Opts) -> bool {
        let (na, nb) = (&self.nodes[a], &self.nodes[b]);
        if na.comm != nb.comm || (o.user_change && na.uid != nb.uid) || na.children.len() != nb.children.len() {
            return false;
        }
        na.children.iter().zip(&nb.children).all(|(x, y)| self.tree_equal(*x, *y, o))
    }
}

fn build(o_by_pid: bool, hide_threads: bool) -> Tree {
    let snap = procfs::scan(Want { cmdline: true, ..Want::default() });
    let mut t = Tree { nodes: Vec::new(), by_pid: std::collections::HashMap::new() };
    // (nó, pid do pai) na ordem em que o /proc lista, como o read_proc do original.
    let mut pending: Vec<(usize, Pid)> = Vec::new();
    for p in &snap.procs {
        let pid = p.pid();
        let i = t.new_node(pid, p.comm().to_vec());
        let args = p.args();
        t.nodes[i].pgid = p.stat.pgrp;
        t.nodes[i].uid = p.ruid();
        t.nodes[i].swapped = args.is_empty();
        t.nodes[i].args = args.iter().skip(1).cloned().collect();
        let ppid = if p.stat.ppid == pid { 0 } else { p.stat.ppid };
        pending.push((i, ppid));
        if !hide_threads {
            for th in procfs::threads(p, Want::default()) {
                if th.tid == pid {
                    continue;
                }
                let mut name = b"{".to_vec();
                name.extend_from_slice(th.comm());
                name.push(b'}');
                let j = t.new_node(th.tid, name);
                t.nodes[j].pgid = p.stat.pgrp;
                t.nodes[j].uid = th.ruid();
                t.nodes[j].thread = true;
                pending.push((j, pid));
            }
        }
    }
    for (i, ppid) in pending {
        let parent = match t.find(ppid) {
            Some(p) if p != i => p,
            _ => t.new_node(ppid, b"?".to_vec()),
        };
        if t.nodes[i].pid != 0 {
            t.add_child(parent, i, o_by_pid);
        }
    }
    t
}

struct Dumper<'a> {
    t: &'a Tree,
    o: &'a Opts,
    out: Out,
    width: Vec<usize>,
    more: Vec<bool>,
}

impl Dumper<'_> {
    fn ensure(&mut self, level: usize) {
        if self.width.len() <= level {
            self.width.resize(level + 1, 0);
            self.more.resize(level + 2, false);
        }
        if self.more.len() <= level + 1 {
            self.more.resize(level + 2, false);
        }
    }

    /// Filhos agrupados: cada um com quantos iguais vieram depois (só com compactação).
    fn groups(&self, n: usize, only_threads: bool) -> Vec<(usize, usize)> {
        let mut rest: Vec<usize> = self.t.nodes[n].children.clone();
        let mut out = Vec::new();
        while !rest.is_empty() {
            let walk = rest.remove(0);
            let mut count = 0;
            if self.o.compact && (!only_threads || self.t.nodes[walk].thread) {
                rest.retain(|&s| {
                    if self.t.tree_equal(walk, s, self.o) {
                        count += 1;
                        false
                    } else {
                        true
                    }
                });
            }
            out.push((walk, count));
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn dump(&mut self, n: usize, level: usize, rep: usize, leaf: bool, last: bool, prev_uid: u32, closing: usize) {
        self.ensure(level);
        let o = self.o;
        let sym = o.sym;
        if !leaf {
            for lvl in 0..level {
                for _ in 0..self.width[lvl] + 1 {
                    self.out.char(b' ');
                }
                let s = if lvl == level - 1 {
                    if last { sym.last_2 } else { sym.branch_2 }
                } else if self.more[lvl + 1] {
                    sym.vert_2
                } else {
                    sym.empty_2
                };
                self.out.string(s.as_bytes());
            }
        }
        let mut add = 0;
        if rep >= 2 {
            add = self.out.int(rep as i64) + 2;
            self.out.string(b"*[");
        }
        let tree = self.t;
        let node = &tree.nodes[n];
        let swapped = o.print_args;
        let mut info = o.print_args;
        if swapped && node.swapped {
            self.out.char(b'(');
        }
        let comm_start = self.out.cur_x;
        self.out.string(&node.comm);
        let comm_len = self.out.cur_x - comm_start;
        let offset = self.out.cur_x;
        if o.pids {
            self.out.char(if info { b',' } else { b'(' });
            info = true;
            self.out.int(i64::from(node.pid));
        }
        if o.pgids {
            self.out.char(if info { b',' } else { b'(' });
            info = true;
            self.out.int(i64::from(node.pgid));
        }
        if o.user_change && prev_uid != node.uid {
            self.out.char(if info { b',' } else { b'(' });
            info = true;
            let mut names = common::Names::new();
            match names.user(node.uid) {
                Some(name) => self.out.string(name.as_bytes()),
                None => {
                    self.out.int(i64::from(node.uid));
                }
            }
        }
        if (swapped && node.swapped) || (!swapped && info) {
            self.out.char(b')');
        }
        if o.print_args {
            let n_args = node.args.len();
            for (i, arg) in node.args.iter().enumerate() {
                self.out.char(b' ');
                let len: usize = arg.iter().map(|&c| if (0x20..0x7f).contains(&c) { 1 } else { 5 }).sum();
                let limit = self.out.width.saturating_sub(if i == n_args - 1 { 0 } else { 4 });
                if self.out.cur_x + len <= limit || !self.out.trunc {
                    for &c in arg {
                        if (0x20..0x7f).contains(&c) {
                            self.out.char(c);
                        } else {
                            self.out.string(format!("\\{c:03o}").as_bytes());
                        }
                    }
                } else {
                    self.out.string(b"...");
                    break;
                }
            }
        }
        let has_children = !node.children.is_empty();
        if o.print_args || !has_children {
            for _ in 0..closing {
                self.out.char(b']');
            }
            self.out.newline();
        }
        self.more[level] = !last;
        let uid = node.uid;
        if o.print_args {
            // `swapped + (comm_len > 1 ? 0 : -1)` do original.
            self.width[level] = if comm_len > 1 { usize::from(swapped) } else { usize::from(swapped).saturating_sub(1) };
            let groups = self.groups(n, true);
            let total = groups.len();
            for (k, (walk, count)) in groups.into_iter().enumerate() {
                self.dump(walk, level + 1, count + 1, false, k + 1 == total, uid, usize::from(count > 0));
            }
            return;
        }
        self.width[level] = comm_len + self.out.cur_x - offset + add;
        if self.out.cur_x >= self.out.width && self.out.trunc {
            self.out.string(sym.first_3.as_bytes());
            self.out.string(b"+");
            self.out.newline();
            return;
        }
        let groups = self.groups(n, false);
        let total = groups.len();
        for (k, (walk, count)) in groups.into_iter().enumerate() {
            let next = k + 1 < total;
            if k == 0 {
                self.out.string(if next { sym.first_3 } else { sym.single_3 }.as_bytes());
            }
            self.dump(walk, level + 1, count + 1, k == 0, !next, uid, closing + usize::from(count > 0));
        }
    }
}

/// Largura da saída: `COLUMNS` válido (`strtol` de base 0), senão 132.
fn output_width() -> usize {
    if let Some(v) = sysio::env::var_os("COLUMNS") {
        let s = v.to_string_lossy().into_owned();
        if !s.is_empty() {
            let (digits, radix) = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                (h, 16)
            } else if s.len() > 1 && s.starts_with('0') {
                (&s[1..], 8)
            } else {
                (s.as_str(), 10)
            };
            if let Ok(t) = i64::from_str_radix(digits, radix)
                && t > 0
                && t < 0x7fff_ffff
                && !digits.starts_with(['+', '-'])
            {
                return t as usize;
            }
        }
    }
    132
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage() -> i32 {
    io::eprint(USAGE);
    1
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut o = Opts {
        print_args: false,
        compact: true,
        pids: false,
        pgids: false,
        user_change: false,
        sort_by_pid: false,
        sym: &SYM_ASCII,
    };
    let mut trunc = true;
    let mut hide_threads = false;
    let mut show_parents = false;
    let mut g = Getopt::from_env(&argv[1..], "aAcC:gGhH:nN:pslStTuUVZ", LONGS);
    while let Some(r) = g.next_opt() {
        match r {
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                return usage();
            }
            Ok(opt) => match opt.short() {
                Some('a') => o.print_args = true,
                Some('A') | Some('G') => o.sym = &SYM_ASCII,
                Some('U') => o.sym = &SYM_UTF,
                Some('c') => o.compact = false,
                Some('C') => {
                    if opt.arg_str() != "age" {
                        return usage();
                    }
                }
                Some('g') => {
                    o.pgids = true;
                    o.compact = false;
                }
                Some('h') => {}
                Some('H') => {
                    let a = opt.arg_str();
                    if common::parse_long(&a).is_none() {
                        return usage();
                    }
                }
                Some('l') => trunc = false,
                Some('n') => o.sort_by_pid = true,
                Some('N') => {
                    let a = opt.arg_str();
                    if !["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"].contains(&a.as_str()) {
                        return usage();
                    }
                }
                Some('p') => {
                    o.pids = true;
                    o.compact = false;
                }
                Some('s') => show_parents = true,
                Some('S') | Some('t') | Some('Z') => {}
                Some('T') => hide_threads = true,
                Some('u') => o.user_change = true,
                Some('V') => {
                    io::eprint(VERSION);
                    return 0;
                }
                _ => unreachable!("tabela de opções do pstree"),
            },
        }
    }
    let operands = g.operands();
    if operands.len() > 1 {
        return usage();
    }
    let mut pid = ROOT_PID;
    let mut pid_set = false;
    let mut by_user: Option<u32> = None;
    if let Some(a) = operands.first() {
        let s = String::from_utf8_lossy(a).into_owned();
        if a.first().is_some_and(u8::is_ascii_digit) {
            match common::strtol(&s) {
                common::Strtol::Ok(v) | common::Strtol::Range(v) => pid = v as Pid,
                common::Strtol::Invalid => return usage(),
            }
            pid_set = true;
        } else {
            let mut names = common::Names::new();
            match names.uid_of(&s) {
                Some(u) => by_user = Some(u),
                None => {
                    io::eprint(format!("No such user name: {s}\n"));
                    return 1;
                }
            }
        }
    }

    let mut t = build(o.sort_by_pid, hide_threads);

    if show_parents && pid_set {
        let Some(mut cur) = t.find(pid) else { return 1 };
        while let Some(parent) = t.nodes[cur].parent {
            t.nodes[parent].children = vec![cur];
            cur = parent;
        }
        pid = ROOT_PID;
    }

    let mut status = 0;
    {
        let out_state = Out { buf: Vec::new(), cur_x: 1, width: output_width(), trunc, last_char: None, charlen: 0 };
        let mut d = Dumper { t: &t, o: &o, out: out_state, width: Vec::new(), more: Vec::new() };
        match by_user {
            None => match t.find(pid) {
                Some(n) => d.dump(n, 0, 1, true, true, 0, 0),
                None => status = 1,
            },
            Some(uid) => {
                let roots: Vec<usize> = (0..t.nodes.len()).filter(|&i| t.nodes[i].parent.is_none()).collect();
                let mut stack: Vec<usize> = roots.into_iter().rev().collect();
                while let Some(n) = stack.pop() {
                    if t.nodes[n].uid == uid && !t.nodes[n].thread && t.nodes[n].comm != b"?" {
                        d.dump(n, 0, 1, true, true, uid, 0);
                        continue;
                    }
                    for &c in t.nodes[n].children.iter().rev() {
                        stack.push(c);
                    }
                }
            }
        }
        out(d.out.buf);
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_with_plus_in_last_column() {
        let mut o = Out { buf: Vec::new(), cur_x: 1, width: 5, trunc: true, last_char: None, charlen: 0 };
        o.string(b"abcdefgh");
        o.newline();
        o.string(b"abcde");
        o.newline();
        assert_eq!(o.buf, b"abcd+\nabcde\n");
    }
}
