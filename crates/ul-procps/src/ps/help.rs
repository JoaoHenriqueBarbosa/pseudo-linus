//! Ajuda do ps (`--help [seção]`, help.c) e o `--info` (global.c).

use ul_misc::util::io;

use super::*;

const WORDS: [(&str, &str); 6] =
    [("simple", "s"), ("list", "l"), ("output", "o"), ("threads", "t"), ("misc", "m"), ("all", "a")];

impl Ps {
    /// `do_help`: escreve a ajuda (stdout se `rc` é 0, senão stderr) e devolve `rc`, que é o código
    /// de saída do programa.
    pub(super) fn do_help(&self, opt: Option<&[u8]>, rc: i32) -> i32 {
        let section = match opt {
            Some(o) => WORDS.iter().position(|(w, a)| o == w.as_bytes() || o == a.as_bytes()),
            None => None,
        };
        // 0 simple, 1 list, 2 output, 3 threads, 4 misc, 5 all, None = padrão.
        let show = |idx: usize| section == Some(idx) || section == Some(5);
        let mut s = format!("\nUsage:\n {} [options]\n", self.myname);
        if show(0) {
            s.push_str("\nBasic options:\n");
            s.push_str(" -A, -e               all processes\n");
            s.push_str(" -a                   all with tty, except session leaders\n");
            s.push_str("  a                   all with tty, including other users\n");
            s.push_str(" -d                   all except session leaders\n");
            s.push_str(" -N, --deselect       negate selection\n");
            s.push_str("  r                   only running processes\n");
            s.push_str("  T                   all processes on this terminal\n");
            s.push_str("  x                   processes without controlling ttys\n");
        }
        if show(1) {
            s.push_str("\nSelection by list:\n");
            s.push_str(" -C <command>         command name\n");
            s.push_str(" -G, --Group <GID>    real group id or name\n");
            s.push_str(" -g, --group <group>  session or effective group name\n");
            s.push_str(" -p, p, --pid <PID>   process id\n");
            s.push_str("        --ppid <PID>  parent process id\n");
            s.push_str(" -q, q, --quick-pid <PID>\n                      process id (quick mode)\n");
            s.push_str(" -s, --sid <session>  session id\n");
            s.push_str(" -t, t, --tty <tty>   terminal\n");
            s.push_str(" -u, U, --user <UID>  effective user id or name\n");
            s.push_str(" -U, --User <UID>     real user id or name\n");
            s.push_str("\n  The selection options take as their argument either:\n    a comma-separated list e.g. '-u root,nobody' or\n    a blank-separated list e.g. '-p 123 4567'\n");
        }
        if show(2) {
            s.push_str("\nOutput formats:\n");
            s.push_str(" -D <format>          date format for lstart\n");
            s.push_str(" -F                   extra full\n");
            s.push_str(" -f                   full-format, including command lines\n");
            s.push_str("  f, --forest         ascii art process tree\n");
            s.push_str(" -H                   show process hierarchy\n");
            s.push_str(" -j                   jobs format\n");
            s.push_str("  j                   BSD job control format\n");
            s.push_str(" -l                   long format\n");
            s.push_str("  l                   BSD long format\n");
            s.push_str(" -M, Z                add security data (for SELinux)\n");
            s.push_str(" -O <format>          preloaded with default columns\n");
            s.push_str("  O <format>          as -O, with BSD personality\n");
            s.push_str(" -o, o, --format <format>\n                      user-defined format\n");
            s.push_str("  -P                  add psr column\n");
            s.push_str("  s                   signal format\n");
            s.push_str("  u                   user-oriented format\n");
            s.push_str("  v                   virtual memory format\n");
            s.push_str("  X                   register format\n");
            s.push_str(" -y                   do not show flags, show rss vs. addr (used with -l)\n");
            s.push_str("     --context        display security context (for SELinux)\n");
            s.push_str("     --headers        repeat header lines, one per page\n");
            s.push_str("     --no-headers     do not print header at all\n");
            s.push_str("     --cols, --columns, --width <num>\n                      set screen width\n");
            s.push_str("     --rows, --lines <num>\n                      set screen height\n");
            s.push_str("     --signames       display signal masks using signal names\n");
        }
        if show(3) {
            s.push_str("\nShow threads:\n");
            s.push_str("  H                   as if they were processes\n");
            s.push_str(" -L                   possibly with LWP and NLWP columns\n");
            s.push_str(" -m, m                after processes\n");
            s.push_str(" -T                   possibly with SPID column\n");
        }
        if show(4) {
            s.push_str("\nMiscellaneous options:\n");
            s.push_str(" -c                   show scheduling class with -l option\n");
            s.push_str("  c                   show true command name\n");
            s.push_str("  e                   show the environment after command\n");
            s.push_str("  k,    --sort        specify sort order as: [+|-]key[,[+|-]key[,...]]\n");
            s.push_str("  L                   show format specifiers\n");
            s.push_str("  n                   display numeric uid and wchan\n");
            s.push_str("  S,    --cumulative  include some dead child process data\n");
            s.push_str(" -y                   do not show flags, show rss (only with -l)\n");
            s.push_str(" -V, V, --version     display version information and exit\n");
            s.push_str(" -w, w                unlimited output width\n");
            s.push_str(&format!(
                "\n        --help <{}|{}|{}|{}|{}|{}>\n                      display help and exit\n",
                WORDS[0].0, WORDS[1].0, WORDS[2].0, WORDS[3].0, WORDS[4].0, WORDS[5].0
            ));
        }
        if section.is_none() {
            s.push_str(&format!(
                "\n Try '{} --help <{}|{}|{}|{}|{}|{}>'\n  or '{} --help <{}|{}|{}|{}|{}|{}>'\n for additional help text.\n",
                self.myname,
                WORDS[0].0,
                WORDS[1].0,
                WORDS[2].0,
                WORDS[3].0,
                WORDS[4].0,
                WORDS[5].0,
                self.myname,
                WORDS[0].1,
                WORDS[1].1,
                WORDS[2].1,
                WORDS[3].1,
                WORDS[4].1,
                WORDS[5].1
            ));
        }
        s.push_str("\nFor more details see ps(1).\n");
        if rc == 0 {
            crate::common::out(s);
        } else {
            io::eprint(s);
        }
        rc
    }

