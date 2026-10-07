//! Análise da linha de comando do ps (parser.c) e o estado inicial (`reset_global`, `set_screen_size`,
//! `set_personality`, de global.c). A ordem de tentativas é a do original: primeiro SysV/Unix98 e
//! GNU, e se algo falha, uma segunda passada em modo BSD; a mensagem mostrada é a da primeira.

use sysabi::{Fd, FileType, OFlags, sys};
use ul_misc::util::io;

use super::proc::read_path;
use super::util::strtoul0;
use super::*;

/// Falha de uma passada do parser: mensagem (`error: ...`) ou saída direta do programa.
pub enum PErr {
    Msg(String),
    Exit(i32),
}

fn msg(s: &str) -> PErr {
    PErr::Msg(s.to_string())
}

type PR = Result<(), PErr>;

/// Onde está o argumento de uma opção: depois da letra `i` de um grupo curto, ou depois do nome
/// longo que acaba em `pos`.
#[derive(Clone, Copy)]
enum ArgAt {
    Short(usize),
    Gnu(usize),
}
use ArgAt::{Gnu, Short};

/// Um valor de lista já interpretado.
enum Sel {
    Num(u64),
    Cmd(Vec<u8>),
}

const SPACES: &[u8] = b" ,\t";

impl Ps {
    // -----------------------------------------------------------------------------------------
    // Estado global (global.c).

    /// `set_screen_size`.
    fn set_screen_size(&mut self) {
        let sysc = sys::current();
        let mut ws = None;
        for fd in [Fd(1), Fd(2), Fd(0)] {
            if let Ok(w) = sysc.tcgetwinsize(fd)
                && w.cols > 0 && w.rows > 0 {
                    ws = Some(w);
                    break;
                }
        }
        if ws.is_none()
            && let Ok(fd) = sys::open(b"/dev/tty", OFlags::NOCTTY | OFlags::NONBLOCK | OFlags::RDONLY, 0) {
                let r = sysc.tcgetwinsize(fd);
                let _ = sys::close(fd);
                if let Ok(w) = r
                    && w.cols > 0 && w.rows > 0 {
                        ws = Some(w);
                    }
            }
        let (cols, rows) = match ws {
            Some(w) => (i32::from(w.cols), i32::from(w.rows)),
            None => (80, 24),
        };
        self.screen_cols = cols;
        self.screen_rows = rows;
        if !sysc.isatty(Fd::STDOUT) {
            self.screen_cols = OUTBUF_SIZE;
        }
        let env_num = |name: &str| -> Option<i64> {
            let v = getenv(name).filter(|v| !v.is_empty())?;
            let (n, used) = strtoul0(&v);
            // `strtol(.., 0)` aceita sinal; os valores válidos são positivos.
            (used == v.len() && n > 0 && n < OUTBUF_SIZE as u64).then_some(n as i64)
        };
        if let Some(c) = env_num("COLUMNS") {
            self.screen_cols = c as i32;
        }
        if let Some(l) = env_num("LINES") {
            self.screen_rows = l as i32;
        }
        if self.screen_cols < 9 || self.screen_rows < 2 {
            io::eprint(format!("your {}x{} screen size is bogus. expect trouble\n", self.screen_cols, self.screen_rows));
        }
    }

