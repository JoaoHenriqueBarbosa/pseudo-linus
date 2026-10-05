//! O quadro do top em modo batch: o resumo (carga, tarefas, CPUs, memória), o cabeçalho das
//! colunas e as linhas de tarefas (top.c: `summary_show`, `do_cpus`, `do_memory`, `sum_see`,
//! `show_special`, `calibrate_fields`, `window_show`, `window_hlp` e `task_show`).

use sysabi::{Fd, sys};

use super::data::HistTic;
use super::fields::{self, EU_CMD, EU_CPU, EU_MEM, EU_NCE, EU_PID, EU_PRI, EU_RES, EU_SHR, EU_STA};
use super::fields::{EU_TM2, EU_UEN, EU_VRT};
use super::text::{
    TICS_AS_SECS, make_chr, make_num, make_str, make_str_utf8, scale_mem, scale_pcnt, scale_tics, utf8_delta,
    utf8_embody, utf8_justify,
};
use super::{R, Top};
use crate::ps::display::cmp_keys;

/// `ROWMAXSIZ`: o tamanho do buffer de uma linha do quadro.
const ROWMAXSIZ: usize = 2048;
/// `W_MIN_COL` e `W_MIN_ROW`.
const W_MIN_COL: i32 = 3;
const W_MIN_ROW: i32 = 3;
/// `SCREENMAX`.
pub const SCREENMAX: i32 = 512;
/// Separador entre duas CPUs (ou memória e swap) na mesma linha (`Adjoin_sp`).
const ADJOIN_SP: &str = " ~6 ~1";

/// Uma escala de memória do resumo: divisor, casas decimais e rótulo.
struct MemScale {
    div: f32,
    prec: usize,
    label: &'static str,
}

static MEM_SCALES: [MemScale; 6] = [
    MemScale { div: 1.0, prec: 0, label: "KiB" },
    MemScale { div: 1024.0, prec: 1, label: "MiB" },
    MemScale { div: 1024.0 * 1024.0, prec: 1, label: "GiB" },
    MemScale { div: 1024.0 * 1024.0 * 1024.0, prec: 1, label: "TiB" },
    MemScale { div: 1024.0 * 1024.0 * 1024.0 * 1024.0, prec: 1, label: "PiB" },
    MemScale { div: 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0, prec: 1, label: "EiB" },
];

/// `strtol(s, &end, 0)` completo: `Some` só se o texto todo é um número positivo.
fn env_positive(name: &str) -> Option<i64> {
    let v = sys::getenv(name).filter(|v| !v.is_empty())?;
    let (n, used) = crate::ps::util::strtoul0(&v);
    (used == v.len() && n > 0 && n <= 0x7fff_ffff).then_some(n as i64)
}

impl Top {
    // -----------------------------------------------------------------------------------------
    // Saída.

    /// `PUFF` em modo batch: corta no tamanho do buffer e tira os espaços do fim da linha.
    pub fn puff(&mut self, s: &[u8]) {
        let mut end = s.len().min(ROWMAXSIZ - 1);
        while end > 0 && s[end - 1] == b' ' {
            end -= 1;
        }
        self.out.extend_from_slice(&s[..end]);
    }

    /// `show_special`: troca as marcas `~N` (que em batch não têm efeito) e corta cada linha na
    /// largura da tela.
    pub fn show_special(&mut self, glob: &[u8]) {
        let mut glob = glob;
        while let Some(nl) = glob.iter().position(|b| *b == b'\n') {
            let lin: Vec<u8> = glob[..nl.min(509)].to_vec();
            let mut room = self.screen_cols;
            let (mut sub_beg, mut sub_end) = (0usize, 0usize);
            let mut row: Vec<u8> = Vec::new();
            while sub_beg < lin.len() {
                let mut ch = lin.get(sub_end).map_or(0, |b| i32::from(*b));
                if ch == i32::from(b'~') {
                    ch = lin.get(sub_end + 1).map_or(0, |b| i32::from(*b)) - i32::from(b'0');
                }
                if (0..=8).contains(&ch) {
                    let seg_end = sub_end.min(lin.len());
                    let seg = &lin[sub_beg..seg_end];
                    let fit = utf8_embody(seg, room).min(seg.len());
                    row.extend_from_slice(&seg[..fit]);
                    room -= seg.len() as i32;
                    room += utf8_delta(seg);
                    sub_end += 2;
                    sub_beg = if ch == 0 { lin.len() } else { sub_end };
                } else {
                    sub_end += 1;
                }
                if room <= 0 {
                    break;
                }
            }
            row.push(b'\n');
            self.puff(&row);
            glob = &glob[nl + 1..];
        }
    }

