//! Interpretador de bytecode TrueType (`ttinterp.c`, versão 40 com `SUBPIXEL_HINTING_MINIMAL`,
//! como o `libfreetype6` do Debian é compilado) e o estado por tamanho do driver (`ttobjs.c`):
//! `fpgm`, `prep`, CVT, armazenamento e zona crepuscular.
//!
//! No C, o contexto de execução compartilha ponteiros com o tamanho (CVT, armazenamento,
//! definições e a zona crepuscular). Como cada tamanho tem um único contexto, os dados
//! compartilhados moram aqui no próprio contexto; o que o C desvia para cópias privadas durante
//! o programa do glifo (`glyfCvt`, `glyfStorage`) vira um vetor à parte com uma marca de desvio
//! que o `TT_Load_Context` desfaz.

use std::rc::Rc;

use crate::calc::{div_fix, hypot, msb, mul_div, mul_div_no_round, mul_fix};
use crate::outline::Vector;
use crate::sfnt::{u16_at, u32_at, Sfnt};
use crate::Error;

pub(crate) const TAG_ON: u8 = 0x01;
pub(crate) const TOUCH_X: u8 = 0x08;
pub(crate) const TOUCH_Y: u8 = 0x10;
pub(crate) const TOUCH_BOTH: u8 = TOUCH_X | TOUCH_Y;
pub(crate) const HAS_SCANMODE: u8 = 0x04;

const MAX_RUNNABLE_OPCODES: u64 = 1_000_000;
const INTERPRETER_VERSION: i64 = 40;
const CALL_SIZE: usize = 32;

const RANGE_FONT: usize = 1;
const RANGE_CVT: usize = 2;
const RANGE_GLYPH: usize = 3;

const ROUND_TO_HALF_GRID: i32 = 0;
const ROUND_TO_GRID: i32 = 1;
const ROUND_TO_DOUBLE_GRID: i32 = 2;
const ROUND_DOWN_TO_GRID: i32 = 3;
const ROUND_UP_TO_GRID: i32 = 4;
const ROUND_OFF: i32 = 5;
const ROUND_SUPER: i32 = 6;
const ROUND_SUPER_45: i32 = 7;

/// `Pop_Push_Count`: quantos valores cada instrução tira (nibble alto) e põe (nibble baixo).
#[rustfmt::skip]
const POP_PUSH: [u8; 256] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x02, 0x02, 0x00, 0x50,
    0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00, 0x00, 0x10, 0x00, 0x10, 0x10, 0x10, 0x10,
    0x12, 0x10, 0x00, 0x22, 0x01, 0x11, 0x10, 0x20, 0x00, 0x10, 0x20, 0x10, 0x10, 0x00, 0x10, 0x10,
    0x00, 0x00, 0x00, 0x00, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00, 0x20, 0x20, 0x00, 0x00, 0x20, 0x20,
    0x00, 0x00, 0x20, 0x11, 0x20, 0x11, 0x11, 0x11, 0x20, 0x21, 0x21, 0x01, 0x01, 0x00, 0x00, 0x10,
    0x21, 0x21, 0x21, 0x21, 0x21, 0x21, 0x11, 0x11, 0x10, 0x00, 0x21, 0x21, 0x11, 0x10, 0x10, 0x10,
    0x21, 0x21, 0x21, 0x21, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x20, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00, 0x00, 0x00, 0x10, 0x10,
    0x00, 0x20, 0x20, 0x00, 0x00, 0x10, 0x20, 0x20, 0x11, 0x10, 0x33, 0x21, 0x21, 0x10, 0x20, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10,
    0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10,
    0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
    0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
];

/// `opcode_length`: negativo para os `NPUSH`, cujo tamanho vem do byte seguinte.
fn opcode_length(op: u8) -> i64 {
    match op {
        0x40 => -1,
        0x41 => -2,
        0xB0..=0xB7 => i64::from(op - 0xB0) + 2,
        0xB8..=0xBF => 2 * i64::from(op - 0xB8) + 3,
        _ => 1,
    }
}

/// `TT_MulFix14_long_long`: os argumentos são `FT_Int32`.
fn mul_fix14(a: i64, b: i16) -> i64 {
    let mut r = i64::from(a as i32) * i64::from(b);
    r = r.wrapping_add(0x2000 + (r >> 63));
    i64::from((r >> 14) as i32)
}

/// `TT_DotFix14_long_long`.
fn dot_fix14(ax: i64, ay: i64, bx: i16, by: i16) -> i64 {
    let t1 = i64::from(ax as i32) * i64::from(bx);
    let t2 = i64::from(ay as i32) * i64::from(by);
    let mut t = t1.wrapping_add(t2);
    t = t.wrapping_add(0x2000 + (t >> 63));
    i64::from((t >> 14) as i32)
}

fn pix_floor(x: i64) -> i64 {
    x & -64
}

fn pix_round_l(x: i64) -> i64 {
    x.wrapping_add(32) & -64
}

fn pix_ceil_l(x: i64) -> i64 {
    x.wrapping_add(63) & -64
}

/// `FT_Vector_NormLen`, só o vetor; a aritmética é a de 32 bits do C.
fn norm_len(vx: i64, vy: i64) -> (i64, i64) {
    let (x_, y_) = (vx as i32, vy as i32);
    let (mut sx, mut sy) = (1i64, 1i64);
    let mut x = if x_ < 0 {
        sx = -1;
        0u32.wrapping_sub(x_ as u32)
    } else {
        x_ as u32
    };
    let mut y = if y_ < 0 {
        sy = -1;
        0u32.wrapping_sub(y_ as u32)
    } else {
        y_ as u32
    };
    if x == 0 {
        return if y > 0 { (vx, sy * 0x10000) } else { (vx, vy) };
    } else if y == 0 {
        return if x > 0 { (sx * 0x10000, vy) } else { (vx, vy) };
    }
    let est = |x: u32, y: u32| if x > y { x.wrapping_add(y >> 1) } else { y.wrapping_add(x >> 1) };
    let mut l = est(x, y);
    let mut shift = 31 - msb(l);
    shift -= 15 + i32::from(l >= (0xAAAA_AAAAu32 >> shift));
    if shift > 0 {
        x <<= shift;
        y <<= shift;
        l = est(x, y);
    } else {
        x >>= -shift;
        y >>= -shift;
        l >>= -shift;
    }
    let mut b = 0x10000i32.wrapping_sub(l as i32);
    let (xs, ys) = (x as i32, y as i32);
    let (mut u, mut v);
    loop {
        u = xs.wrapping_add(xs.wrapping_mul(b) >> 16) as u32;
        v = ys.wrapping_add(ys.wrapping_mul(b) >> 16) as u32;
        let mut z = (u.wrapping_mul(u).wrapping_add(v.wrapping_mul(v)) as i32).wrapping_neg() / 0x200;
        z = z.wrapping_mul((0x10000 + b) >> 8) / 0x10000;
        b = b.wrapping_add(z);
        if z <= 0 {
            break;
        }
    }
    (sx * i64::from(u), sy * i64::from(v))
}

/// `FT_UnitVector` (2.14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnitVector {
    pub x: i16,
    pub y: i16,
}

const X_AXIS: UnitVector = UnitVector { x: 0x4000, y: 0 };

/// `Normalize`: o vetor nulo deixa o destino como está.
fn normalize(vx: i64, vy: i64, r: &mut UnitVector) {
    if vx == 0 && vy == 0 {
        return;
    }
    let (x, y) = norm_len(vx, vy);
    r.x = (x / 4) as i16;
    r.y = (y / 4) as i16;
}

/// `TT_GraphicsState`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gs {
    pub rp0: u16,
    pub rp1: u16,
    pub rp2: u16,
    pub dual: UnitVector,
    pub proj: UnitVector,
    pub free: UnitVector,
    pub loop_: i64,
    pub minimum_distance: i64,
    pub round_state: i32,
    pub auto_flip: bool,
    pub control_value_cutin: i64,
    pub single_width_cutin: i64,
    pub single_width_value: i64,
    pub delta_base: u16,
    pub delta_shift: u16,
    pub instruct_control: u8,
    pub scan_control: bool,
    pub scan_type: i32,
    pub gep0: u16,
    pub gep1: u16,
    pub gep2: u16,
}

/// `tt_default_graphics_state`.
pub(crate) const DEFAULT_GS: Gs = Gs {
    rp0: 0,
    rp1: 0,
    rp2: 0,
    dual: X_AXIS,
    proj: X_AXIS,
    free: X_AXIS,
    loop_: 1,
    minimum_distance: 64,
    round_state: ROUND_TO_GRID,
    auto_flip: true,
    control_value_cutin: 68,
    single_width_cutin: 0,
    single_width_value: 0,
    delta_base: 9,
    delta_shift: 3,
    instruct_control: 0,
    scan_control: false,
    scan_type: 0,
    gep0: 1,
    gep1: 1,
    gep2: 1,
};

/// `TT_Size_Metrics`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TtMetrics {
    pub x_ratio: i64,
    pub y_ratio: i64,
    pub ppem: u16,
    pub ratio: i64,
    pub scale: i64,
    pub compensations: [i64; 4],
}

/// `TT_GlyphZoneRec`. Os contornos guardam o índice absoluto do último ponto, como no C,
/// e `first_point` é o deslocamento da zona dentro do contorno do glifo.
#[derive(Clone, Debug, Default)]
pub(crate) struct Zone {
    pub n_points: usize,
    pub org: Vec<Vector>,
    pub cur: Vec<Vector>,
    pub orus: Vec<Vector>,
    pub tags: Vec<u8>,
    pub contours: Vec<usize>,
    pub first_point: usize,
}

impl Zone {
    fn with_points(n: usize) -> Zone {
        Zone {
            n_points: n,
            org: vec![Vector::default(); n],
            cur: vec![Vector::default(); n],
            orus: vec![Vector::default(); n],
            tags: vec![0; n],
            contours: Vec::new(),
            first_point: 0,
        }
    }
}

/// `TT_DefRecord`.
#[derive(Clone, Copy, Debug, Default)]
struct Def {
    range: i32,
    start: i64,
    end: i64,
    opc: u32,
    active: bool,
}

/// `TT_CallRec`, com a definição como índice.
#[derive(Clone, Copy, Debug, Default)]
struct CallRec {
    caller_range: i32,
    caller_ip: i64,
    cur_count: i64,
    idef: bool,
    def: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Proj {
    X,
    Y,
    Proj,
    Dual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Any,
    X,
    Y,
}

/// O `maxp` com os ajustes do `tt_face_load_maxp`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Maxp {
    max_twilight_points: u16,
    max_storage: u16,
    max_function_defs: u16,
    max_instruction_defs: u16,
    max_stack_elements: u16,
}

/// O que o driver lê da fonte para o bytecode: `fpgm`, `prep`, `cvt `, `maxp` e `hdmx`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Programs {
    fpgm: Option<Rc<[u8]>>,
    prep: Option<Rc<[u8]>>,
    cvt: Vec<i64>,
    maxp: Maxp,
    num_glyphs: u64,
    /// `hdmx`: (ppem, deslocamento das larguras no `data`), ordenado por ppem.
    hdmx: Vec<(u8, usize)>,
    hdmx_record_size: usize,
    /// Onde o `hdmx` começa no `data`.
    hdmx_base: usize,
}

impl Programs {
    pub fn load(sfnt: &Sfnt, data: &[u8]) -> Programs {
        let mut p = Programs { num_glyphs: u64::from(sfnt.num_glyphs), ..Programs::default() };
        let rc = |t: Option<&[u8]>| t.filter(|t| !t.is_empty()).map(Rc::from);
        p.fpgm = rc(sfnt.table(data, b"fpgm"));
        p.prep = rc(sfnt.table(data, b"prep"));
        if let Some(t) = sfnt.table(data, b"cvt ") {
            p.cvt = t.chunks_exact(2).map(|c| i64::from(i16::from_be_bytes([c[0], c[1]]))).collect();
        }
        if let Some(t) = sfnt.table(data, b"maxp") {
            if u32_at(t, 0).is_some_and(|v| v >= 0x0001_0000) && t.len() >= 32 {
                let g = |o| u16_at(t, o).unwrap_or(0);
                p.maxp = Maxp {
                    max_twilight_points: g(16).min(0xFFFF - 4),
                    max_storage: g(18),
                    max_function_defs: g(20).max(64),
                    max_instruction_defs: g(22),
                    max_stack_elements: g(24),
                };
            }
        }
        // `tt_face_load_hdmx`.
        if let Some((off, len)) = sfnt.table_range(data, b"hdmx") {
            let t = &data[off..off + len];
            if len >= 8 {
                let num_records = usize::from(u16_at(t, 2).unwrap_or(0));
                let mut record_size = u32_at(t, 4).unwrap_or(0);
                if record_size >= 0xFFFF_0000 {
                    record_size &= 0xFFFF;
                }
                let want = (u64::from(sfnt.num_glyphs) + 2 + 3) & !3;
                if (1..=255).contains(&num_records) && u64::from(record_size) == want {
                    let rs = record_size as usize;
                    let mut q = 8usize;
                    for _ in 0..num_records {
                        if q + rs > len {
                            break;
                        }
                        p.hdmx.push((t[q], off + q + 2));
                        q += rs;
                    }
                    p.hdmx.sort_by_key(|r| r.0);
                    p.hdmx_record_size = rs;
                    p.hdmx_base = off;
                }
            }
        }
        p
    }