    /// `set_personality`: define `personality` e os formatos de cada estilo a partir do ambiente.
    fn set_personality(&mut self) {
        self.personality = 0;
        self.prefer_bsd_defaults = false;
        self.bsd_j_format = Some("OL_j");
        self.bsd_l_format = Some("OL_l");
        self.bsd_s_format = Some("OL_s");
        self.bsd_u_format = Some("OL_u");
        self.bsd_v_format = Some("OL_v");
        self.sysv_f_format = None;
        self.sysv_fl_format = None;
        self.sysv_j_format = None;
        self.sysv_l_format = None;
        let nonempty = |n: &str| getenv(n).filter(|v| !v.is_empty());
        let mut s = nonempty("PS_PERSONALITY").or_else(|| nonempty("CMD_ENV")).unwrap_or_else(|| b"unknown".to_vec());
        if getenv("I_WANT_A_BROKEN_PS").is_some() {
            s = b"old".to_vec();
        }
        if s.len() > 15 {
            return;
        }
        self.saved_personality_text = String::from_utf8_lossy(&s).into_owned();
        let name = String::from_utf8_lossy(&s).to_ascii_lowercase();
        match name.as_str() {
            "bsd" => {
                self.personality = PER_FORCE_BSD | PER_BSD_H | PER_BSD_M;
                self.prefer_bsd_defaults = true;
                self.bsd_j_format = Some("FB_j");
                self.bsd_l_format = Some("FB_l");
                self.bsd_u_format = Some("FB_u");
                self.bsd_v_format = Some("FB_v");
            }
            "old" => {
                self.personality = PER_FORCE_BSD | PER_OLD_M;
                self.prefer_bsd_defaults = true;
            }
            "debian" | "gnu" => {
                self.personality = PER_GOOD_O | PER_OLD_M;
                self.prefer_bsd_defaults = true;
                self.sysv_f_format = Some("RD_f");
                self.sysv_j_format = Some("RD_j");
                self.sysv_l_format = Some("RD_l");
            }
            "linux" => self.personality = PER_GOOD_O | PER_ZAP_ADDR | PER_SANE_USER,
            "default" | "unknown" => {}
            "aix" => {
                self.bsd_j_format = Some("FB_j");
                self.bsd_l_format = Some("FB_l");
                self.bsd_u_format = Some("FB_u");
                self.bsd_v_format = Some("FB_v");
            }
            "tru64" | "compaq" | "digital" => {
                self.personality = PER_GOOD_O | PER_BSD_H;
                self.prefer_bsd_defaults = true;
                self.sysv_f_format = Some("F5FMT");
                self.sysv_fl_format = Some("FL5FMT");
                self.sysv_j_format = Some("JFMT");
                self.sysv_l_format = Some("L5FMT");
                self.bsd_j_format = Some("JFMT");
                self.bsd_l_format = Some("LFMT");
                self.bsd_s_format = Some("SFMT");
                self.bsd_u_format = Some("UFMT");
                self.bsd_v_format = Some("VFMT");
            }
            "sunos4" => {
                self.personality = PER_NO_DEFAULT_G;
                self.prefer_bsd_defaults = true;
                self.bsd_j_format = Some("FB_j");
                self.bsd_l_format = Some("FB_l");
                self.bsd_u_format = Some("FB_u");
                self.bsd_v_format = Some("FB_v");
            }
            "irix" | "sgi" => {
                let xpg_ok = getenv("_XPG").is_some_and(|v| v.first().is_some_and(|c| *c > b'0' && *c <= b'9'));
                if !xpg_ok {
                    self.personality = PER_IRIX_L;
                }
            }
            "os390" | "s390" | "390" => self.sysv_j_format = Some("J390"),
            "hp" | "hpux" => self.personality = PER_HPUX_X,
            "svr4" | "sysv" | "sco" => self.personality = PER_SVR4_X,
            "posix" | "solaris2" | "unix95" | "unix98" | "unix" => {}
            _ => {
                // "environment specified an unknown personality": o chamador ignora o erro, e o
                // estado fica como acima (personalidade 0).
            }
        }
    }