    // -----------------------------------------------------------------------------------------
    // O resumo.

    /// `sum_see`: junta linhas do resumo duas a duas (ou mais) quando `double_up` pede, e as imprime.
    /// Devolve quantas linhas foram impressas.
    fn sum_see(&mut self, s: &str, nobuf: bool) -> i32 {
        self.sum_row.extend_from_slice(s.as_bytes());
        if !s.is_empty() && self.w.double_up != 0 && !nobuf {
            self.sum_tog += 1;
            if self.sum_tog <= self.w.double_up {
                self.sum_row.extend_from_slice(ADJOIN_SP.as_bytes());
                return 0;
            }
        }
        if self.sum_row.is_empty() {
            return 0;
        }
        self.sum_row.push(b'\n');
        let row = std::mem::take(&mut self.sum_row);
        self.show_special(&row);
        self.sum_tog = 0;
        1
    }

    /// `sum_tics`: os percentuais de uma linha de CPU.
    fn sum_tics(&mut self, h: &HistTic, pfx: &str, nobuf: bool) -> i32 {
        let mut d = h.delta();
        let mut idl_frme = d.il;
        let mut tot_frme = d.sum_tot;
        if tot_frme < 1 {
            idl_frme = 1;
            tot_frme = 1;
        }
        let scale = (100.0f64 / f64::from(tot_frme as f32)) as f32;
        // Os ticks de máquina virtual entram no sistema.
        d.sy += d.gu + d.gn;
        let pc = |v: i64| f64::from((v as f32) * scale);
        let line = format!(
            "{pfx}~3{:5.1} ~2us,~3{:5.1} ~2sy,~3{:5.1} ~2ni,~3{:5.1} ~2id,~3{:5.1} ~2wa,~3{:5.1} ~2hi,~3{:5.1} ~2si,~3{:5.1} ~2st~3 ~1",
            pc(d.us),
            pc(d.sy),
            pc(d.ni),
            pc(idl_frme),
            pc(d.io),
            pc(d.ir),
            pc(d.si),
            pc(d.st),
        );
        self.sum_see(&line, nobuf)
    }

    /// `do_cpus`: a linha única de todas as CPUs ou uma por CPU, se a tela tem espaço.
    fn do_cpus(&mut self) {
        let no_mas = |t: &Top| i64::from(t.msg_row) + 1 >= t.screen_rows - 1;
        if self.w.view_cpusum {
            let h = self.stat.summary;
            self.msg_row += self.sum_tics(&h, "%Cpu(s):", true);
        } else {
            let n = self.cpu_cnt as usize;
            for i in 0..n {
                let Some(h) = self.stat.cpus.get(i).copied() else { break };
                let pfx = format!("%Cpu{:<3}:", h.id);
                self.msg_row += self.sum_tics(&h, &pfx, i + 1 >= n);
                if no_mas(self) {
                    break;
                }
            }
        }
        // Despeja o que ficou esperando um par.
        self.msg_row += self.sum_see("", true);
    }

    /// `do_memory`: as linhas de memória e de swap na escala de `-E`.
    fn do_memory(&mut self) {
        let sc = &MEM_SCALES[self.summ_mscale.min(5)];
        let m = self.mem;
        let my_qued = m.buffers.wrapping_add(m.cached);
        let bf = |v: u64| -> String {
            let x = (v as f32) / sc.div;
            let mut s = format!("{:.*} ", sc.prec, f64::from(x));
            if s.len() > 9 {
                s.truncate(8);
                s.push('+');
            }
            s
        };
        let (b0, b1, b2, b3) = (bf(m.total), bf(m.free), bf(m.used), bf(my_qued));
        let (b4, b5, b6, b7) = (bf(m.swap_total), bf(m.swap_free), bf(m.swap_used), bf(m.avail));
        let line1 = format!(
            "{} Mem :~3 {b0:>9.9}~2total,~3 {b1:>9.9}~2free,~3 {b2:>9.9}~2used,~3 {b3:>9.9}~2buff/cache~3 ~1    ",
            sc.label
        );
        self.msg_row += self.sum_see(&line1, false);
        let line2 = format!(
            "{} Swap:~3 {b4:>9.9}~2total,~3 {b5:>9.9}~2free,~3 {b6:>9.9}~2used.~3 {b7:>9.9}~2avail Mem ~3",
            sc.label
        );
        self.msg_row += self.sum_see(&line2, true);
    }