    /// `tt_face_get_device_metrics(face, ppem, 0)`: onde começam as larguras desse ppem.
    fn device_widths(&self, ppem: u16) -> Option<usize> {
        if self.hdmx_record_size <= 2 {
            return None;
        }
        self.hdmx.iter().find(|r| u16::from(r.0) == ppem).map(|r| r.1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ready {
    Pending,
    Done(Option<Error>),
}

/// `TT_ExecContextRec`, com os dados que o C compartilha com o tamanho.
pub(crate) struct Exec {
    error: Option<Error>,
    top: i64,
    stack: Vec<i64>,
    args: i64,
    new_top: i64,
    /// 0 é a zona crepuscular, 1 é a do glifo (`pts`).
    zones: [Zone; 2],
    zp: [usize; 3],
    twilight_points: usize,
    point_size: i64,
    x_ppem: u16,
    y_ppem: u16,
    x_scale: i64,
    y_scale: i64,
    ttm: TtMetrics,
    pub gs: Gs,
    ini_range: usize,
    cur_range: usize,
    code: Rc<[u8]>,
    ip: i64,
    opcode: u8,
    length: i64,
    step_ins: bool,
    ranges: [Option<Rc<[u8]>>; 3],
    cvt: Vec<i64>,
    glyf_cvt: Vec<i64>,
    cvt_diverted: bool,
    storage: Vec<i64>,
    glyf_storage: Vec<i64>,
    storage_diverted: bool,
    fdefs: Vec<Def>,
    num_fdefs: usize,
    idefs: Vec<Def>,
    num_idefs: usize,
    max_func: u32,
    max_ins: u32,
    call: Vec<CallRec>,
    call_top: usize,
    period: i64,
    phase: i64,
    threshold: i64,
    is_composite: bool,
    pedantic: bool,
    f_dot_p: i64,
    round: i32,
    project: Proj,
    dualproj: Proj,
    mover: Move,
    stretched: bool,
    pub grayscale: bool,
    pub subpixel_hinting_lean: bool,
    pub vertical_lcd_lean: bool,
    pub backward_compatibility: bool,
    iupx_called: bool,
    iupy_called: bool,
    pub grayscale_cleartype: bool,
    loopcall_counter: u64,
    loopcall_counter_max: u64,
    neg_jump_counter: u64,
    neg_jump_counter_max: u64,
    num_glyphs: u64,
    /// O programa do glifo corrente (`glyphIns`).
    pub glyph_ins: Option<Rc<[u8]>>,
}

/// `TT_SizeRec`: o que sobrevive entre cargas para um tamanho.
pub(crate) struct TtSize {
    pub bytecode_ready: Ready,
    pub cvt_ready: Ready,
    /// `hinted_metrics`.
    pub x_ppem: u16,
    pub y_ppem: u16,
    pub x_scale: i64,
    pub y_scale: i64,
    pub ttm: TtMetrics,
    pub point_size: i64,
    pub widthp: Option<usize>,
    pub gs: Gs,
    num_fdefs: usize,
    num_idefs: usize,
    max_func: u32,
    max_ins: u32,
    ranges: [Option<Rc<[u8]>>; 3],
    pub exec: Option<Box<Exec>>,
}

impl Default for TtSize {
    fn default() -> TtSize {
        TtSize {
            bytecode_ready: Ready::Pending,
            cvt_ready: Ready::Pending,
            x_ppem: 0,
            y_ppem: 0,
            x_scale: 0,
            y_scale: 0,
            ttm: TtMetrics::default(),
            point_size: 0,
            widthp: None,
            gs: DEFAULT_GS,
            num_fdefs: 0,
            num_idefs: 0,
            max_func: 0,
            max_ins: 0,
            ranges: [None, None, None],
            exec: None,
        }
    }
}

impl TtSize {
    /// `tt_size_reset` com o `point_size` do `tt_size_request` (72 dpi).
    #[allow(clippy::too_many_arguments)]
    pub fn reset(&mut self, progs: &Programs, head_flags: u16, upem: i64, x_ppem: u16, y_ppem: u16, x_scale: i64, y_scale: i64) {
        self.x_ppem = x_ppem;
        self.y_ppem = y_ppem;
        if head_flags & 8 != 0 {
            self.x_scale = div_fix(i64::from(x_ppem) << 6, upem);
            self.y_scale = div_fix(i64::from(y_ppem) << 6, upem);
        } else {
            self.x_scale = x_scale;
            self.y_scale = y_scale;
        }
        if x_ppem >= y_ppem {
            self.ttm.scale = self.x_scale;
            self.ttm.ppem = x_ppem;
            self.ttm.x_ratio = 0x10000;
            self.ttm.y_ratio = div_fix(i64::from(y_ppem), i64::from(x_ppem));
        } else {
            self.ttm.scale = self.y_scale;
            self.ttm.ppem = y_ppem;
            self.ttm.x_ratio = div_fix(i64::from(x_ppem), i64::from(y_ppem));
            self.ttm.y_ratio = 0x10000;
        }
        self.widthp = progs.device_widths(x_ppem);
        self.cvt_ready = Ready::Pending;
        self.point_size = mul_div(i64::from(self.ttm.ppem), 64 * 72, 72);
    }

    fn save_context(&mut self, exec: &Exec) {
        self.num_fdefs = exec.num_fdefs;
        self.num_idefs = exec.num_idefs;
        self.max_func = exec.max_func;
        self.max_ins = exec.max_ins;
        self.ranges = exec.ranges.clone();
    }

    /// `tt_size_run_fpgm`.
    fn run_fpgm(&mut self, progs: &Programs) -> Option<Error> {
        let mut exec = self.exec.take().expect("contexto criado no init_bytecode");
        exec.load_context(self, progs);
        exec.call_top = 0;
        exec.top = 0;
        exec.period = 64;
        exec.phase = 0;
        exec.threshold = 0;
        exec.f_dot_p = 0x4000;
        exec.pedantic = false;
        exec.x_ppem = 0;
        exec.y_ppem = 0;
        exec.x_scale = 0;
        exec.y_scale = 0;
        exec.ttm.ppem = 0;
        exec.ttm.scale = 0;
        exec.ttm.ratio = 0x10000;
        exec.ranges[RANGE_FONT - 1] = progs.fpgm.clone();
        exec.ranges[RANGE_CVT - 1] = None;
        exec.ranges[RANGE_GLYPH - 1] = None;
        let err = if progs.fpgm.is_some() {
            exec.goto_range(RANGE_FONT, 0);
            exec.run().err()
        } else {
            None
        };
        self.bytecode_ready = Ready::Done(err);
        if err.is_none() {
            self.save_context(&exec);
        }
        self.exec = Some(exec);
        err
    }

    /// `tt_size_run_prep`.
    pub fn run_prep(&mut self, progs: &Programs) -> Option<Error> {
        let mut exec = self.exec.take().expect("contexto criado no init_bytecode");
        for (c, &f) in exec.cvt.iter_mut().zip(&progs.cvt) {
            *c = mul_fix(f, self.ttm.scale);
        }
        exec.load_context(self, progs);
        exec.call_top = 0;
        exec.top = 0;
        exec.pedantic = false;
        exec.ranges[RANGE_CVT - 1] = progs.prep.clone();
        exec.ranges[RANGE_GLYPH - 1] = None;
        let err = if progs.prep.is_some() {
            exec.goto_range(RANGE_CVT, 0);
            exec.run().err()
        } else {
            None
        };
        self.cvt_ready = Ready::Done(err);
        exec.gs.dual = X_AXIS;
        exec.gs.proj = X_AXIS;
        exec.gs.free = X_AXIS;
        exec.gs.rp0 = 0;
        exec.gs.rp1 = 0;
        exec.gs.rp2 = 0;
        exec.gs.gep0 = 1;
        exec.gs.gep1 = 1;
        exec.gs.gep2 = 1;
        exec.gs.loop_ = 1;
        self.gs = exec.gs;
        self.save_context(&exec);
        self.exec = Some(exec);
        err
    }

    /// `tt_size_init_bytecode`.
    fn init_bytecode(&mut self, progs: &Programs) -> Option<Error> {
        let m = progs.maxp;
        self.bytecode_ready = Ready::Pending;
        self.cvt_ready = Ready::Pending;
        self.num_fdefs = 0;
        self.num_idefs = 0;
        self.max_func = 0;
        self.max_ins = 0;
        self.ttm.compensations = [0; 4];
        let n_twilight = usize::from(m.max_twilight_points) + 4;
        self.exec = Some(Box::new(Exec {
            error: None,
            top: 0,
            stack: Vec::new(),
            args: 0,
            new_top: 0,
            zones: [Zone::with_points(n_twilight), Zone::default()],
            zp: [1, 1, 1],
            twilight_points: n_twilight,
            point_size: 0,
            x_ppem: 0,
            y_ppem: 0,
            x_scale: 0,
            y_scale: 0,
            ttm: TtMetrics::default(),
            gs: DEFAULT_GS,
            ini_range: 0,
            cur_range: 0,
            code: Rc::from(Vec::new()),
            ip: 0,
            opcode: 0,
            length: 0,
            step_ins: false,
            ranges: [None, None, None],
            cvt: vec![0; progs.cvt.len()],
            glyf_cvt: Vec::new(),
            cvt_diverted: false,
            storage: vec![0; usize::from(m.max_storage)],
            glyf_storage: Vec::new(),
            storage_diverted: false,
            fdefs: vec![Def::default(); usize::from(m.max_function_defs)],
            num_fdefs: 0,
            idefs: vec![Def::default(); usize::from(m.max_instruction_defs)],
            num_idefs: 0,
            max_func: 0,
            max_ins: 0,
            call: vec![CallRec::default(); CALL_SIZE],
            call_top: 0,
            period: 0,
            phase: 0,
            threshold: 0,
            is_composite: false,
            pedantic: false,
            f_dot_p: 0,
            round: ROUND_TO_HALF_GRID,
            project: Proj::Proj,
            dualproj: Proj::Proj,
            mover: Move::Any,
            stretched: false,
            grayscale: false,
            subpixel_hinting_lean: false,
            vertical_lcd_lean: false,
            backward_compatibility: false,
            iupx_called: false,
            iupy_called: false,
            grayscale_cleartype: false,
            loopcall_counter: 0,
            loopcall_counter_max: 0,
            neg_jump_counter: 0,
            neg_jump_counter_max: 0,
            num_glyphs: progs.num_glyphs,
            glyph_ins: None,
        }));
        self.gs = DEFAULT_GS;
        self.run_fpgm(progs)
    }

    /// `tt_size_ready_bytecode`.
    pub fn ready_bytecode(&mut self, progs: &Programs) -> Option<Error> {
        let err = match self.bytecode_ready {
            Ready::Pending => self.init_bytecode(progs),
            Ready::Done(e) => e,
        };
        if err.is_some() {
            return err;
        }
        match self.cvt_ready {
            Ready::Pending => {
                if let Some(exec) = &mut self.exec {
                    let tw = &mut exec.zones[0];
                    for i in 0..exec.twilight_points {
                        tw.org[i] = Vector::default();
                        tw.cur[i] = Vector::default();
                    }
                    exec.storage.iter_mut().for_each(|s| *s = 0);
                }
                self.gs = DEFAULT_GS;
                self.run_prep(progs)
            }
            Ready::Done(e) => e,
        }
    }
}

impl Exec {
    /// `TT_Load_Context`.
    pub fn load_context(&mut self, size: &TtSize, progs: &Programs) {
        self.num_fdefs = size.num_fdefs;
        self.num_idefs = size.num_idefs;
        self.point_size = size.point_size;
        self.ttm = size.ttm;
        self.x_ppem = size.x_ppem;
        self.y_ppem = size.y_ppem;
        self.x_scale = size.x_scale;
        self.y_scale = size.y_scale;
        self.max_func = size.max_func;
        self.max_ins = size.max_ins;
        self.ranges = size.ranges.clone();
        self.gs = size.gs;
        self.cvt_diverted = false;
        self.storage_diverted = false;
        self.zones[0].n_points = self.twilight_points;
        self.stack.resize(usize::from(progs.maxp.max_stack_elements) + 32, 0);
        self.glyph_ins = None;
        self.zones[1].n_points = 0;
        self.zones[1].contours.clear();
        self.zp = [1, 1, 1];
    }

    /// `TT_Goto_CodeRange`.
    fn goto_range(&mut self, range: usize, ip: i64) {
        self.code = self.ranges[range - 1].clone().unwrap_or_else(|| Rc::from(Vec::new()));
        self.ip = ip;
        self.cur_range = range;
    }

    /// `Ins_Goto_CodeRange`.
    fn ins_goto(&mut self, range: i32, ip: i64) -> bool {
        if !(1..=3).contains(&range) {
            self.error = Some(Error::BadArgument);
            return false;
        }
        let Some(code) = self.ranges[range as usize - 1].clone() else {
            self.error = Some(Error::InvalidCodeRange);
            return false;
        };
        if ip > code.len() as i64 {
            self.error = Some(Error::CodeOverflow);
            return false;
        }
        self.code = code;
        self.ip = ip;
        self.cur_range = range as usize;
        true
    }

    /// `TT_Run_Context` sobre a zona do glifo já montada em `zones[1]`.
    pub fn run_context(&mut self) -> Result<(), Error> {
        self.ranges[RANGE_GLYPH - 1] = self.glyph_ins.clone();
        self.goto_range(RANGE_GLYPH, 0);
        self.zp = [1, 1, 1];
        self.gs.gep0 = 1;
        self.gs.gep1 = 1;
        self.gs.gep2 = 1;
        self.gs.proj = X_AXIS;
        self.gs.free = X_AXIS;
        self.gs.dual = X_AXIS;
        self.gs.round_state = ROUND_TO_GRID;
        self.gs.loop_ = 1;
        self.top = 0;
        self.call_top = 0;
        self.run()
    }

    pub fn set_glyph_metrics(&mut self, x_scale: i64, y_scale: i64, is_composite: bool) {
        self.x_scale = x_scale;
        self.y_scale = y_scale;
        self.is_composite = is_composite;
    }

    fn throw(&mut self, e: Error) {
        self.error = Some(e);
    }

    fn pedantic_ref(&mut self) {
        if self.pedantic {
            self.throw(Error::InvalidReference);
        }
    }

    fn arg(&self, k: i64) -> i64 {
        self.stack[(self.args + k) as usize]
    }

    fn set_arg(&mut self, k: i64, v: i64) {
        let i = (self.args + k) as usize;
        self.stack[i] = v;
    }

    fn zone(&self, k: usize) -> &Zone {
        &self.zones[self.zp[k]]
    }

    fn npts(&self, k: usize) -> usize {
        self.zones[self.zp[k]].n_points
    }

    fn code_at(&self, i: i64) -> u8 {
        self.code[i as usize]
    }

    // Projeções e razões.

    fn project_with(&self, p: Proj, dx: i64, dy: i64) -> i64 {
        match p {
            Proj::X => dx,
            Proj::Y => dy,
            Proj::Proj => dot_fix14(dx, dy, self.gs.proj.x, self.gs.proj.y),
            Proj::Dual => dot_fix14(dx, dy, self.gs.dual.x, self.gs.dual.y),
        }
    }

    fn project(&self, a: Vector, b: Vector) -> i64 {
        self.project_with(self.project, a.x.wrapping_sub(b.x), a.y.wrapping_sub(b.y))
    }

    fn dualproj(&self, a: Vector, b: Vector) -> i64 {
        self.project_with(self.dualproj, a.x.wrapping_sub(b.x), a.y.wrapping_sub(b.y))
    }

    fn fast_project(&self, v: Vector) -> i64 {
        self.project_with(self.project, v.x, v.y)
    }

    fn fast_dualproj(&self, v: Vector) -> i64 {
        self.project_with(self.dualproj, v.x, v.y)
    }

    fn current_ratio(&mut self) -> i64 {
        if self.ttm.ratio == 0 {
            let pv = self.gs.proj;
            self.ttm.ratio = if pv.y == 0 {
                self.ttm.x_ratio
            } else if pv.x == 0 {
                self.ttm.y_ratio
            } else {
                let x = mul_fix14(self.ttm.x_ratio, pv.x);
                let y = mul_fix14(self.ttm.y_ratio, pv.y);
                hypot(x, y)
            };
        }
        self.ttm.ratio
    }

    fn cur_ppem(&mut self) -> i64 {
        if self.stretched {
            let r = self.current_ratio();
            mul_fix(i64::from(self.ttm.ppem), r)
        } else {
            i64::from(self.ttm.ppem)
        }
    }

    fn cvt_get(&self, i: usize) -> i64 {
        if self.cvt_diverted {
            self.glyf_cvt[i]
        } else {
            self.cvt[i]
        }
    }

    fn cvt_put(&mut self, i: usize, v: i64) {
        if self.cvt_diverted {
            self.glyf_cvt[i] = v;
        } else {
            self.cvt[i] = v;
        }
    }

    fn read_cvt(&mut self, i: usize) -> i64 {
        let v = self.cvt_get(i);
        if self.stretched {
            let r = self.current_ratio();
            mul_fix(v, r)
        } else {
            v
        }
    }

    /// `Modify_CVT_Check`.
    fn modify_cvt_check(&mut self) {
        if self.ini_range == RANGE_GLYPH && !self.cvt_diverted {
            self.glyf_cvt.clear();
            self.glyf_cvt.extend_from_slice(&self.cvt);
            self.cvt_diverted = true;
        }
    }

    fn write_cvt(&mut self, i: usize, v: i64) {
        self.modify_cvt_check();
        let v = if self.stretched {
            let r = self.current_ratio();
            div_fix(v, r)
        } else {
            v
        };
        self.cvt_put(i, v);
    }

    fn move_cvt(&mut self, i: usize, v: i64) {
        self.modify_cvt_check();
        let v = if self.stretched {
            let r = self.current_ratio();
            div_fix(v, r)
        } else {
            v
        };
        let old = self.cvt_get(i);
        self.cvt_put(i, old.wrapping_add(v));
    }

    // Movimentos.

    fn locked_y(&self) -> bool {
        self.backward_compatibility && self.iupx_called && self.iupy_called
    }

    /// `func_move`: `Direct_Move`, `Direct_Move_X` ou `Direct_Move_Y`.
    fn move_point(&mut self, z: usize, p: usize, d: i64) {
        let (fv, fdp, bc, locked) = (self.gs.free, self.f_dot_p, self.backward_compatibility, self.locked_y());
        let zone = &mut self.zones[z];
        match self.mover {
            Move::Any => {
                if fv.x != 0 {
                    if !bc {
                        zone.cur[p].x = zone.cur[p].x.wrapping_add(mul_div(d, i64::from(fv.x), fdp));
                    }
                    zone.tags[p] |= TOUCH_X;
                }
                if fv.y != 0 {
                    if !locked {
                        zone.cur[p].y = zone.cur[p].y.wrapping_add(mul_div(d, i64::from(fv.y), fdp));
                    }
                    zone.tags[p] |= TOUCH_Y;
                }
            }
            Move::X => {
                if !bc {
                    zone.cur[p].x = zone.cur[p].x.wrapping_add(d);
                }
                zone.tags[p] |= TOUCH_X;
            }
            Move::Y => {
                if !locked {
                    zone.cur[p].y = zone.cur[p].y.wrapping_add(d);
                }
                zone.tags[p] |= TOUCH_Y;
            }
        }
    }

    /// `func_move_orig`.
    fn move_orig(&mut self, z: usize, p: usize, d: i64) {
        let (fv, fdp) = (self.gs.free, self.f_dot_p);
        let zone = &mut self.zones[z];
        match self.mover {
            Move::Any => {
                if fv.x != 0 {
                    zone.org[p].x = zone.org[p].x.wrapping_add(mul_div(d, i64::from(fv.x), fdp));
                }
                if fv.y != 0 {
                    zone.org[p].y = zone.org[p].y.wrapping_add(mul_div(d, i64::from(fv.y), fdp));
                }
            }
            Move::X => zone.org[p].x = zone.org[p].x.wrapping_add(d),
            Move::Y => zone.org[p].y = zone.org[p].y.wrapping_add(d),
        }
    }

    // Arredondamentos.

    fn round_with(&self, mode: i32, d: i64, color: usize) -> i64 {
        let comp = self.ttm.compensations[color];
        let pos = d >= 0;
        match mode {
            ROUND_TO_GRID => {
                if pos {
                    pix_round_l(d.wrapping_add(comp)).max(0)
                } else {
                    pix_round_l(comp.wrapping_sub(d)).wrapping_neg().min(0)
                }
            }
            ROUND_TO_HALF_GRID => {
                if pos {
                    let v = pix_floor(d.wrapping_add(comp)).wrapping_add(32);
                    if v < 0 {
                        32
                    } else {
                        v
                    }
                } else {
                    let v = pix_floor(comp.wrapping_sub(d)).wrapping_add(32).wrapping_neg();
                    if v > 0 {
                        -32
                    } else {
                        v
                    }
                }
            }
            ROUND_DOWN_TO_GRID => {
                if pos {
                    pix_floor(d.wrapping_add(comp)).max(0)
                } else {
                    pix_floor(comp.wrapping_sub(d)).wrapping_neg().min(0)
                }
            }
            ROUND_UP_TO_GRID => {
                if pos {
                    pix_ceil_l(d.wrapping_add(comp)).max(0)
                } else {
                    pix_ceil_l(comp.wrapping_sub(d)).wrapping_neg().min(0)
                }
            }
            ROUND_TO_DOUBLE_GRID => {
                if pos {
                    (d.wrapping_add(comp).wrapping_add(16) & -32).max(0)
                } else {
                    (comp.wrapping_sub(d).wrapping_add(16) & -32).wrapping_neg().min(0)
                }
            }
            ROUND_SUPER => {
                let k = self.threshold - self.phase + comp;
                if pos {
                    let v = (d.wrapping_add(k) & -self.period).wrapping_add(self.phase);
                    if v < 0 {
                        self.phase
                    } else {
                        v
                    }
                } else {
                    let v = (k.wrapping_sub(d) & -self.period).wrapping_neg().wrapping_sub(self.phase);
                    if v > 0 {
                        -self.phase
                    } else {
                        v
                    }
                }
            }
            ROUND_SUPER_45 => {
                let k = self.threshold - self.phase + comp;
                if pos {
                    let v = (d.wrapping_add(k) / self.period * self.period).wrapping_add(self.phase);
                    if v < 0 {
                        self.phase
                    } else {
                        v
                    }
                } else {
                    let v = (k.wrapping_sub(d) / self.period * self.period).wrapping_neg().wrapping_sub(self.phase);
                    if v > 0 {
                        -self.phase
                    } else {
                        v
                    }
                }
            }
            _ => {
                // `Round_None`.
                if pos {
                    d.wrapping_add(comp).max(0)
                } else {
                    d.wrapping_sub(comp).min(0)
                }
            }
        }
    }

    fn round(&self, d: i64, color: usize) -> i64 {
        self.round_with(self.round, d, color)
    }

    /// `Compute_Round`: estados fora da tabela deixam a função como está.
    fn compute_round(&mut self, mode: i32) {
        if (0..=7).contains(&mode) {
            self.round = mode;
        }
    }

    fn set_super_round(&mut self, grid_period: i64, selector: i64) {
        self.period = match selector & 0xC0 {
            0 => grid_period / 2,
            0x80 => grid_period * 2,
            _ => grid_period,
        };
        self.phase = match selector & 0x30 {
            0 => 0,
            0x10 => self.period / 4,
            0x20 => self.period / 2,
            _ => self.period * 3 / 4,
        };
        self.threshold = if selector & 0x0F == 0 {
            self.period - 1
        } else {
            ((selector & 0x0F) - 4) * self.period / 8
        };
        self.period >>= 8;
        self.phase >>= 8;
        self.threshold >>= 8;
    }

    /// `Compute_Funcs`.
    fn compute_funcs(&mut self) {
        let (fv, pv, dv) = (self.gs.free, self.gs.proj, self.gs.dual);
        self.f_dot_p = if fv.x == 0x4000 {
            i64::from(pv.x)
        } else if fv.y == 0x4000 {
            i64::from(pv.y)
        } else {
            (i64::from(pv.x) * i64::from(fv.x) + i64::from(pv.y) * i64::from(fv.y)) >> 14
        };
        self.project = if pv.x == 0x4000 {
            Proj::X
        } else if pv.y == 0x4000 {
            Proj::Y
        } else {
            Proj::Proj
        };
        self.dualproj = if dv.x == 0x4000 {
            Proj::X
        } else if dv.y == 0x4000 {
            Proj::Y
        } else {
            Proj::Dual
        };
        self.mover = Move::Any;
        if self.f_dot_p == 0x4000 {
            if fv.x == 0x4000 {
                self.mover = Move::X;
            } else if fv.y == 0x4000 {
                self.mover = Move::Y;
            }
        }
        if self.f_dot_p.abs() < 0x400 {
            self.f_dot_p = 0x4000;
        }
        self.ttm.ratio = 0;
    }

    // Controle de fluxo.

    /// `SkipCode`: `true` em caso de falha.
    fn skip_code(&mut self) -> bool {
        self.ip += self.length;
        if self.ip < self.code.len() as i64 {
            self.opcode = self.code_at(self.ip);
            self.length = opcode_length(self.opcode);
            let mut ok = true;
            if self.length < 0 {
                if self.ip + 1 >= self.code.len() as i64 {
                    ok = false;
                } else {
                    self.length = 2 - self.length * i64::from(self.code_at(self.ip + 1));
                }
            }
            if ok && self.ip + self.length <= self.code.len() as i64 {
                return false;
            }
        }
        self.throw(Error::CodeOverflow);
        true
    }

    fn ins_if(&mut self) {
        if self.arg(0) != 0 {
            return;
        }
        let mut n_ifs = 1;
        loop {
            if self.skip_code() {
                return;
            }
            let out = match self.opcode {
                0x58 => {
                    n_ifs += 1;
                    false
                }
                0x1B => n_ifs == 1,
                0x59 => {
                    n_ifs -= 1;
                    n_ifs == 0
                }
                _ => false,
            };
            if out {
                return;
            }
        }
    }

    fn ins_else(&mut self) {
        let mut n_ifs = 1;
        loop {
            if self.skip_code() {
                return;
            }
            match self.opcode {
                0x58 => n_ifs += 1,
                0x59 => n_ifs -= 1,
                _ => {}
            }
            if n_ifs == 0 {
                return;
            }
        }
    }

    fn def(&self, c: &CallRec) -> Def {
        if c.idef {
            self.idefs[c.def]
        } else {
            self.fdefs[c.def]
        }
    }

    fn ins_jmpr(&mut self, offset: i64) {
        if offset == 0 && self.args == 0 {
            self.throw(Error::BadArgument);
            return;
        }
        self.ip = self.ip.wrapping_add(offset);
        if self.ip < 0 || (self.call_top > 0 && self.ip > self.def(&self.call[self.call_top - 1]).end) {
            self.throw(Error::BadArgument);
            return;
        }
        self.step_ins = false;
        if offset < 0 {
            self.neg_jump_counter += 1;
            if self.neg_jump_counter > self.neg_jump_counter_max {
                self.throw(Error::ExecutionTooLong);
            }
        }
    }

    fn ins_fdef(&mut self) {
        if self.ini_range == RANGE_GLYPH {
            self.throw(Error::DefInGlyfBytecode);
            return;
        }
        let n = self.arg(0) as u64;
        let rec = (0..self.num_fdefs).find(|&i| u64::from(self.fdefs[i].opc) == n).unwrap_or(self.num_fdefs);
        if rec == self.num_fdefs {
            if self.num_fdefs >= self.fdefs.len() {
                self.throw(Error::TooManyFunctionDefs);
                return;
            }
            self.num_fdefs += 1;
        }
        if n > 0xFFFF {
            self.throw(Error::TooManyFunctionDefs);
            return;
        }
        self.fdefs[rec] = Def { range: self.cur_range as i32, opc: n as u32, start: self.ip + 1, active: true, end: self.fdefs[rec].end };
        if n > u64::from(self.max_func) {
            self.max_func = n as u32;
        }
        while !self.skip_code() {
            match self.opcode {
                0x89 | 0x2C => {
                    self.throw(Error::NestedDefs);
                    return;
                }
                0x2D => {
                    self.fdefs[rec].end = self.ip;
                    return;
                }
                _ => {}
            }
        }
    }

    fn ins_idef(&mut self) {
        if self.ini_range == RANGE_GLYPH {
            self.throw(Error::DefInGlyfBytecode);
            return;
        }
        let a = self.arg(0);
        let def = (0..self.num_idefs).find(|&i| u64::from(self.idefs[i].opc) == a as u64).unwrap_or(self.num_idefs);
        if def == self.num_idefs {
            if self.num_idefs >= self.idefs.len() {
                self.throw(Error::TooManyInstructionDefs);
                return;
            }
            self.num_idefs += 1;
        }
        if !(0..=0xFF).contains(&a) {
            self.throw(Error::TooManyInstructionDefs);
            return;
        }
        self.idefs[def] = Def { opc: a as u32, start: self.ip + 1, range: self.cur_range as i32, active: true, end: self.idefs[def].end };
        if a as u64 > u64::from(self.max_ins) {
            self.max_ins = a as u32;
        }
        while !self.skip_code() {
            match self.opcode {
                0x89 | 0x2C => {
                    self.throw(Error::NestedDefs);
                    return;
                }
                0x2D => {
                    self.idefs[def].end = self.ip;
                    return;
                }
                _ => {}
            }
        }
    }

    fn ins_endf(&mut self) {
        if self.call_top == 0 {
            self.throw(Error::EndfInExecStream);
            return;
        }
        self.call_top -= 1;
        let rec = &mut self.call[self.call_top];
        rec.cur_count -= 1;
        let rec = *rec;
        self.step_ins = false;
        if rec.cur_count > 0 {
            self.call_top += 1;
            self.ip = self.def(&rec).start;
        } else {
            self.ins_goto(rec.caller_range, rec.caller_ip);
        }
    }

    /// Acha a função `f` como o `Ins_CALL`: pelo atalho do índice ou na busca linear.
    fn find_fdef(&self, f: u64) -> Option<usize> {
        if f >= u64::from(self.max_func) + 1 {
            return None;
        }
        let direct = f as usize;
        if u64::from(self.max_func) + 1 == self.num_fdefs as u64
            && direct < self.fdefs.len()
            && u64::from(self.fdefs[direct].opc) == f
        {
            return Some(direct);
        }
        (0..self.num_fdefs).find(|&i| u64::from(self.fdefs[i].opc) == f)
    }

    fn push_call(&mut self, idef: bool, def: usize, count: i64) {
        let d = if idef { self.idefs[def] } else { self.fdefs[def] };
        self.call[self.call_top] =
            CallRec { caller_range: self.cur_range as i32, caller_ip: self.ip + 1, cur_count: count, idef, def };
        self.call_top += 1;
        self.ins_goto(d.range, d.start);
        self.step_ins = false;
    }

    fn ins_call(&mut self) {
        let f = self.arg(0) as u64;
        let Some(def) = self.find_fdef(f).filter(|&d| self.fdefs[d].active) else {
            self.throw(Error::InvalidReference);
            return;
        };
        if self.call_top >= CALL_SIZE {
            self.throw(Error::StackOverflow);
            return;
        }
        self.push_call(false, def, 1);
    }

    fn ins_loopcall(&mut self) {
        let f = self.arg(1) as u64;
        let Some(def) = self.find_fdef(f).filter(|&d| self.fdefs[d].active) else {
            self.throw(Error::InvalidReference);
            return;
        };
        if self.call_top >= CALL_SIZE {
            self.throw(Error::StackOverflow);
            return;
        }
        let count = self.arg(0);
        if count > 0 {
            self.push_call(false, def, i64::from(count as i32));
            self.loopcall_counter = self.loopcall_counter.wrapping_add(count as u64);
            if self.loopcall_counter > self.loopcall_counter_max {
                self.throw(Error::ExecutionTooLong);
            }
        }
    }

    fn ins_unknown(&mut self) {
        let op = self.opcode;
        if let Some(i) = (0..self.num_idefs).find(|&i| self.idefs[i].opc as u8 == op && self.idefs[i].active) {
            if self.call_top >= CALL_SIZE {
                self.throw(Error::StackOverflow);
                return;
            }
            self.push_call(true, i, 1);
            return;
        }
        self.throw(Error::InvalidOpcode);
    }

    // Vetores.

    /// `Ins_SxVTL`: `false` em caso de falha.
    fn sxvtl(&mut self, idx1: u16, idx2: u16, which: u8) -> bool {
        let mut op = self.opcode;
        if usize::from(idx1) >= self.npts(2) || usize::from(idx2) >= self.npts(1) {
            self.pedantic_ref();
            return false;
        }
        let p1 = self.zone(1).cur[usize::from(idx2)];
        let p2 = self.zone(2).cur[usize::from(idx1)];
        let mut a = p1.x.wrapping_sub(p2.x);
        let mut b = p1.y.wrapping_sub(p2.y);
        if a == 0 && b == 0 {
            a = 0x4000;
            op = 0;
        }
        if op & 1 != 0 {
            let c = b;
            b = a;
            a = c.wrapping_neg();
        }
        let v = if which == 0 { &mut self.gs.proj } else { &mut self.gs.free };
        normalize(a, b, v);
        true
    }

    fn ins_sdpvtl(&mut self) {
        let mut op = self.opcode;
        let p1 = self.arg(1) as u16 as usize;
        let p2 = self.arg(0) as u16 as usize;
        if p2 >= self.npts(1) || p1 >= self.npts(2) {
            self.pedantic_ref();
            return;
        }
        let ab = |v1: Vector, v2: Vector, op: &mut u8| {
            let mut a = v1.x.wrapping_sub(v2.x);
            let mut b = v1.y.wrapping_sub(v2.y);
            if a == 0 && b == 0 {
                a = 0x4000;
                *op = 0;
            }
            if *op & 1 != 0 {
                let c = b;
                b = a;
                a = c.wrapping_neg();
            }
            (a, b)
        };
        let (a, b) = ab(self.zone(1).org[p2], self.zone(2).org[p1], &mut op);
        normalize(a, b, &mut self.gs.dual);
        let (a, b) = ab(self.zone(1).cur[p2], self.zone(2).cur[p1], &mut op);
        normalize(a, b, &mut self.gs.proj);
        self.compute_funcs();
    }

    fn ins_szp(&mut self, which: usize) {
        let z = match self.arg(0) as i32 {
            0 => 0,
            1 => 1,
            _ => {
                self.pedantic_ref();
                return;
            }
        };
        let g = z as u16;
        match which {
            0 => {
                self.zp[0] = z;
                self.gs.gep0 = g;
            }
            1 => {
                self.zp[1] = z;
                self.gs.gep1 = g;
            }
            2 => {
                self.zp[2] = z;
                self.gs.gep2 = g;
            }
            _ => {
                self.zp = [z, z, z];
                self.gs.gep0 = g;
                self.gs.gep1 = g;
                self.gs.gep2 = g;
            }
        }
    }

    fn ins_instctrl(&mut self) {
        let k = self.arg(1) as u64;
        let l = self.arg(0) as u64;
        if !(1..=3).contains(&k) {
            self.pedantic_ref();
            return;
        }
        let kf = 1u64 << (k - 1);
        if l != 0 && l != kf {
            self.pedantic_ref();
            return;
        }
        if self.ini_range == RANGE_CVT {
            self.gs.instruct_control &= !(kf as u8);
            self.gs.instruct_control |= l as u8;
        } else if self.ini_range == RANGE_GLYPH && k == 3 {
            self.backward_compatibility = l != 4;
        } else {
            self.pedantic_ref();
        }
    }

    fn ins_scanctrl(&mut self) {
        let v = self.arg(0);
        let a = (v & 0xFF) as i32;
        if a == 0xFF {
            self.gs.scan_control = true;
            return;
        } else if a == 0 {
            self.gs.scan_control = false;
            return;
        }
        let ppem = i32::from(self.ttm.ppem);
        if v & 0x100 != 0 && ppem <= a {
            self.gs.scan_control = true;
        }
        if v & 0x800 != 0 && ppem > a {
            self.gs.scan_control = false;
        }
    }

    // Pontos.

    fn ins_flippt(&mut self) {
        if !self.locked_y() {
            if self.top < self.gs.loop_ {
                if self.pedantic {
                    self.throw(Error::TooFewArguments);
                }
            } else {
                while self.gs.loop_ > 0 {
                    self.args -= 1;
                    let p = self.stack[self.args as usize] as u16 as usize;
                    if p >= self.zones[1].n_points {
                        if self.pedantic {
                            self.throw(Error::InvalidReference);
                            return;
                        }
                    } else {
                        self.zones[1].tags[p] ^= TAG_ON;
                    }
                    self.gs.loop_ -= 1;
                }
            }
        }
        self.gs.loop_ = 1;
        self.new_top = self.args;
    }

    fn ins_fliprg(&mut self, on: bool) {
        if self.locked_y() {
            return;
        }
        let k = self.arg(1) as u16;
        let l = self.arg(0) as u16;
        let n = self.zones[1].n_points;
        if usize::from(k) >= n || usize::from(l) >= n {
            self.pedantic_ref();
            return;
        }
        for i in l..=k {
            let t = &mut self.zones[1].tags[usize::from(i)];
            if on {
                *t |= TAG_ON;
            } else {
                *t &= !TAG_ON;
            }
        }
    }

    /// `Compute_Point_Displacement`: (dx, dy, zona de referência, ponto de referência).
    fn point_displacement(&mut self) -> Option<(i64, i64, usize, usize)> {
        let (z, p) = if self.opcode & 1 != 0 { (self.zp[0], self.gs.rp1) } else { (self.zp[1], self.gs.rp2) };
        let p = usize::from(p);
        if p >= self.zones[z].n_points {
            self.pedantic_ref();
            return None;
        }
        let d = self.project(self.zones[z].cur[p], self.zones[z].org[p]);
        let x = mul_div(d, i64::from(self.gs.free.x), self.f_dot_p);
        let y = mul_div(d, i64::from(self.gs.free.y), self.f_dot_p);
        Some((x, y, z, p))
    }

    /// `Move_Zp2_Point`.
    fn move_zp2(&mut self, p: usize, dx: i64, dy: i64, touch: bool) {
        let (fv, bc, locked) = (self.gs.free, self.backward_compatibility, self.locked_y());
        let zone = &mut self.zones[self.zp[2]];
        if fv.x != 0 {
            if !bc {
                zone.cur[p].x = zone.cur[p].x.wrapping_add(dx);
            }
            if touch {
                zone.tags[p] |= TOUCH_X;
            }
        }
        if fv.y != 0 {
            if !locked {
                zone.cur[p].y = zone.cur[p].y.wrapping_add(dy);
            }
            if touch {
                zone.tags[p] |= TOUCH_Y;
            }
        }
    }

    /// O laço do `SHP`, `ALIGNRP`, `IP` e afins: tira um ponto da pilha.
    fn pop_point(&mut self) -> usize {
        self.args -= 1;
        self.stack[self.args as usize] as u16 as usize
    }

    fn end_loop(&mut self) {
        self.gs.loop_ = 1;
        self.new_top = self.args;
    }

    fn ins_shp(&mut self) {
        if self.top < self.gs.loop_ {
            self.pedantic_ref();
            self.end_loop();
            return;
        }
        let Some((dx, dy, _, _)) = self.point_displacement() else { return };
        while self.gs.loop_ > 0 {
            let p = self.pop_point();
            if p >= self.npts(2) {
                if self.pedantic {
                    self.throw(Error::InvalidReference);
                    return;
                }
            } else {
                self.move_zp2(p, dx, dy, true);
            }
            self.gs.loop_ -= 1;
        }
        self.end_loop();
    }

    fn ins_shc(&mut self) {
        let contour = self.arg(0) as u16 as usize;
        let bounds = if self.gs.gep2 == 0 { 1 } else { self.zone(2).contours.len() };
        if contour >= bounds {
            self.pedantic_ref();
            return;
        }
        let Some((dx, dy, zr, refp)) = self.point_displacement() else { return };
        let z2 = self.zone(2);
        let rel = |e: usize| usize::from(e.wrapping_add(1).wrapping_sub(z2.first_point) as u16);
        let start = if contour == 0 { 0 } else { rel(z2.contours[contour - 1]) };
        let limit = if self.gs.gep2 == 0 { z2.n_points } else { rel(z2.contours[contour]) };
        let limit = limit.min(z2.cur.len());
        for i in start..limit {
            if zr != self.zp[2] || refp != i {
                self.move_zp2(i, dx, dy, true);
            }
        }
    }

    fn ins_shz(&mut self) {
        if self.arg(0) as u32 >= 2 {
            self.pedantic_ref();
            return;
        }
        let Some((dx, dy, zr, refp)) = self.point_displacement() else { return };
        let z2 = self.zone(2);
        let limit = if self.gs.gep2 == 0 {
            z2.n_points
        } else if self.gs.gep2 == 1 && !z2.contours.is_empty() {
            usize::from((z2.contours[z2.contours.len() - 1] + 1) as u16)
        } else {
            0
        };
        let limit = limit.min(z2.cur.len());
        for i in 0..limit {
            if zr != self.zp[2] || refp != i {
                self.move_zp2(i, dx, dy, false);
            }
        }
    }

    fn ins_shpix(&mut self) {
        let in_twilight = self.gs.gep0 == 0 || self.gs.gep1 == 0 || self.gs.gep2 == 0;
        if self.top < self.gs.loop_ + 1 {
            self.pedantic_ref();
            self.end_loop();
            return;
        }
        let dx = mul_fix14(self.arg(0), self.gs.free.x);
        let dy = mul_fix14(self.arg(0), self.gs.free.y);
        while self.gs.loop_ > 0 {
            let p = self.pop_point();
            if p >= self.npts(2) {
                if self.pedantic {
                    self.throw(Error::InvalidReference);
                    return;
                }
            } else if self.backward_compatibility {
                let touched_y = self.zone(2).tags[p] & TOUCH_Y != 0;
                if in_twilight
                    || (!(self.iupx_called && self.iupy_called)
                        && ((self.is_composite && self.gs.free.y != 0) || touched_y))
                {
                    self.move_zp2(p, 0, dy, true);
                }
            } else {
                self.move_zp2(p, dx, dy, true);
            }
            self.gs.loop_ -= 1;
        }
        self.end_loop();
    }

    fn ins_msirp(&mut self) {
        let p = self.arg(0) as u16 as usize;
        let rp0 = usize::from(self.gs.rp0);
        if p >= self.npts(1) || rp0 >= self.npts(0) {
            self.pedantic_ref();
            return;
        }
        let (z0, z1) = (self.zp[0], self.zp[1]);
        if self.gs.gep1 == 0 {
            self.zones[z1].org[p] = self.zones[z0].org[rp0];
            self.move_orig(z1, p, self.arg(1));
            self.zones[z1].cur[p] = self.zones[z1].org[p];
        }
        let distance = self.project(self.zones[z1].cur[p], self.zones[z0].cur[rp0]);
        self.move_point(z1, p, self.arg(1).wrapping_sub(distance));
        self.gs.rp1 = self.gs.rp0;
        self.gs.rp2 = p as u16;
        if self.opcode & 1 != 0 {
            self.gs.rp0 = p as u16;
        }
    }

    fn ins_mdap(&mut self) {
        let p = self.arg(0) as u16 as usize;
        if p >= self.npts(0) {
            self.pedantic_ref();
            return;
        }
        let z0 = self.zp[0];
        let distance = if self.opcode & 1 != 0 {
            let cur = self.fast_project(self.zones[z0].cur[p]);
            self.round(cur, 3).wrapping_sub(cur)
        } else {
            0
        };
        self.move_point(z0, p, distance);
        self.gs.rp0 = p as u16;
        self.gs.rp1 = p as u16;
    }

    fn ins_miap(&mut self) {
        let cvt_entry = self.arg(1) as u64;
        let p = self.arg(0) as u16;
        let pu = usize::from(p);
        if pu < self.npts(0) && cvt_entry < self.cvt.len() as u64 {
            let z0 = self.zp[0];
            let mut distance = self.read_cvt(cvt_entry as usize);
            if self.gs.gep0 == 0 {
                let v = Vector { x: mul_fix14(distance, self.gs.free.x), y: mul_fix14(distance, self.gs.free.y) };
                self.zones[z0].org[pu] = v;
                self.zones[z0].cur[pu] = v;
            }
            let org_dist = self.fast_project(self.zones[z0].cur[pu]);
            if self.opcode & 1 != 0 {
                let delta = distance.wrapping_sub(org_dist).wrapping_abs();
                if delta > self.gs.control_value_cutin {
                    distance = org_dist;
                }
                distance = self.round(distance, 3);
            }
            self.move_point(z0, pu, distance.wrapping_sub(org_dist));
        } else {
            self.pedantic_ref();
        }
        self.gs.rp0 = p;
        self.gs.rp1 = p;
    }

    /// A distância original entre `zp1[p]` e `zp0[rp0]`, como `MDRP` e `MD` a calculam.
    fn orus_dist(&self, z1: usize, p: usize, z0: usize, q: usize) -> i64 {
        if self.gs.gep0 == 0 || self.gs.gep1 == 0 {
            self.dualproj(self.zones[z1].org[p], self.zones[z0].org[q])
        } else {
            let (v1, v2) = (self.zones[z1].orus[p], self.zones[z0].orus[q]);
            if self.x_scale == self.y_scale {
                mul_fix(self.dualproj(v1, v2), self.x_scale)
            } else {
                let v = Vector {
                    x: mul_fix(v1.x.wrapping_sub(v2.x), self.x_scale),
                    y: mul_fix(v1.y.wrapping_sub(v2.y), self.y_scale),
                };
                self.fast_dualproj(v)
            }
        }
    }

    fn min_dist(&self, org_dist: i64, distance: i64) -> i64 {
        let md = self.gs.minimum_distance;
        if org_dist >= 0 {
            if distance < md {
                return md;
            }
        } else if distance > md.wrapping_neg() {
            return md.wrapping_neg();
        }
        distance
    }

    fn ins_mdrp(&mut self) {
        let p = self.arg(0) as u16 as usize;
        let rp0 = usize::from(self.gs.rp0);
        if p >= self.npts(1) || rp0 >= self.npts(0) {
            self.pedantic_ref();
        } else {
            let (z0, z1) = (self.zp[0], self.zp[1]);
            let mut org_dist = self.orus_dist(z1, p, z0, rp0);
            let (swv, swc) = (self.gs.single_width_value, self.gs.single_width_cutin);
            if swc > 0 && org_dist < swv + swc && org_dist > swv - swc {
                org_dist = if org_dist >= 0 { swv } else { -swv };
            }
            let color = usize::from(self.opcode & 3);
            let mut distance = if self.opcode & 4 != 0 {
                self.round(org_dist, color)
            } else {
                self.round_with(ROUND_OFF, org_dist, color)
            };
            if self.opcode & 8 != 0 {
                distance = self.min_dist(org_dist, distance);
            }
            let cur = self.project(self.zones[z1].cur[p], self.zones[z0].cur[rp0]);
            self.move_point(z1, p, distance.wrapping_sub(cur));
        }
        self.gs.rp1 = self.gs.rp0;
        self.gs.rp2 = p as u16;
        if self.opcode & 16 != 0 {
            self.gs.rp0 = p as u16;
        }
    }

    fn ins_mirp(&mut self) {
        let p = self.arg(0) as u16 as usize;
        let cvt_entry = self.arg(1).wrapping_add(1) as u64;
        let rp0 = usize::from(self.gs.rp0);
        if p >= self.npts(1) || cvt_entry >= self.cvt.len() as u64 + 1 || rp0 >= self.npts(0) {
            self.pedantic_ref();
        } else {
            let (z0, z1) = (self.zp[0], self.zp[1]);
            let mut cvt_dist = if cvt_entry == 0 { 0 } else { self.read_cvt(cvt_entry as usize - 1) };
            let delta = cvt_dist.wrapping_sub(self.gs.single_width_value).wrapping_abs();
            if delta < self.gs.single_width_cutin {
                cvt_dist = if cvt_dist >= 0 { self.gs.single_width_value } else { -self.gs.single_width_value };
            }
            if self.gs.gep1 == 0 {
                let base = self.zones[z0].org[rp0];
                let v = Vector {
                    x: base.x.wrapping_add(mul_fix14(cvt_dist, self.gs.free.x)),
                    y: base.y.wrapping_add(mul_fix14(cvt_dist, self.gs.free.y)),
                };
                self.zones[z1].org[p] = v;
                self.zones[z1].cur[p] = v;
            }
            let org_dist = self.dualproj(self.zones[z1].org[p], self.zones[z0].org[rp0]);
            let cur_dist = self.project(self.zones[z1].cur[p], self.zones[z0].cur[rp0]);
            if self.gs.auto_flip && (org_dist ^ cvt_dist) < 0 {
                cvt_dist = cvt_dist.wrapping_neg();
            }
            let color = usize::from(self.opcode & 3);
            let mut distance = if self.opcode & 4 != 0 {
                if self.gs.gep0 == self.gs.gep1 && cvt_dist.wrapping_sub(org_dist).wrapping_abs() > self.gs.control_value_cutin {
                    cvt_dist = org_dist;
                }
                self.round(cvt_dist, color)
            } else {
                self.round_with(ROUND_OFF, cvt_dist, color)
            };
            if self.opcode & 8 != 0 {
                distance = self.min_dist(org_dist, distance);
            }
            self.move_point(z1, p, distance.wrapping_sub(cur_dist));
        }
        self.gs.rp1 = self.gs.rp0;
        if self.opcode & 16 != 0 {
            self.gs.rp0 = p as u16;
        }
        self.gs.rp2 = p as u16;
    }

    fn ins_alignrp(&mut self) {
        let rp0 = usize::from(self.gs.rp0);
        if self.top < self.gs.loop_ || rp0 >= self.npts(0) {
            self.pedantic_ref();
            self.end_loop();
            return;
        }
        let (z0, z1) = (self.zp[0], self.zp[1]);
        while self.gs.loop_ > 0 {
            let p = self.pop_point();
            if p >= self.npts(1) {
                if self.pedantic {
                    self.throw(Error::InvalidReference);
                    return;
                }
            } else {
                let d = self.project(self.zones[z1].cur[p], self.zones[z0].cur[rp0]);
                self.move_point(z1, p, d.wrapping_neg());
            }
            self.gs.loop_ -= 1;
        }
        self.end_loop();
    }

    fn ins_isect(&mut self) {
        let p = self.arg(0) as u16 as usize;
        let a0 = self.arg(1) as u16 as usize;
        let a1 = self.arg(2) as u16 as usize;
        let b0 = self.arg(3) as u16 as usize;
        let b1 = self.arg(4) as u16 as usize;
        if b0 >= self.npts(0) || b1 >= self.npts(0) || a0 >= self.npts(1) || a1 >= self.npts(1) || p >= self.npts(2) {
            self.pedantic_ref();
            return;
        }
        let (zb, za, zp) = (self.zp[0], self.zp[1], self.zp[2]);
        let (pb0, pb1) = (self.zones[zb].cur[b0], self.zones[zb].cur[b1]);
        let (pa0, pa1) = (self.zones[za].cur[a0], self.zones[za].cur[a1]);
        let dbx = pb1.x.wrapping_sub(pb0.x);
        let dby = pb1.y.wrapping_sub(pb0.y);
        let dax = pa1.x.wrapping_sub(pa0.x);
        let day = pa1.y.wrapping_sub(pa0.y);
        let dx = pb0.x.wrapping_sub(pa0.x);
        let dy = pb0.y.wrapping_sub(pa0.y);
        let disc = mul_div(dax, dby.wrapping_neg(), 0x40).wrapping_add(mul_div(day, dbx, 0x40));
        let dot = mul_div(dax, dbx, 0x40).wrapping_add(mul_div(day, dby, 0x40));
        let r = if 19i64.wrapping_mul(disc.wrapping_abs()) > dot.wrapping_abs() {
            let val = mul_div(dx, dby.wrapping_neg(), 0x40).wrapping_add(mul_div(dy, dbx, 0x40));
            Vector {
                x: pa0.x.wrapping_add(mul_div(val, dax, disc)),
                y: pa0.y.wrapping_add(mul_div(val, day, disc)),
            }
        } else {
            Vector {
                x: pa0.x.wrapping_add(pa1.x).wrapping_add(pb0.x.wrapping_add(pb1.x)) / 4,
                y: pa0.y.wrapping_add(pa1.y).wrapping_add(pb0.y.wrapping_add(pb1.y)) / 4,
            }
        };
        self.zones[zp].cur[p] = r;
        self.zones[zp].tags[p] |= TOUCH_BOTH;
    }

    fn ins_alignpts(&mut self) {
        let p1 = self.arg(0) as u16 as usize;
        let p2 = self.arg(1) as u16 as usize;
        if p1 >= self.npts(1) || p2 >= self.npts(0) {
            self.pedantic_ref();
            return;
        }
        let (z0, z1) = (self.zp[0], self.zp[1]);
        let d = self.project(self.zones[z0].cur[p2], self.zones[z1].cur[p1]) / 2;
        self.move_point(z1, p1, d);
        self.move_point(z0, p2, d.wrapping_neg());
    }

    fn ins_ip(&mut self) {
        if self.top < self.gs.loop_ {
            self.pedantic_ref();
            self.end_loop();
            return;
        }
        let twilight = self.gs.gep0 == 0 || self.gs.gep1 == 0 || self.gs.gep2 == 0;
        let rp1 = usize::from(self.gs.rp1);
        let rp2 = usize::from(self.gs.rp2);
        if rp1 >= self.npts(0) {
            self.pedantic_ref();
            self.end_loop();
            return;
        }
        let (z0, z1, z2) = (self.zp[0], self.zp[1], self.zp[2]);
        let orus_base = if twilight { self.zones[z0].org[rp1] } else { self.zones[z0].orus[rp1] };
        let (xs, ys) = (self.x_scale, self.y_scale);
        let scaled = |a: Vector, b: Vector| Vector {
            x: mul_fix(a.x.wrapping_sub(b.x), xs),
            y: mul_fix(a.y.wrapping_sub(b.y), ys),
        };
        let (old_range, cur_range) = if rp2 >= self.npts(1) {
            (0, 0)
        } else {
            let old = if twilight {
                self.dualproj(self.zones[z1].org[rp2], orus_base)
            } else if xs == ys {
                self.dualproj(self.zones[z1].orus[rp2], orus_base)
            } else {
                self.fast_dualproj(scaled(self.zones[z1].orus[rp2], orus_base))
            };
            (old, self.project(self.zones[z1].cur[rp2], self.zones[z0].cur[rp1]))
        };
        while self.gs.loop_ > 0 {
            let p = self.pop_point_u32();
            if p >= self.npts(2) {
                if self.pedantic {
                    self.throw(Error::InvalidReference);
                    return;
                }
                self.gs.loop_ -= 1;
                continue;
            }
            let org_dist = if twilight {
                self.dualproj(self.zones[z2].org[p], orus_base)
            } else if xs == ys {
                self.dualproj(self.zones[z2].orus[p], orus_base)
            } else {
                self.fast_dualproj(scaled(self.zones[z2].orus[p], orus_base))
            };
            // `cur_base` é um ponteiro no C: lido de novo a cada ponto.
            let cur_dist = self.project(self.zones[z2].cur[p], self.zones[z0].cur[rp1]);
            let new_dist = if org_dist != 0 {
                if old_range != 0 {
                    mul_div(org_dist, cur_range, old_range)
                } else {
                    org_dist
                }
            } else {
                0
            };
            self.move_point(z2, p, new_dist.wrapping_sub(cur_dist));
            self.gs.loop_ -= 1;
        }
        self.end_loop();
    }

    /// O `IP` lê o ponto como `FT_UInt`, sem o corte para 16 bits dos outros.
    fn pop_point_u32(&mut self) -> usize {
        self.args -= 1;
        self.stack[self.args as usize] as u32 as usize
    }

    fn ins_utp(&mut self) {
        let p = self.arg(0) as u16 as usize;
        if p >= self.npts(0) {
            self.pedantic_ref();
            return;
        }
        let mut mask = 0xFFu8;
        if self.gs.free.x != 0 {
            mask &= !TOUCH_X;
        }
        if self.gs.free.y != 0 {
            mask &= !TOUCH_Y;
        }
        let z0 = self.zp[0];
        self.zones[z0].tags[p] &= mask;
    }

    fn ins_iup(&mut self) {
        if self.backward_compatibility {
            if self.iupx_called && self.iupy_called {
                return;
            }
            if self.opcode & 1 != 0 {
                self.iupx_called = true;
            } else {
                self.iupy_called = true;
            }
        }
        let pts = &mut self.zones[1];
        if pts.contours.is_empty() {
            return;
        }
        let x_axis = self.opcode & 1 != 0;
        let mask = if x_axis { TOUCH_X } else { TOUCH_Y };
        let get = |v: &Vector| if x_axis { v.x } else { v.y };
        let n = pts.n_points;
        let mut point = 0usize;
        for c in 0..pts.contours.len() {
            let mut end_point = pts.contours[c].wrapping_sub(pts.first_point) as u32 as usize;
            let first_point = point;
            if end_point >= n {
                end_point = n - 1;
            }
            while point <= end_point && pts.tags[point] & mask == 0 {
                point += 1;
            }
            if point <= end_point {
                let first_touched = point;
                let mut cur_touched = point;
                point += 1;
                while point <= end_point {
                    if pts.tags[point] & mask != 0 {
                        iup_interpolate(pts, x_axis, cur_touched + 1, point - 1, cur_touched, point);
                        cur_touched = point;
                    }
                    point += 1;
                }
                if cur_touched == first_touched {
                    // `iup_worker_shift_`.
                    let dx = get(&pts.cur[cur_touched]).wrapping_sub(get(&pts.org[cur_touched]));
                    if dx != 0 {
                        for i in (first_point..=end_point).filter(|&i| i != cur_touched) {
                            let v = get(&pts.cur[i]).wrapping_add(dx);
                            set_axis(&mut pts.cur[i], x_axis, v);
                        }
                    }
                } else {
                    iup_interpolate(pts, x_axis, cur_touched + 1, end_point, cur_touched, first_touched);
                    if first_touched > 0 {
                        iup_interpolate(pts, x_axis, first_point, first_touched - 1, cur_touched, first_touched);
                    }
                }
            }
        }
    }

    fn ins_deltap(&mut self) {
        let p = self.cur_ppem() as u64;
        let nump = self.arg(0) as u64;
        let mut k = 1u64;
        while k <= nump {
            if self.args < 2 {
                if self.pedantic {
                    self.throw(Error::TooFewArguments);
                }
                self.args = 0;
                break;
            }
            self.args -= 2;
            let a = self.stack[self.args as usize + 1] as u16 as usize;
            let mut b = self.stack[self.args as usize];
            if a < self.npts(0) {
                let mut c = (b as u64 & 0xF0) >> 4;
                match self.opcode {
                    0x71 => c += 16,
                    0x72 => c += 32,
                    _ => {}
                }
                c += u64::from(self.gs.delta_base);
                if p == c {
                    b = (b as u64 & 0xF) as i64 - 8;
                    if b >= 0 {
                        b += 1;
                    }
                    b = b.wrapping_mul(1i64 << (6 - self.gs.delta_shift));
                    let z0 = self.zp[0];
                    if self.backward_compatibility {
                        let touched_y = self.zones[z0].tags[a] & TOUCH_Y != 0;
                        if !(self.iupx_called && self.iupy_called)
                            && ((self.is_composite && self.gs.free.y != 0) || touched_y)
                        {
                            self.move_point(z0, a, b);
                        }
                    } else {
                        self.move_point(z0, a, b);
                    }
                }
            } else {
                self.pedantic_ref();
            }
            k += 1;
        }
        self.new_top = self.args;
    }

    fn ins_deltac(&mut self) {
        let p = self.cur_ppem() as u64;
        let nump = self.arg(0) as u64;
        let mut k = 1u64;
        while k <= nump {
            if self.args < 2 {
                if self.pedantic {
                    self.throw(Error::TooFewArguments);
                }
                self.args = 0;
                break;
            }
            self.args -= 2;
            let a = self.stack[self.args as usize + 1] as u64;
            let mut b = self.stack[self.args as usize];
            if a >= self.cvt.len() as u64 {
                if self.pedantic {
                    self.throw(Error::InvalidReference);
                    return;
                }
            } else {
                let mut c = (b as u64 & 0xF0) >> 4;
                match self.opcode {
                    0x74 => c += 16,
                    0x75 => c += 32,
                    _ => {}
                }
                c += u64::from(self.gs.delta_base);
                if p == c {
                    b = (b as u64 & 0xF) as i64 - 8;
                    if b >= 0 {
                        b += 1;
                    }
                    b = b.wrapping_mul(1i64 << (6 - self.gs.delta_shift));
                    self.move_cvt(a as usize, b);
                }
            }
            k += 1;
        }
        self.new_top = self.args;
    }

    fn ins_getinfo(&mut self) {
        let a = self.arg(0);
        let mut k = 0i64;
        if a & 1 != 0 {
            k = INTERPRETER_VERSION;
        }
        if a & 32 != 0 && self.grayscale {
            k |= 1 << 12;
        }
        if self.subpixel_hinting_lean {
            if a & 64 != 0 {
                k |= 1 << 13;
            }
            if a & 256 != 0 && self.vertical_lcd_lean {
                k |= 1 << 15;
            }
            if a & 1024 != 0 {
                k |= 1 << 17;
            }
            if a & 2048 != 0 {
                k |= 1 << 18;
            }
            if a & 4096 != 0 && self.grayscale_cleartype {
                k |= 1 << 19;
            }
        }
        self.set_arg(0, k);
    }

    fn ins_push(&mut self, count: i64, words: bool, skip: i64) -> bool {
        if count >= self.stack.len() as i64 + 1 - self.top {
            self.throw(Error::StackOverflow);
            return false;
        }
        if words {
            self.ip += skip;
            for k in 0..count {
                self.ip += 2;
                let v = i16::from_be_bytes([self.code_at(self.ip - 2), self.code_at(self.ip - 1)]);
                self.set_arg(k, i64::from(v));
            }
            self.step_ins = false;
        } else {
            for k in 0..count {
                let v = self.code_at(self.ip + skip + k);
                self.set_arg(k, i64::from(v));
            }
        }
        true
    }

    /// `TT_RunIns`.
    pub fn run(&mut self) -> Result<(), Error> {
        let mut num_twilight = 30.max(2 * (self.zones[1].n_points + self.cvt.len()));
        if self.zones[0].n_points > num_twilight {
            num_twilight = num_twilight.min(0xFFFF);
            self.zones[0].n_points = num_twilight;
        }
        self.loopcall_counter = 0;
        self.neg_jump_counter = 0;
        let np = self.zones[1].n_points as u64;
        let cvt = self.cvt.len() as u64;
        self.loopcall_counter_max = if np != 0 { 50.max(10 * np) + 50.max(cvt / 10) } else { 300 + 22 * cvt };
        if self.loopcall_counter_max > 100 * self.num_glyphs {
            self.loopcall_counter_max = 100 * self.num_glyphs;
        }
        self.neg_jump_counter_max = self.loopcall_counter_max;
        self.ttm.ratio = 0;
        self.stretched = self.x_ppem != self.y_ppem;
        self.ini_range = self.cur_range;
        self.compute_funcs();
        self.compute_round(self.gs.round_state);
        self.iupx_called = false;
        self.iupy_called = false;
        let mut ins_counter = 0u64;
        loop {
            let code_size = self.code.len() as i64;
            if self.ip < 0 || self.ip >= code_size {
                // Só acontece com um intervalo vazio; o C leria fora do vetor.
                return Err(Error::CodeOverflow);
            }
            self.opcode = self.code_at(self.ip);
            self.length = opcode_length(self.opcode);
            if self.length < 0 {
                if self.ip + 1 >= code_size {
                    return Err(Error::CodeOverflow);
                }
                self.length = 2 - self.length * i64::from(self.code_at(self.ip + 1));
            }
            if self.ip + self.length > code_size {
                return Err(Error::CodeOverflow);
            }
            let pp = POP_PUSH[usize::from(self.opcode)];
            let pops = i64::from(pp >> 4);
            self.args = self.top - pops;
            if self.args < 0 {
                if self.pedantic {
                    return Err(Error::TooFewArguments);
                }
                for i in 0..pops as usize {
                    self.stack[i] = 0;
                }
                self.args = 0;
            }
            // Sem `blend`, o `GETVARIATION` deixa `new_top` como estava.
            if self.opcode != 0x91 {
                self.new_top = self.args + i64::from(pp & 15);
            }
            if self.new_top > self.stack.len() as i64 {
                return Err(Error::StackOverflow);
            }
            self.step_ins = true;
            self.error = None;
            self.execute();
            if let Some(e) = self.error {
                // O `Invalid_Opcode` ainda procura uma `IDEF` ativa, mas o `Ins_UNKNOWN` já
                // procurou a mesma coisa: aqui ela nunca é achada.
                return Err(e);
            }
            self.top = self.new_top;
            if self.step_ins {
                self.ip += self.length;
            }
            ins_counter += 1;
            if ins_counter > MAX_RUNNABLE_OPCODES {
                return Err(Error::ExecutionTooLong);
            }
            if self.ip >= self.code.len() as i64 {
                if self.call_top > 0 {
                    return Err(Error::CodeOverflow);
                }
                return Ok(());
            }
        }
    }

    fn execute(&mut self) {
        let op = self.opcode;
        match op {
            0x00..=0x05 => {
                let aa: i16 = i16::from(op & 1) << 14;
                let bb = aa ^ 0x4000;
                if op < 4 {
                    self.gs.proj = UnitVector { x: aa, y: bb };
                    self.gs.dual = self.gs.proj;
                }
                if op & 2 == 0 {
                    self.gs.free = UnitVector { x: aa, y: bb };
                }
                self.compute_funcs();
            }
            0x06 | 0x07 => {
                if self.sxvtl(self.arg(1) as u16, self.arg(0) as u16, 0) {
                    self.gs.dual = self.gs.proj;
                    self.compute_funcs();
                }
            }
            0x08 | 0x09 => {
                if self.sxvtl(self.arg(1) as u16, self.arg(0) as u16, 1) {
                    self.compute_funcs();
                }
            }
            0x0A => {
                let (y, x) = (i64::from(self.arg(1) as i16), i64::from(self.arg(0) as i16));
                normalize(x, y, &mut self.gs.proj);
                self.gs.dual = self.gs.proj;
                self.compute_funcs();
            }
            0x0B => {
                let (y, x) = (i64::from(self.arg(1) as i16), i64::from(self.arg(0) as i16));
                normalize(x, y, &mut self.gs.free);
                self.compute_funcs();
            }
            0x0C => {
                self.set_arg(0, i64::from(self.gs.proj.x));
                self.set_arg(1, i64::from(self.gs.proj.y));
            }
            0x0D => {
                self.set_arg(0, i64::from(self.gs.free.x));
                self.set_arg(1, i64::from(self.gs.free.y));
            }
            0x0E => {
                self.gs.free = self.gs.proj;
                self.compute_funcs();
            }
            0x0F => self.ins_isect(),
            0x10 => self.gs.rp0 = self.arg(0) as u16,
            0x11 => self.gs.rp1 = self.arg(0) as u16,
            0x12 => self.gs.rp2 = self.arg(0) as u16,
            0x13 => self.ins_szp(0),
            0x14 => self.ins_szp(1),
            0x15 => self.ins_szp(2),
            0x16 => self.ins_szp(3),
            0x17 => {
                let v = self.arg(0);
                if v < 0 {
                    self.throw(Error::BadArgument);
                } else {
                    self.gs.loop_ = v.min(0xFFFF);
                }
            }
            0x18 => self.set_round(ROUND_TO_GRID),
            0x19 => self.set_round(ROUND_TO_HALF_GRID),
            0x1A => self.gs.minimum_distance = self.arg(0),
            0x1B => self.ins_else(),
            0x1C => self.ins_jmpr(self.arg(0)),
            0x1D => self.gs.control_value_cutin = self.arg(0),
            0x1E => self.gs.single_width_cutin = self.arg(0),
            0x1F => self.gs.single_width_value = mul_fix(self.arg(0), self.ttm.scale),
            0x20 => self.set_arg(1, self.arg(0)),
            0x21 => {}
            0x22 => self.new_top = 0,
            0x23 => {
                let (a, b) = (self.arg(0), self.arg(1));
                self.set_arg(0, b);
                self.set_arg(1, a);
            }
            0x24 => self.set_arg(0, self.top),
            0x25 => {
                let l = self.arg(0);
                if l <= 0 || l > self.args {
                    if self.pedantic {
                        self.throw(Error::InvalidReference);
                    }
                    self.set_arg(0, 0);
                } else {
                    self.set_arg(0, self.stack[(self.args - l) as usize]);
                }
            }
            0x26 => {
                let l = self.arg(0);
                if l <= 0 || l > self.args {
                    if self.pedantic {
                        self.throw(Error::InvalidReference);
                    }
                } else {
                    let base = (self.args - l) as usize;
                    let k = self.stack[base];
                    self.stack.copy_within(base + 1..base + l as usize, base);
                    self.stack[self.args as usize - 1] = k;
                }
            }
            0x27 => self.ins_alignpts(),
            0x29 => self.ins_utp(),
            0x2A => self.ins_loopcall(),
            0x2B => self.ins_call(),
            0x2C => self.ins_fdef(),
            0x2D => self.ins_endf(),
            0x2E | 0x2F => self.ins_mdap(),
            0x30 | 0x31 => self.ins_iup(),
            0x32 | 0x33 => self.ins_shp(),
            0x34 | 0x35 => self.ins_shc(),
            0x36 | 0x37 => self.ins_shz(),
            0x38 => self.ins_shpix(),
            0x39 => self.ins_ip(),
            0x3A | 0x3B => self.ins_msirp(),
            0x3C => self.ins_alignrp(),
            0x3D => self.set_round(ROUND_TO_DOUBLE_GRID),
            0x3E | 0x3F => self.ins_miap(),
            0x40 => {
                let l = i64::from(self.code_at(self.ip + 1));
                if self.ins_push(l, false, 2) {
                    self.new_top += l;
                }
            }
            0x41 => {
                let l = i64::from(self.code_at(self.ip + 1));
                if self.ins_push(l, true, 2) {
                    self.new_top += l;
                }
            }
            0x42 => {
                let i = self.arg(0) as u64;
                if i >= self.storage.len() as u64 {
                    self.pedantic_ref();
                } else {
                    if self.ini_range == RANGE_GLYPH && !self.storage_diverted {
                        self.glyf_storage.clear();
                        self.glyf_storage.extend_from_slice(&self.storage);
                        self.storage_diverted = true;
                    }
                    let v = self.arg(1);
                    if self.storage_diverted {
                        self.glyf_storage[i as usize] = v;
                    } else {
                        self.storage[i as usize] = v;
                    }
                }
            }
            0x43 => {
                let i = self.arg(0) as u64;
                if i >= self.storage.len() as u64 {
                    self.pedantic_ref();
                    self.set_arg(0, 0);
                } else {
                    let v = if self.storage_diverted { self.glyf_storage[i as usize] } else { self.storage[i as usize] };
                    self.set_arg(0, v);
                }
            }
            0x44 => {
                let i = self.arg(0) as u64;
                if i >= self.cvt.len() as u64 {
                    self.pedantic_ref();
                } else {
                    self.write_cvt(i as usize, self.arg(1));
                }
            }
            0x45 => {
                let i = self.arg(0) as u64;
                if i >= self.cvt.len() as u64 {
                    self.pedantic_ref();
                    self.set_arg(0, 0);
                } else {
                    let v = self.read_cvt(i as usize);
                    self.set_arg(0, v);
                }
            }
            0x46 | 0x47 => {
                let l = self.arg(0) as u64;
                let r = if l >= self.npts(2) as u64 {
                    self.pedantic_ref();
                    0
                } else if op & 1 != 0 {
                    self.fast_dualproj(self.zone(2).org[l as usize])
                } else {
                    self.fast_project(self.zone(2).cur[l as usize])
                };
                self.set_arg(0, r);
            }
            0x48 => {
                let l = self.arg(0) as u16 as usize;
                if l >= self.npts(2) {
                    self.pedantic_ref();
                } else {
                    let z2 = self.zp[2];
                    let k = self.fast_project(self.zones[z2].cur[l]);
                    self.move_point(z2, l, self.arg(1).wrapping_sub(k));
                    if self.gs.gep2 == 0 {
                        self.zones[z2].org[l] = self.zones[z2].cur[l];
                    }
                }
            }
            0x49 | 0x4A => {
                let k = self.arg(1) as u16 as usize;
                let l = self.arg(0) as u16 as usize;
                let d = if l >= self.npts(0) || k >= self.npts(1) {
                    self.pedantic_ref();
                    0
                } else if op & 1 != 0 {
                    self.project(self.zone(0).cur[l], self.zone(1).cur[k])
                } else {
                    self.orus_dist(self.zp[0], l, self.zp[1], k)
                };
                self.set_arg(0, d);
            }
            0x4B => {
                let v = self.cur_ppem();
                self.set_arg(0, v);
            }
            0x4C => self.set_arg(0, self.point_size),
            0x4D => self.gs.auto_flip = true,
            0x4E => self.gs.auto_flip = false,
            0x4F => self.throw(Error::DebugOpcode),
            0x50..=0x55 => {
                let (a, b) = (self.arg(0), self.arg(1));
                let r = match op {
                    0x50 => a < b,
                    0x51 => a <= b,
                    0x52 => a > b,
                    0x53 => a >= b,
                    0x54 => a == b,
                    _ => a != b,
                };
                self.set_arg(0, i64::from(r));
            }
            0x56 => {
                let r = self.round(self.arg(0), 3) & 127 == 64;
                self.set_arg(0, i64::from(r));
            }
            0x57 => {
                let r = self.round(self.arg(0), 3) & 127 == 0;
                self.set_arg(0, i64::from(r));
            }
            0x58 => self.ins_if(),
            0x59 => {}
            0x5A => self.set_arg(0, i64::from(self.arg(0) != 0 && self.arg(1) != 0)),
            0x5B => self.set_arg(0, i64::from(self.arg(0) != 0 || self.arg(1) != 0)),
            0x5C => self.set_arg(0, i64::from(self.arg(0) == 0)),
            0x5D | 0x71 | 0x72 => self.ins_deltap(),
            0x5E => self.gs.delta_base = self.arg(0) as u16,
            0x5F => {
                if self.arg(0) as u64 > 6 {
                    self.throw(Error::BadArgument);
                } else {
                    self.gs.delta_shift = self.arg(0) as u16;
                }
            }
            0x60 => self.set_arg(0, self.arg(0).wrapping_add(self.arg(1))),
            0x61 => self.set_arg(0, self.arg(0).wrapping_sub(self.arg(1))),
            0x62 => {
                if self.arg(1) == 0 {
                    self.throw(Error::DivideByZero);
                } else {
                    self.set_arg(0, mul_div_no_round(self.arg(0), 64, self.arg(1)));
                }
            }
            0x63 => self.set_arg(0, mul_div(self.arg(0), self.arg(1), 64)),
            0x64 => {
                if self.arg(0) < 0 {
                    self.set_arg(0, self.arg(0).wrapping_neg());
                }
            }
            0x65 => self.set_arg(0, self.arg(0).wrapping_neg()),
            0x66 => self.set_arg(0, pix_floor(self.arg(0))),
            0x67 => self.set_arg(0, pix_ceil_l(self.arg(0))),
            0x68..=0x6B => self.set_arg(0, self.round(self.arg(0), usize::from(op & 3))),
            0x6C..=0x6F => self.set_arg(0, self.round_with(ROUND_OFF, self.arg(0), usize::from(op & 3))),
            0x70 => {
                let i = self.arg(0) as u64;
                if i >= self.cvt.len() as u64 {
                    self.pedantic_ref();
                } else {
                    // Sem `Modify_CVT_Check`: no programa do glifo, escreve no CVT do tamanho
                    // enquanto ele não foi desviado.
                    let v = mul_fix(self.arg(1), self.ttm.scale);
                    self.cvt_put(i as usize, v);
                }
            }
            0x73..=0x75 => self.ins_deltac(),
            0x76 => {
                self.set_super_round(0x4000, self.arg(0));
                self.set_round(ROUND_SUPER);
            }
            0x77 => {
                self.set_super_round(0x2D41, self.arg(0));
                self.set_round(ROUND_SUPER_45);
            }
            0x78 => {
                if self.arg(1) != 0 {
                    self.ins_jmpr(self.arg(0));
                }
            }
            0x79 => {
                if self.arg(1) == 0 {
                    self.ins_jmpr(self.arg(0));
                }
            }
            0x7A => self.set_round(ROUND_OFF),
            0x7C => self.set_round(ROUND_UP_TO_GRID),
            0x7D => self.set_round(ROUND_DOWN_TO_GRID),
            0x7E | 0x7F => {}
            0x80 => self.ins_flippt(),
            0x81 => self.ins_fliprg(true),
            0x82 => self.ins_fliprg(false),
            0x85 => self.ins_scanctrl(),
            0x86 | 0x87 => self.ins_sdpvtl(),
            0x88 => self.ins_getinfo(),
            0x89 => self.ins_idef(),
            0x8A => {
                let (a, b, c) = (self.arg(2), self.arg(1), self.arg(0));
                self.set_arg(2, c);
                self.set_arg(1, a);
                self.set_arg(0, b);
            }
            0x8B => {
                if self.arg(1) > self.arg(0) {
                    self.set_arg(0, self.arg(1));
                }
            }
            0x8C => {
                if self.arg(1) < self.arg(0) {
                    self.set_arg(0, self.arg(1));
                }
            }
            0x8D => {
                if self.arg(0) >= 0 {
                    self.gs.scan_type = (self.arg(0) as i32) & 0xFFFF;
                }
            }
            0x8E => self.ins_instctrl(),
            0xE0..=0xFF => self.ins_mirp(),
            0xC0..=0xDF => self.ins_mdrp(),
            0xB8..=0xBF => {
                let l = i64::from(op - 0xB8 + 1);
                self.ins_push(l, true, 1);
            }
            0xB0..=0xB7 => {
                let l = i64::from(op - 0xB0 + 1);
                self.ins_push(l, false, 1);
            }
            // 0x28, 0x7B, 0x83, 0x84, 0x8F a 0xAF: só uma `IDEF` os define.
            _ => self.ins_unknown(),
        }
    }

    fn set_round(&mut self, mode: i32) {
        self.gs.round_state = mode;
        self.round = mode;
    }
}

fn set_axis(v: &mut Vector, x_axis: bool, val: i64) {
    if x_axis {
        v.x = val;
    } else {
        v.y = val;
    }
}

/// `iup_worker_interpolate_` sobre um eixo.
fn iup_interpolate(z: &mut Zone, x_axis: bool, p1: usize, p2: usize, mut ref1: usize, mut ref2: usize) {
    let get = |v: &Vector| if x_axis { v.x } else { v.y };
    if p1 > p2 {
        return;
    }
    if ref1 >= z.n_points || ref2 >= z.n_points {
        return;
    }
    let mut orus1 = get(&z.orus[ref1]);
    let mut orus2 = get(&z.orus[ref2]);
    if orus1 > orus2 {
        std::mem::swap(&mut orus1, &mut orus2);
        std::mem::swap(&mut ref1, &mut ref2);
    }
    let org1 = get(&z.org[ref1]);
    let org2 = get(&z.org[ref2]);
    let cur1 = get(&z.cur[ref1]);
    let cur2 = get(&z.cur[ref2]);
    let delta1 = cur1.wrapping_sub(org1);
    let delta2 = cur2.wrapping_sub(org2);
    let mut scale: Option<i64> = None;
    for i in p1..=p2 {
        let x = get(&z.org[i]);
        let v = if x <= org1 {
            x.wrapping_add(delta1)
        } else if x >= org2 {
            x.wrapping_add(delta2)
        } else if cur1 == cur2 || orus1 == orus2 {
            cur1
        } else {
            let s = *scale.get_or_insert_with(|| div_fix(cur2.wrapping_sub(cur1), orus2.wrapping_sub(orus1)));
            cur1.wrapping_add(mul_fix(get(&z.orus[i]).wrapping_sub(orus1), s))
        };
        set_axis(&mut z.cur[i], x_axis, v);
    }
}

/// O que o `tt_loader_init` decide antes de carregar um glifo com hinting.
pub(crate) struct LoadSetup {
    /// O `instruct_control` pediu `FT_LOAD_NO_HINTING`.
    pub no_hinting: bool,
    /// O deslocamento das larguras do `hdmx` que o `compute_glyph_metrics` usa.
    pub widthp: Option<usize>,
}

impl TtSize {
    /// A parte do `tt_loader_init` que prepara o contexto (`IS_HINTED` e não só a `glyf`).
    pub fn prepare_load(&mut self, progs: &Programs, mono: bool, is_fixed_pitch: bool) -> Result<LoadSetup, Error> {
        if self.bytecode_ready == Ready::Pending || self.cvt_ready == Ready::Pending {
            if let Some(e) = self.ready_bytecode(progs) {
                return Err(e);
            }
        } else if let Ready::Done(Some(e)) = self.bytecode_ready {
            return Err(e);
        } else if let Ready::Done(Some(e)) = self.cvt_ready {
            return Err(e);
        }
        let mut exec = self.exec.take().expect("contexto pronto");
        let lean = !mono;
        let cleartype = lean;
        exec.vertical_lcd_lean = false;
        let grayscale = !mono && !lean;
        exec.load_context(self, progs);
        let mut reexecute = false;
        if lean != exec.subpixel_hinting_lean {
            exec.subpixel_hinting_lean = lean;
            reexecute = true;
        }
        if cleartype != exec.grayscale_cleartype {
            exec.grayscale_cleartype = cleartype;
            reexecute = true;
        }
        if grayscale != exec.grayscale {
            exec.grayscale = grayscale;
            reexecute = true;
        }
        self.exec = Some(exec);
        if reexecute {
            if let Some(e) = self.run_prep(progs) {
                return Err(e);
            }
            let mut exec = self.exec.take().expect("contexto pronto");
            exec.load_context(self, progs);
            self.exec = Some(exec);
        }
        let exec = self.exec.as_mut().expect("contexto pronto");
        let no_hinting = exec.gs.instruct_control & 1 != 0;
        if exec.gs.instruct_control & 2 != 0 {
            exec.gs = DEFAULT_GS;
        }
        exec.backward_compatibility = lean && exec.gs.instruct_control & 4 == 0;
        exec.pedantic = false;
        // No C, `loader->load_flags` ainda está zerado aqui: só contam a compatibilidade e o
        // passo fixo.
        let widthp = if !exec.backward_compatibility && !is_fixed_pitch { self.widthp } else { None };
        Ok(LoadSetup { no_hinting, widthp })
    }

    /// `GS.scan_control` e `GS.scan_type` do contexto depois da carga do glifo.
    pub fn scan_mode(&self) -> Option<i32> {
        self.exec.as_ref().filter(|e| e.gs.scan_control).map(|e| e.gs.scan_type)
    }

    /// `TT_Hint_Glyph`: `zone` já tem `cur`, `orus` e os quatro fantasmas no fim. Devolve os
    /// fantasmas a copiar de volta para o carregador, ou `None` no modo de compatibilidade.
    pub fn hint_glyph(&mut self, zone: Zone, ins: Option<Rc<[u8]>>, is_composite: bool) -> (Zone, Option<[Vector; 4]>, Option<u8>) {
        let gs = self.gs;
        let (xs, ys) = (self.x_scale, self.y_scale);
        let exec = self.exec.as_mut().expect("contexto pronto");
        let mut zone = zone;
        let n_ins = ins.as_ref().map_or(0, |i| i.len());
        if n_ins > 0 {
            zone.org = zone.cur.clone();
        }
        exec.gs = gs;
        if is_composite {
            exec.set_glyph_metrics(0x10000, 0x10000, true);
            zone.orus = zone.cur.clone();
        } else {
            exec.set_glyph_metrics(xs, ys, false);
        }
        let n = zone.n_points;
        zone.cur[n - 4].x = crate::calc::pix_round(zone.cur[n - 4].x);
        zone.cur[n - 3].x = crate::calc::pix_round(zone.cur[n - 3].x);
        zone.cur[n - 2].y = crate::calc::pix_round(zone.cur[n - 2].y);
        zone.cur[n - 1].y = crate::calc::pix_round(zone.cur[n - 1].y);
        let mut scan = None;
        if n_ins > 0 {
            exec.glyph_ins = ins;
            exec.is_composite = is_composite;
            exec.zones[1] = zone;
            // Sem `FT_LOAD_PEDANTIC`, o erro do programa do glifo é ignorado.
            let _ = exec.run_context();
            zone = exec.zones[1].clone();
            scan = Some(((exec.gs.scan_type << 5) as u8) | HAS_SCANMODE);
        }
        let pp = if exec.backward_compatibility {
            None
        } else {
            Some([zone.cur[n - 4], zone.cur[n - 3], zone.cur[n - 2], zone.cur[n - 1]])
        };
        (zone, pp, scan)
    }
}