    /// `reset_global`: estado inicial. Sai com 47 se não há `/proc` (a mensagem do original).
    pub(super) fn reset_global(&mut self) -> R<()> {
        // `fatal_proc_unmounted`: o próprio processo tem que ter /proc/self/stat.
        if read_path("/proc/self/stat").filter(|d| !d.is_empty()).is_none() {
            io::eprint("Error, do this: mount -t proc proc /proc\n");
            return Err(47);
        }
        let me = sys::current().getpid();
        let Some(mine) = proc::load_pt(&format!("/proc/{me}"), me, me, true) else {
            io::eprint("fatal library error, lookup self\n");
            return Err(1);
        };
        self.cached_tty = mine.tty;
        self.set_screen_size();
        self.set_personality();
        self.all_processes = false;
        self.bsd_c_option = false;
        self.bsd_e_option = false;
        self.cached_euid = sys::current().geteuid();
        self.forest_prefix.clear();
        self.forest_type = 0;
        self.format_flags = 0;
        self.format_list.clear();
        self.format_modifiers = 0;
        self.header_gap = -1;
        self.header_type = HEAD_SINGLE;
        self.include_dead_children = false;
        self.lines_to_next_header = 1;
        self.negate_selection = false;
        self.page_size = 4096;
        self.running_only = false;
        self.selection_list.clear();
        self.simple_select = 0;
        self.sort_list.clear();
        self.thread_flags = 0;
        self.unix_f_option = false;
        self.user_is_number = false;
        self.wchan_is_number = false;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Listas.

    fn parse_pid(&mut self, s: &[u8]) -> Result<Sel, String> {
        let (num, used) = strtoul0(s);
        if used != s.len() {
            return Err("process ID list syntax error".into());
        }
        if !(1..=0x7fff_ffff).contains(&num) {
            return Err("process ID out of range".into());
        }
        Ok(Sel::Num(num))
    }

    /// Um usuário (`GROUP` falso) ou grupo da lista, por número ou nome.
    fn parse_id<const GROUP: bool>(&mut self, s: &[u8]) -> Result<Sel, String> {
        let what = if GROUP { "group" } else { "user" };
        let (mut num, used) = strtoul0(s);
        if used != s.len() {
            let name = String::from_utf8_lossy(s).into_owned();
            let id = if GROUP { self.names.gid_of(&name) } else { self.names.uid_of(&name) };
            match id {
                Some(id) => num = u64::from(id),
                None => {
                    if !self.negate_selection {
                        return Err(format!("{what} name does not exist"));
                    }
                    num = u64::MAX;
                }
            }
        }
        if !self.negate_selection && num > 0xffff_fffe {
            return Err(format!("{what} ID out of range"));
        }
        Ok(Sel::Num(u64::from(num as u32)))
    }

    fn parse_cmd(&mut self, s: &[u8]) -> Result<Sel, String> {
        let mut v = s.to_vec();
        v.truncate(63);
        Ok(Sel::Cmd(v))
    }

    fn parse_tty(&mut self, s: &[u8]) -> Result<Sel, String> {
        let st_of = |p: &[u8]| sys::stat(p).ok();
        let found = if s.first() == Some(&b'/') {
            match st_of(s) {
                Some(st) => Some(st),
                None => return Err("TTY could not be found".into()),
            }
        } else {
            let txt = String::from_utf8_lossy(s).into_owned();
            let mut hit = None;
            for pat in ["/dev/pts/{}", "/dev/{}", "/dev/tty{}", "/dev/pty{}", "/dev/{}nsole"] {
                let path = pat.replace("{}", &txt);
                if let Some(st) = st_of(path.as_bytes()) {
                    hit = Some(st);
                    break;
                }
            }
            hit
        };
        if let Some(st) = found {
            if st.file_type() != FileType::CharDevice {
                return Err("list member was not a TTY".into());
            }
            return Ok(Sel::Num(st.rdev));
        }
        if s == b"-" || s == b"?" {
            return Ok(Sel::Num(0));
        }
        if s.len() == 1 && st_of(s).is_some() {
            return Ok(Sel::Num(0));
        }
        Err("TTY could not be found".into())
    }

    /// `parse_list`: separa por espaço, vírgula ou tab; empilha o nó na cabeça (o chamador dá o tipo).
    fn parse_list(&mut self, arg: &[u8], f: fn(&mut Ps, &[u8]) -> Result<Sel, String>) -> Result<(), String> {
        let improper = || Err("improper list".to_string());
        let mut need_item = true;
        if arg.is_empty() {
            return improper();
        }
        for c in arg {
            if SPACES.contains(c) {
                if need_item {
                    return improper();
                }
                need_item = true;
            } else {
                need_item = false;
            }
        }
        if need_item {
            return improper();
        }
        let mut node = SelNode { typecode: 0, nums: Vec::new(), cmds: Vec::new() };
        for item in arg.split(|c| SPACES.contains(c)) {
            match f(self, item)? {
                Sel::Num(n) => node.nums.push(n),
                Sel::Cmd(c) => node.cmds.push(c),
            }
        }
        self.selection_list.insert(0, node);
        Ok(())
    }

    /// Atalho: lista com o tipo definido, como `parse_list` seguido de `selection_list->typecode = t`.
    fn list_as(&mut self, arg: &[u8], f: fn(&mut Ps, &[u8]) -> Result<Sel, String>, typecode: u8) -> PR {
        self.parse_list(arg, f).map_err(PErr::Msg)?;
        self.selection_list[0].typecode = typecode;
        Ok(())
    }

    /// Argumento de uma opção curta: o resto do mesmo argv ou o próximo (`get_opt_arg`).
    fn get_opt_arg(&mut self, a: &[u8], i: usize) -> Option<Vec<u8>> {
        if a.len() > i + 1 {
            return Some(a[i + 1..].to_vec());
        }
        if self.thisarg + 2 > self.argv.len() {
            return None;
        }
        let next = &self.argv[self.thisarg + 1];
        if next.is_empty() {
            return None;
        }
        self.thisarg += 1;
        Some(self.argv[self.thisarg].clone())
    }

    /// O argumento obrigatório de uma opção, curta ou longa; sem ele, a mensagem `missing`.
    fn opt_arg(&mut self, a: &[u8], at: ArgAt, missing: &str) -> Result<Vec<u8>, PErr> {
        let arg = match at {
            ArgAt::Short(i) => self.get_opt_arg(a, i),
            ArgAt::Gnu(pos) => self.grab_gnu_arg(a, pos),
        };
        arg.ok_or_else(|| msg(missing))
    }

    /// Opção cujo argumento é uma lista de seleção do tipo `typecode`.
    fn opt_list(&mut self, a: &[u8], at: ArgAt, missing: &str, f: fn(&mut Ps, &[u8]) -> Result<Sel, String>, typecode: u8) -> PR {
        let arg = self.opt_arg(a, at, missing)?;
        self.list_as(&arg, f, typecode)
    }

    /// Opção cujo argumento é uma especificação de formato ou ordenação.
    fn opt_format(&mut self, a: &[u8], at: ArgAt, missing: &str, sf: i32) -> PR {
        let arg = self.opt_arg(a, at, missing)?;
        self.defer_sf_option(&arg, sf);
        Ok(())
    }

    /// Opção longa sem argumento: `--name=x` é erro.
    fn no_arg(no_arg: bool, name: &str) -> PR {
        if no_arg { Ok(()) } else { Err(PErr::Msg(format!("option --{name} does not take an argument"))) }
    }

    /// `--heading`/`--no-heading`: só um tipo de cabeçalho por vez.
    fn heading(&mut self, no_arg: bool, name: &str, header_type: i32) -> PR {
        Ps::no_arg(no_arg, name)?;
        if self.header_type != 0 {
            return Err(msg("only one heading option may be specified"));
        }
        self.header_type = header_type;
        Ok(())
    }

    /// Seleciona o terminal do próprio `ps` (`T`, e `t` sem argumento).
    fn own_tty(&mut self) {
        let node = SelNode { typecode: SEL_TTY, nums: vec![self.cached_tty as i64 as u64], cmds: Vec::new() };
        self.selection_list.insert(0, node);
    }

    fn exclusive(&self, opt: &str) -> PR {
        if self.argv.len() != 2 || self.argv[1] != opt.as_bytes() {
            return Err(PErr::Msg(format!("the option is exclusive: {opt}")));
        }
        Ok(())
    }

    fn version_exit(&self, opt: &str) -> PR {
        self.exclusive(opt)?;
        crate::common::out(format!("{} from procps-ng 4.0.4\n", self.myname));
        Err(PErr::Exit(0))
    }

    // -----------------------------------------------------------------------------------------
    // Opções SysV.

    fn parse_sysv_option(&mut self) -> PR {
        let a = self.argv[self.thisarg].clone();
        let mut i = 1;
        while i < a.len() {
            match a[i] {
                b'A' => self.all_processes = true,
                b'C' => return self.opt_list(&a, Short(i), "list of command names must follow -C", Ps::parse_cmd, SEL_COMM),
                b'D' => self.lstart_format = Some(self.opt_arg(&a, Short(i), "date format must follow -D")?),
                b'F' => {
                    self.format_modifiers |= FM_F;
                    self.format_flags |= FF_UF;
                    self.unix_f_option = true;
                }
                b'G' => return self.opt_list(&a, Short(i), "list of real groups must follow -G", Ps::parse_id::<true>, SEL_RGID),
                b'H' => self.forest_type = b'u',
                b'L' => self.thread_flags |= TF_U_L,
                b'M' => self.format_modifiers |= FM_M,
                b'N' => self.negate_selection = true,
                b'O' => return self.opt_format(&a, Short(i), "format or sort specification must follow -O", SF_U_O_UP),
                b'P' => self.format_modifiers |= FM_P,
                b'T' => self.thread_flags |= TF_U_T,
                b'U' => return self.opt_list(&a, Short(i), "list of real users must follow -U", Ps::parse_id::<false>, SEL_RUID),
                b'V' => return self.version_exit("-V"),
                b'Z' => self.format_modifiers |= FM_M,
                b'a' => self.simple_select |= SS_U_A,
                b'c' => self.format_modifiers |= FM_C,
                b'd' => self.simple_select |= SS_U_D,
                b'e' => self.all_processes = true,
                b'f' => {
                    self.format_flags |= FF_UF;
                    self.unix_f_option = true;
                }
                b'g' => {
                    let arg = self.opt_arg(&a, Short(i), "list of session leaders OR effective group names must follow -g")?;
                    if self.parse_list(&arg, Ps::parse_pid).is_ok() {
                        self.selection_list[0].typecode = SEL_SESS;
                        return Ok(());
                    }
                    if self.parse_list(&arg, Ps::parse_id::<true>).is_ok() {
                        self.selection_list[0].typecode = SEL_EGID;
                        return Ok(());
                    }
                    return Err(msg("list of session leaders OR effective group IDs was invalid"));
                }
                b'j' => {
                    if self.sysv_j_format.is_some() {
                        self.format_flags |= FF_UJ;
                    } else {
                        self.format_modifiers |= FM_J;
                    }
                }
                b'l' => self.format_flags |= FF_UL,
                b'm' => self.thread_flags |= TF_U_M,
                b'o' => return self.opt_format(&a, Short(i), "format specification must follow -o", SF_U_O),
                b'p' => return self.opt_list(&a, Short(i), "list of process IDs must follow -p", Ps::parse_pid, SEL_PID),
                b'q' => return self.opt_list(&a, Short(i), "List of process IDs must follow -q.", Ps::parse_pid, SEL_PID_QUICK),
                b's' => return self.opt_list(&a, Short(i), "list of session IDs must follow -s", Ps::parse_pid, SEL_SESS),
                b't' => return self.opt_list(&a, Short(i), "list of terminals (pty, tty...) must follow -t", Ps::parse_tty, SEL_TTY),
                b'u' => return self.opt_list(&a, Short(i), "list of users must follow -u", Ps::parse_id::<false>, SEL_EUID),
                b'w' => self.w_count += 1,
                b'x' => {
                    if self.personality & PER_SVR4_X != 0 {
                        self.format_modifiers |= FM_Y;
                    } else if self.personality & PER_HPUX_X != 0 {
                        self.w_count += 2;
                        self.unix_f_option = true;
                    } else {
                        return Err(msg("must set personality to get -x option"));
                    }
                }
                b'y' => self.format_modifiers |= FM_Y,
                b'-' => return Err(msg("embedded '-' among SysV options makes no sense")),
                _ => return Err(msg("unsupported SysV option")),
            }
            i += 1;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Opções BSD.

    fn parse_bsd_option(&mut self) -> PR {
        let a = self.argv[self.thisarg].clone();
        let has_dash = a.first() == Some(&b'-');
        if has_dash {
            if !self.force_bsd {
                return Err(msg("cannot happen - problem #1"));
            }
        } else if self.personality & PER_FORCE_BSD != 0 {
            if !self.force_bsd {
                return Err(msg("cannot happen - problem #2"));
            }
        } else if self.force_bsd {
            return Err(msg("second chance parse failed, not BSD or SysV"));
        }
        let mut i = usize::from(has_dash);
        while i < a.len() {
            match a[i] {
                b'0'..=b'9' => {
                    let arg = a[i..].to_vec();
                    return self.list_as(&arg, Ps::parse_pid, SEL_PID);
                }
                b'H' => self.thread_flags |= TF_B_H,
                b'L' => {
                    self.exclusive("L")?;
                    self.print_format_specifiers();
                    return Err(PErr::Exit(0));
                }
                b'M' => self.thread_flags |= TF_B_M,
                b'O' => return self.opt_format(&a, Short(i), "format or sort specification must follow O", SF_B_O_UP),
                b'S' => self.include_dead_children = true,
                b'T' => self.own_tty(),
                b'U' => return self.opt_list(&a, Short(i), "list of users must follow U", Ps::parse_id::<false>, SEL_EUID),
                b'V' => return self.version_exit("V"),
                b'W' => return Err(msg("obsolete W option not supported (you have a /dev/drum?)")),
                b'X' => self.format_flags |= FF_LX,
                b'Z' => self.format_modifiers |= FM_M,
                b'a' => self.simple_select |= SS_B_A,
                b'c' => self.bsd_c_option = true,
                b'e' => self.bsd_e_option = true,
                b'f' => self.forest_type = b'b',
                b'g' => self.simple_select |= SS_B_G,
                b'h' => self.heading(true, "", if self.personality & PER_BSD_H != 0 { HEAD_MULTI } else { HEAD_NONE })?,
                b'j' => self.format_flags |= FF_BJ,
                b'k' => return self.opt_format(&a, Short(i), "long sort specification must follow 'k'", SF_G_SORT),
                b'l' => self.format_flags |= FF_BL,
                b'm' => {
                    if self.personality & PER_OLD_M != 0 {
                        self.format_flags |= FF_LM;
                    } else if self.personality & PER_BSD_M != 0 {
                        self.defer_sf_option(b"pmem", SF_B_M);
                    } else {
                        self.thread_flags |= TF_B_M;
                    }
                }
                b'n' => {
                    self.wchan_is_number = true;
                    self.user_is_number = true;
                }
                b'o' => return self.opt_format(&a, Short(i), "format specification must follow o", SF_B_O),
                b'p' => return self.opt_list(&a, Short(i), "list of process IDs must follow p", Ps::parse_pid, SEL_PID),
                b'q' => return self.opt_list(&a, Short(i), "List of process IDs must follow q.", Ps::parse_pid, SEL_PID_QUICK),
                b'r' => self.running_only = true,
                b's' => self.format_flags |= FF_BS,
                b't' => match self.get_opt_arg(&a, i) {
                    None => {
                        self.own_tty();
                        return Ok(());
                    }
                    Some(arg) => return self.list_as(&arg, Ps::parse_tty, SEL_TTY),
                },
                b'u' => self.format_flags |= FF_BU,
                b'v' => self.format_flags |= FF_BV,
                b'w' => self.w_count += 1,
                b'x' => self.simple_select |= SS_B_X,
                b'-' => return Err(msg("embedded '-' among BSD options makes no sense")),
                _ => return Err(msg("unsupported option (BSD syntax)")),
            }
            i += 1;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Opções longas do GNU.

    /// `grab_gnu_arg`: o que vem depois de `=` ou `:`, ou o próximo argumento.
    fn grab_gnu_arg(&mut self, a: &[u8], pos: usize) -> Option<Vec<u8>> {
        match a.get(pos) {
            Some(b'=') | Some(b':') => {
                if a.len() > pos + 1 {
                    Some(a[pos + 1..].to_vec())
                } else {
                    None
                }
            }
            Some(_) => None,
            None => {
                if self.thisarg + 2 > self.argv.len() {
                    return None;
                }
                if self.argv[self.thisarg + 1].is_empty() {
                    return None;
                }
                self.thisarg += 1;
                Some(self.argv[self.thisarg].clone())
            }
        }
    }

    fn parse_gnu_option(&mut self) -> PR {
        const NAMES: &[&str] = &[
            "Group", "User", "cols", "columns", "context", "cumulative", "date-format", "deselect", "forest", "format", "group",
            "header", "headers", "heading", "headings", "info", "lines", "no-header", "no-headers", "no-heading", "no-headings",
            "noheader", "noheaders", "noheading", "noheadings", "pid", "ppid", "quick-pid", "rows", "sid", "signames", "sort",
            "tty", "user", "version", "width",
        ];
        let a = self.argv[self.thisarg].clone();
        let s = &a[2..];
        let sl = s.iter().position(|c| *c == b':' || *c == b'=').unwrap_or(s.len());
        if sl > 15 {
            return Err(msg("unknown gnu long option"));
        }
        let name = String::from_utf8_lossy(&s[..sl]).into_owned();
        let pos = 2 + sl;
        let no_arg = sl == s.len();
        let known = NAMES.contains(&name.as_str());
        if !known {
            if name.as_bytes() == b"help" {
                let arg = self.grab_gnu_arg(&a, pos);
                let rc = self.do_help(arg.as_deref(), 0);
                return Err(PErr::Exit(rc));
            }
            return Err(msg("unknown gnu long option"));
        }
        match name.as_str() {
            "Group" => self.opt_list(&a, Gnu(pos), "list of real groups must follow --Group", Ps::parse_id::<true>, SEL_RGID),
            "User" => self.opt_list(&a, Gnu(pos), "list of real users must follow --User", Ps::parse_id::<false>, SEL_RUID),
            "cols" | "width" | "columns" | "rows" | "lines" => {
                let rows = name == "rows" || name == "lines";
                if let Some(arg) = self.grab_gnu_arg(&a, pos)
                    && !arg.is_empty() {
                        let (t, used) = strtol_arg(&arg);
                        if used == arg.len() && t > 0 && t < 2_000_000_000 {
                            if rows {
                                self.screen_rows = t as i32;
                            } else {
                                self.screen_cols = t as i32;
                            }
                            return Ok(());
                        }
                    }
                Err(msg(if rows {
                    "number of rows must follow --rows or --lines"
                } else {
                    "number of columns must follow --cols, --width, or --columns"
                }))
            }
            "cumulative" => {
                Ps::no_arg(no_arg, "cumulative")?;
                self.include_dead_children = true;
                Ok(())
            }
            "date-format" => {
                self.lstart_format = Some(self.opt_arg(&a, Gnu(pos), "date format must follow --date-format")?);
                Ok(())
            }
            "deselect" => {
                Ps::no_arg(no_arg, "deselect")?;
                self.negate_selection = true;
                Ok(())
            }
            "no-header" | "no-headers" | "no-heading" | "no-headings" | "noheader" | "noheaders" | "noheading" | "noheadings" => {
                self.heading(no_arg, "no-heading", HEAD_NONE)
            }
            "header" | "headers" | "heading" | "headings" => self.heading(no_arg, "heading", HEAD_MULTI),
            "forest" => {
                Ps::no_arg(no_arg, "forest")?;
                self.forest_type = b'g';
                Ok(())
            }
            "format" => self.opt_format(&a, Gnu(pos), "format specification must follow --format", SF_G_FORMAT),
            "group" => self.opt_list(&a, Gnu(pos), "list of effective groups must follow --group", Ps::parse_id::<true>, SEL_EGID),
            "info" => {
                self.exclusive("--info")?;
                self.self_info();
                Err(PErr::Exit(0))
            }
            "pid" => self.opt_list(&a, Gnu(pos), "list of process IDs must follow --pid", Ps::parse_pid, SEL_PID),
            "quick-pid" => self.opt_list(&a, Gnu(pos), "List of process IDs must follow --quick-pid.", Ps::parse_pid, SEL_PID_QUICK),
            "ppid" => self.opt_list(&a, Gnu(pos), "list of process IDs must follow --ppid", Ps::parse_pid, SEL_PPID),
            "sid" => self.opt_list(&a, Gnu(pos), "some sid thing(s) must follow --sid", Ps::parse_pid, SEL_SESS),
            "signames" => {
                self.signal_names = true;
                Ok(())
            }
            "sort" => self.opt_format(&a, Gnu(pos), "long sort specification must follow --sort", SF_G_SORT),
            "tty" => self.opt_list(&a, Gnu(pos), "list of ttys must follow --tty", Ps::parse_tty, SEL_TTY),
            "user" => self.opt_list(&a, Gnu(pos), "list of effective users must follow --user", Ps::parse_id::<false>, SEL_EUID),
            "version" => self.version_exit("--version"),
            "context" => {
                self.format_flags |= FF_FC;
                Ok(())
            }
            _ => Err(msg("unknown gnu long option")),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Pids soltos no fim (`ps 1 2 -3 +4`).

    fn parse_trailing_pids(&mut self) -> PR {
        let rest: Vec<Vec<u8>> = self.argv[self.thisarg..].to_vec();
        self.thisarg = self.argv.len() - 1;
        let mut pids: Vec<u64> = Vec::new();
        let mut grps: Vec<u64> = Vec::new();
        let mut sess: Vec<u64> = Vec::new();
        for data in &rest {
            let (target, text): (&mut Vec<u64>, &[u8]) = match data.first() {
                Some(b'-') => (&mut grps, &data[1..]),
                Some(b'+') => (&mut sess, &data[1..]),
                _ => (&mut pids, &data[..]),
            };
            match self.parse_pid(text) {
                Ok(Sel::Num(n)) => target.push(n),
                Ok(_) => {}
                Err(e) => return Err(PErr::Msg(e)),
            }
        }
        for (list, code) in [(pids, SEL_PID), (grps, SEL_PGRP), (sess, SEL_SESS)] {
            if !list.is_empty() {
                self.selection_list.insert(0, SelNode { typecode: code, nums: list, cmds: Vec::new() });
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Laço principal do parser.

    fn parse_all_options(&mut self) -> PR {
        #[derive(PartialEq)]
        enum At {
            Gnu,
            End,
            Pgrp,
            Sysv,
            Pid,
            Bsd,
            Fail,
            Sess,
        }
        fn arg_type(s: &[u8]) -> At {
            let c0 = s.first().copied().unwrap_or(0);
            if c0.is_ascii_alphabetic() {
                return At::Bsd;
            }
            if c0.is_ascii_digit() {
                return At::Pid;
            }
            if c0 == b'+' {
                return At::Sess;
            }
            if c0 != b'-' {
                return At::Fail;
            }
            let c1 = s.get(1).copied().unwrap_or(0);
            if c1.is_ascii_alphabetic() {
                return At::Sysv;
            }
            if c1.is_ascii_digit() {
                return At::Pgrp;
            }
            if c1 != b'-' {
                return At::Fail;
            }
            let c2 = s.get(2).copied().unwrap_or(0);
            if c2.is_ascii_alphabetic() {
                return At::Gnu;
            }
            if c2 == 0 {
                return At::End;
            }
            At::Fail
        }
        loop {
            self.thisarg += 1;
            if self.thisarg >= self.argv.len() {
                break;
            }
            let at = arg_type(&self.argv[self.thisarg]);
            match at {
                At::Gnu => self.parse_gnu_option()?,
                At::Sysv | At::Bsd => {
                    if at == At::Sysv && !self.force_bsd {
                        self.parse_sysv_option()?;
                    } else {
                        if at == At::Bsd && self.force_bsd && self.personality & PER_FORCE_BSD == 0 {
                            return Err(msg("way bad"));
                        }
                        self.prefer_bsd_defaults = true;
                        self.parse_bsd_option()?;
                    }
                }
                At::Pgrp | At::Sess | At::Pid => {
                    self.prefer_bsd_defaults = true;
                    self.parse_trailing_pids()?;
                }
                At::End | At::Fail => return Err(msg("garbage option")),
            }
        }
        Ok(())
    }

    fn choose_dimensions(&mut self) {
        if self.w_count != 0 && self.screen_cols < 132 {
            self.screen_cols = 132;
        }
        if self.w_count > 1 {
            self.screen_cols = OUTBUF_SIZE;
        }
    }

    fn thread_option_check(&mut self) -> PR {
        if self.thread_flags == 0 {
            self.thread_flags = TF_SHOW_PROC;
            return Ok(());
        }
        let tf = self.thread_flags;
        if self.forest_type != 0 {
            return Err(msg("thread display conflicts with forest display"));
        }
        if tf & TF_B_H != 0 && tf & (TF_B_M | TF_U_M) != 0 {
            return Err(msg("thread flags conflict; can't use H with m or -m"));
        }
        if tf & TF_B_M != 0 && tf & TF_U_M != 0 {
            return Err(msg("thread flags conflict; can't use both m and -m"));
        }
        if tf & TF_U_L != 0 && tf & TF_U_T != 0 {
            return Err(msg("thread flags conflict; can't use both -L and -T"));
        }
        if tf & TF_B_H != 0 {
            self.thread_flags |= TF_SHOW_PROC | TF_LOOSE_TASKS;
        }
        if tf & (TF_B_M | TF_U_M) != 0 {
            self.thread_flags |= TF_SHOW_PROC | TF_SHOW_TASK | TF_SHOW_BOTH;
        }
        if tf & (TF_U_T | TF_U_L) != 0 {
            if tf & (TF_B_M | TF_U_M | TF_B_H) != 0 {
                self.thread_flags |= TF_MUST_USE;
            } else {
                self.thread_flags |= TF_SHOW_TASK;
            }
        }
        Ok(())
    }

    /// Uma passada completa: opções, threads, ordenação e formato, seleção e dimensões.
    fn parse_pass(&mut self) -> PR {
        self.parse_all_options()?;
        self.thread_option_check()?;
        self.process_sf_options().map_err(PErr::Msg)?;
        self.select_bits_setup().map_err(PErr::Msg)?;
        self.choose_dimensions();
        Ok(())
    }

    /// `arg_parse`: SysV primeiro; se falha, BSD. Sai com 1 e o uso se as duas falham.
    pub(super) fn arg_parse(&mut self) -> R<()> {
        let mut first_err: Option<String> = None;
        if self.personality & PER_FORCE_BSD == 0 {
            match self.parse_pass() {
                Ok(()) => return Ok(()),
                Err(PErr::Exit(c)) => return Err(c),
                Err(PErr::Msg(m)) => first_err = Some(m),
            }
        }
        // try_bsd
        self.reset_global()?;
        self.w_count = 0;
        self.reset_sortformat();
        self.format_flags = 0;
        self.thisarg = 0;
        self.force_bsd = true;
        self.prefer_bsd_defaults = true;
        if (PER_OLD_M | PER_BSD_M) & self.personality == 0 {
            self.personality |= PER_OLD_M;
        }
        let err2 = match self.parse_pass() {
            Ok(()) => return Ok(()),
            Err(PErr::Exit(c)) => return Err(c),
            Err(PErr::Msg(m)) => m,
        };
        self.w_count = 0;
        let shown = if self.personality & PER_FORCE_BSD != 0 { err2 } else { first_err.unwrap_or(err2) };
        io::eprint(format!("error: {shown}\n"));
        Err(self.do_help(None, 1))
    }
}

/// `strtol(arg, &end, 0)` para `--cols`/`--rows`: valor e bytes consumidos.
fn strtol_arg(arg: &[u8]) -> (i64, usize) {
    let (neg, body) = match arg.first() {
        Some(b'-') => (true, &arg[1..]),
        Some(b'+') => (false, &arg[1..]),
        _ => (false, arg),
    };
    let (v, used) = strtoul0(body);
    let skipped = arg.len() - body.len();
    let v = v.min(i64::MAX as u64) as i64;
    (if neg { -v } else { v }, if used == 0 { 0 } else { used + skipped })
}