    /// `--info` (self_info): vai para o stderr.
    pub(super) fn self_info(&self) {
        let f = |o: Option<&str>| o.unwrap_or("(none)").to_string();
        let mut s = format!(
            "BSD j    {}\nBSD l    {}\nBSD s    {}\nBSD u    {}\nBSD v    {}\nSysV -f  {}\nSysV -fl {}\nSysV -j  {}\nSysV -l  {}\n\n",
            f(self.bsd_j_format),
            f(self.bsd_l_format),
            f(self.bsd_s_format),
            f(self.bsd_u_format),
            f(self.bsd_v_format),
            f(self.sysv_f_format),
            f(self.sysv_fl_format),
            f(self.sysv_j_format),
            f(self.sysv_l_format)
        );
        s.push_str("procps-ng version 4.0.4\n");
        s.push_str("Compiled with: glibc 2.41, gcc 14.2\n\n");
        s.push_str(&format!(
            "header_gap={} lines_to_next_header={}\nscreen_cols={} screen_rows={}\n\n",
            self.header_gap, self.lines_to_next_header, self.screen_cols, self.screen_rows
        ));
        let tty = self.cached_tty as u32;
        s.push_str(&format!(
            "personality=0x{:08x} (from \"{}\")\nEUID={} TTY={},{} page_size={}\n",
            self.personality,
            self.saved_personality_text,
            self.cached_euid as i32,
            crate::common::dev_major(u64::from(tty)),
            crate::common::dev_minor(u64::from(tty)),
            self.page_size
        ));
        s.push_str("sizeof(proc_t)=8 sizeof(long)=8 sizeof(long)=8\n");
        s.push_str("archdefs: x86_64\n");
        io::eprint(s);
    }
}