    /// `summary_show`.
    pub fn summary_show(&mut self) {
        let room = |t: &Top, n: i64| i64::from(t.msg_row) + n < t.screen_rows - 1;
        if room(self, 1) {
            let line = format!("{} -{}\n", self.myname, self.uptime_sprint());
            self.show_special(line.as_bytes());
            self.msg_row += 1;
        }
        if room(self, 2) {
            let what = if self.thread_mode { "Threads" } else { "Tasks" };
            let c = self.counts;
            let line = format!(
                "{what}:~3 {:3} ~2total,~3 {:3} ~2running,~3 {:3} ~2sleeping,~3 {:3} ~2stopped,~3 {:3} ~2zombie~3\n",
                c.total,
                c.running,
                c.sleeping + c.other,
                c.stopped,
                c.zombied
            );
            self.show_special(line.as_bytes());
            self.msg_row += 1;
            self.do_cpus();
        }
        if room(self, 2) {
            self.do_memory();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Geometria e cabeçalho.

    /// `adj_geometry`: colunas e linhas do quadro (em batch, `-w` ou `COLUMNS` mandam).
    pub fn adj_geometry(&mut self) {
        // O que o ncurses deixa em `columns` e `lines` (o terminfo do dumb, mudado por COLUMNS/LINES).
        let mut cols = env_positive("COLUMNS").map_or(80, |v| v as i32);
        if let Some(sysc) = sys::try_current() {
            if let Ok(ws) = sysc.tcgetwinsize(Fd::STDOUT) {
                if ws.cols > 0 && ws.rows > 0 {
                    cols = i32::from(ws.cols);
                }
            }
        }
        cols = cols.clamp(W_MIN_COL, SCREENMAX);
        if !self.w_set {
            if self.width_mode > 0 {
                self.w_cols = self.width_mode;
            } else if self.width_mode < 0 {
                if let Some(c) = env_positive("COLUMNS") {
                    self.w_cols = c as i32;
                }
                if let Some(l) = env_positive("LINES") {
                    self.w_rows = l as i32;
                }
                if self.w_cols == 0 {
                    self.w_cols = SCREENMAX;
                }
                if self.w_cols != 0 && self.w_cols < W_MIN_COL {
                    self.w_cols = W_MIN_COL;
                }
                if self.w_rows != 0 && self.w_rows < W_MIN_ROW {
                    self.w_rows = W_MIN_ROW;
                }
            }
            if self.w_cols > SCREENMAX {
                self.w_cols = SCREENMAX;
            }
            self.w_set = true;
        }
        if self.w_cols != 0 {
            cols = self.w_cols;
        }
        self.screen_cols = cols;
        self.screen_rows = if self.w_rows != 0 { i64::from(self.w_rows) } else { i64::from(i32::MAX) };
    }

    /// `calibrate_fields`: quais colunas cabem na tela, a largura de COMMAND e o cabeçalho.
    pub fn calibrate_fields(&mut self) {
        self.adj_geometry();
        let pid_w = self.pid_width;
        let mut shown: Vec<usize> = Vec::new();
        let mut len_so_far = 0usize;
        let mut varcolcnt = 0i32;
        let mut varcolsz = 0i32;
        for &f in fields::DEFAULT_FIELDS.iter() {
            let h = fields::NAMES[f];
            let var = fields::width(f, pid_w) == -1;
            let width = fields::width(f, pid_w);
            let len = (if var { h.len() } else { width as usize }) + 1;
            // Não cabe: as colunas seguintes também ficam de fora.
            if (self.screen_cols as i64) < (len_so_far + len) as i64 {
                break;
            }
            if var {
                varcolcnt += 1;
                varcolsz += h.len() as i32;
            }
            len_so_far += len;
            shown.push(f);
        }
        varcolsz += self.screen_cols - len_so_far as i32;
        if varcolcnt != 0 {
            varcolsz /= varcolcnt;
        }
        self.varcolsz = varcolsz;
        self.procflgs = shown;
        self.build_header();
    }

    /// `build_headers`: o texto do cabeçalho com a largura final das colunas.
    fn build_header(&mut self) {
        let mut hdr: Vec<u8> = Vec::new();
        for &f in &self.procflgs {
            let w = if fields::width(f, self.pid_width) == -1 { self.varcolsz } else { fields::width(f, self.pid_width) };
            hdr.extend(utf8_justify(fields::NAMES[f].as_bytes(), w, fields::align_right(f)));
        }
        self.columnhdr = hdr;
    }

    // -----------------------------------------------------------------------------------------
    // As tarefas.

    /// `wins_usrselect`: a tarefa passa pelo filtro `-u`/`-U`?
    fn usrselect(&self, idx: usize) -> bool {
        let p = &self.tasks[idx].p;
        let w = &self.w;
        match w.usrseltyp {
            0 => return true,
            b'U' => {
                if p.ruid == w.usrseluid || p.suid == w.usrseluid || p.fuid == w.usrseluid {
                    return w.usrselflg;
                }
                if p.euid == w.usrseluid {
                    return w.usrselflg;
                }
            }
            _ => {
                if p.euid == w.usrseluid {
                    return w.usrselflg;
                }
            }
        }
        !w.usrselflg
    }

    /// `task_show`: o texto da linha da tarefa `idx` (vazio se nenhuma coluna cabe).
    fn task_row(&mut self, idx: usize) -> Vec<u8> {
        let mut row: Vec<u8> = Vec::new();
        let flgs = self.procflgs.clone();
        let pid_w = self.pid_width;
        for f in flgs {
            let w = fields::width(f, pid_w);
            let cell = {
                let t = &self.tasks[idx];
                let p = &t.p;
                match f {
                    EU_PID => make_num(i64::from(p.tid), w, true),
                    EU_STA => make_chr(p.state, w, false),
                    EU_PRI => {
                        if p.priority < -99 || p.priority > 999 {
                            make_str(b"rt", w, true)
                        } else {
                            make_num(i64::from(p.priority), w, true)
                        }
                    }
                    EU_NCE => make_num(i64::from(p.nice), w, true),
                    EU_VRT => scale_mem(self.task_mscale, (p.statm_all()[0] << 2) as f32, w, true),
                    EU_RES => scale_mem(self.task_mscale, (p.statm_all()[1] << 2) as f32, w, true),
                    EU_SHR => scale_mem(self.task_mscale, (p.statm_all()[2] << 2) as f32, w, true),
                    EU_CPU => {
                        let mut u = t.pcpu as f32;
                        let n = p.nlwp;
                        u *= self.frame_etscale;
                        if f64::from(u) > 100.0 * f64::from(n) {
                            u = (100.0 * f64::from(n)) as f32;
                        }
                        if u > self.cpu_pmax {
                            u = self.cpu_pmax;
                        }
                        scale_pcnt(u, w, true, false)
                    }
                    EU_MEM => {
                        let res = (p.statm_all()[1] << 2) as f32;
                        scale_pcnt(res * 100.0f32 / (self.mem.total as f32), w, true, false)
                    }
                    EU_TM2 => {
                        let tics = if self.w.show_ctimes {
                            p.utime.wrapping_add(p.stime).wrapping_add(p.cutime).wrapping_add(p.cstime)
                        } else {
                            p.utime.wrapping_add(p.stime)
                        };
                        scale_tics(tics, self.hertz, w, true, TICS_AS_SECS)
                    }
                    EU_UEN => {
                        let name = self.ps.user_name(p.euid);
                        make_str_utf8(&name, w, false)
                    }
                    EU_CMD => {
                        let which: Vec<u8> = if self.w.show_cmdline { p.cmdline().to_vec() } else { p.cmd.clone() };
                        make_str_utf8(&which, self.varcolsz, false)
                    }
                    _ => continue,
                }
            };
            row.extend(cell);
        }
        row
    }

    /// Imprime a linha da tarefa e diz se ela contou (não está vazia).
    fn print_task(&mut self, idx: usize) -> bool {
        let row = self.task_row(idx);
        let mut line = vec![b'\n'];
        line.extend_from_slice(&row);
        self.puff(&line);
        !row.is_empty()
    }

    /// `window_hlp`: acha a primeira tarefa visível, de onde a listagem parte.
    fn window_hlp(&mut self) {
        let end = self.counts.total as i64;
        let beg = 0i64;
        let mut reversed = false;
        self.begtask += self.begnext;
        if self.begtask <= beg {
            self.begtask = beg;
            self.begnext = 1;
        } else if self.begtask >= end {
            self.begtask = end - 1;
        }
        // Uma tarefa é visível se passa pelo filtro de usuário e a linha dela não é vazia.
        let visible = |t: &mut Top, i: i64| -> bool { t.usrselect(i as usize) && !t.task_row(i as usize).is_empty() };
        'fwd: loop {
            if self.begnext > 0 {
                let mut i = self.begtask;
                while i < end {
                    if visible(self, i) {
                        break;
                    }
                    i += 1;
                }
                if i < end {
                    self.begtask = i;
                    break 'fwd;
                }
                self.begtask = end - 1;
            }
            let mut i = self.begtask;
            while i > beg {
                if visible(self, i) {
                    break;
                }
                i -= 1;
            }
            self.begtask = i;
            if self.begtask == beg && !reversed && !visible(self, beg) {
                reversed = true;
                // Sem sorte para trás: tenta de novo para a frente, uma vez só.
                self.begnext = 1;
                continue 'fwd;
            }
            break;
        }
        self.begnext = 0;
    }

    /// `window_show`: o cabeçalho e as tarefas que cabem em `wmax` linhas. Devolve as linhas usadas.
    pub fn window_show(&mut self, wmax: i64) -> R<i64> {
        let hdr = self.columnhdr.clone();
        let mut line = vec![b'\n'];
        line.extend_from_slice(&hdr);
        self.puff(&line);
        let n = self.counts.total as usize;
        if n == 0 {
            return Ok(1);
        }
        // Ordena a partir da ordem em que as tarefas foram lidas.
        let sortindx = self.w.sortindx;
        let desc = self.w.qsrt_normal;
        let tasks = std::mem::take(&mut self.tasks);
        let keys: Vec<_> = tasks.iter().map(|t| self.sort_key(sortindx, t)).collect();
        let mut order: Vec<usize> = (0..n).collect();
        if n >= 2 {
            order.sort_by(|a, b| {
                let o = cmp_keys(&keys[*a], &keys[*b]);
                if desc { o.reverse() } else { o }
            });
        }
        let mut old: Vec<Option<super::Task>> = tasks.into_iter().map(Some).collect();
        let mut sorted: Vec<super::Task> = Vec::with_capacity(n);
        for i in order {
            if let Some(t) = old[i].take() {
                sorted.push(t);
            }
        }
        self.tasks = sorted;

        if self.begnext != 0 {
            self.window_hlp();
        }
        let winlines = self.screen_rows - i64::from(self.msg_row) - 1;
        let wmax = wmax.min(winlines + 1);
        let mut i = self.begtask.max(0) as usize;
        let mut lwin = 1i64;
        while i < n && lwin < wmax {
            let busy = self.tasks[i].pcpu > 0;
            if (self.w.show_idleps || busy) && self.usrselect(i) && self.print_task(i) {
                lwin += 1;
            }
            i += 1;
        }
        Ok(lwin)
    }

    /// `zap_fieldstab`, a parte feita uma vez: largura do PID e o limite do %CPU.
    pub fn zap_fieldstab(&mut self) -> R<()> {
        let mut digits = 5;
        let n = self.ps.pid_length();
        if n > 5 {
            if n > 10 {
                return self.error_exit("failed pid maximum size test");
            }
            digits = n;
        }
        self.pid_width = digits;
        let mut pmax = 99.9f64;
        if self.cpu_cnt > 1 && !self.thread_mode {
            pmax = 100.0 * f64::from(self.cpu_cnt);
            if self.cpu_cnt > 1000 {
                pmax = pmax.min(9_999_999.0);
            } else if self.cpu_cnt > 100 {
                // O original compara o número de CPUs com 999999 (nunca é verdade nesta faixa).
            } else if self.cpu_cnt > 10 {
                pmax = pmax.min(99_999.0);
            } else {
                pmax = pmax.min(999.9);
            }
        }
        self.cpu_pmax = pmax as f32;
        self.calibrate_fields();
        Ok(())
    }
}
