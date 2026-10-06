//! `PIL._imagingft`: o `_imagingft.c` do Pillow 11.1.0 sobre o porte do FreeType 2.13.3 (`zft`),
//! com o layout básico (sem Raqm): um glifo por caractere, kerning da tabela `kern`.

use std::cell::RefCell;
use std::rc::Rc;

use sysabi::{sys, OFlags};
use zft::{Face, LOAD_TARGET_MONO};

use super::{core_of, float_arg, int_arg};
use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// `PIXEL(x)`: de 26.6 para pixels, arredondando.
fn pixel(x: i64) -> i64 {
    ((x + 32) & -64) >> 6
}

fn ft_error(e: zft::Error) -> PyException {
    exc("OSError", e.message())
}

/// `GlyphInfo`.
#[derive(Clone, Copy, Default)]
struct GlyphInfo {
    index: u32,
    x_offset: i64,
    y_offset: i64,
    x_advance: i64,
    y_advance: i64,
}

pub struct FontObj {
    face: RefCell<Face>,
}

/// Um argumento `z` do `PyArg_ParseTuple`: `str` ou `None`.
fn opt_str(v: Option<&Value>) -> PyResult<Option<String>> {
    match v {
        None | Some(Value::None) => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.as_str().to_string())),
        Some(other) => Err(type_error(format!("argument must be str or None, not {}", other.type_name()))),
    }
}

struct LayoutArgs {
    mode: Option<String>,
    dir: Option<String>,
    features: Value,
    lang: Option<String>,
}

impl LayoutArgs {
    fn parse(args: &[Value], from: usize) -> PyResult<LayoutArgs> {
        Ok(LayoutArgs {
            mode: opt_str(args.get(from))?,
            dir: opt_str(args.get(from + 1))?,
            features: args.get(from + 2).cloned().unwrap_or(Value::None),
            lang: opt_str(args.get(from + 3))?,
        })
    }

    fn mask(&self) -> bool {
        self.mode.as_deref() == Some("1")
    }

    fn color(&self) -> bool {
        self.mode.as_deref() == Some("RGBA")
    }

    fn horizontal(&self) -> bool {
        self.dir.as_deref() != Some("ttb")
    }

    fn load_flags(&self) -> u32 {
        if self.mask() {
            LOAD_TARGET_MONO
        } else {
            0
        }
    }
}

impl FontObj {
    /// `text_layout_fallback`.
    fn layout(&self, string: &Value, a: &LayoutArgs) -> PyResult<Vec<GlyphInfo>> {
        let unsupported = !matches!(a.features, Value::None) || a.dir.is_some() || a.lang.is_some();
        let chars: Vec<u32> = match string {
            Value::Str(s) => s.as_str().chars().map(u32::from).collect(),
            // O C lê `char` com sinal: acima de 127 vira um código enorme, sem glifo.
            Value::Bytes(b) => b.iter().map(|&c| if c < 128 { u32::from(c) } else { u32::MAX }).collect(),
            _ => {
                if unsupported {
                    return Err(not_supported());
                }
                return Err(type_error("expected string or bytes"));
            }
        };
        if unsupported {
            return Err(not_supported());
        }
        let mut face = self.face.borrow_mut();
        let kerning = face.has_kerning();
        let flags = a.load_flags();
        let mut out: Vec<GlyphInfo> = Vec::with_capacity(chars.len());
        let mut last = 0;
        for &ch in &chars {
            let index = face.char_index(ch);
            let slot = face.load_glyph(index, flags).map_err(ft_error)?;
            if kerning && last != 0 && index != 0 {
                let (dx, dy) = face.kerning(last, index);
                if let Some(prev) = out.last_mut() {
                    prev.x_advance += pixel(dx);
                    prev.y_advance += pixel(dy);
                }
            }
            out.push(GlyphInfo { index, x_offset: 0, y_offset: 0, x_advance: slot.hori_advance, y_advance: 0 });
            last = index;
        }
        Ok(out)
    }

    /// `bounding_box_and_anchors`: `(largura, altura, x_offset, y_offset)`.
    fn bbox(&self, anchor: Option<&str>, horizontal: bool, glyphs: &[GlyphInfo], flags: u32) -> PyResult<(i64, i64, i64, i64)> {
        let mut face = self.face.borrow_mut();
        let (mut position, mut x_min, mut x_max, mut y_min, mut y_max) = (0i64, 0i64, 0i64, 0i64, 0i64);
        for g in glyphs {
            let (px, py);
            if horizontal {
                px = pixel(position + g.x_offset);
                py = pixel(g.y_offset);
                position += g.x_advance;
                x_max = x_max.max(pixel(position));
            } else {
                px = pixel(g.x_offset);
                py = pixel(position + g.y_offset);
                position += g.y_advance;
                y_min = y_min.min(pixel(position));
            }
            let slot = face.load_glyph(g.index, flags).map_err(ft_error)?;
            // `FT_Glyph_Get_CBox` com `FT_GLYPH_BBOX_PIXELS`.
            let b = slot.outline.cbox();
            x_max = x_max.max((b.x_max + 63).div_euclid(64) + px);
            x_min = x_min.min(b.x_min.div_euclid(64) + px);
            y_max = y_max.max((b.y_max + 63).div_euclid(64) + py);
            y_min = y_min.min(b.y_min.div_euclid(64) + py);
        }
        let anchor_s = anchor.unwrap_or(if horizontal { "la" } else { "lt" });
        let bad = || exc("ValueError", format!("bad anchor specified: {anchor_s}"));
        let a: Vec<char> = anchor_s.chars().collect();
        if anchor_s.len() != 2 || a.len() != 2 {
            return Err(bad());
        }
        let (mut x_anchor, mut y_anchor) = (0, 0);
        if !glyphs.is_empty() {
            let m = face.size;
            if horizontal {
                x_anchor = match a[0] {
                    'l' => 0,
                    'm' => pixel(position / 2),
                    'r' => pixel(position),
                    _ => return Err(bad()),
                };
                y_anchor = match a[1] {
                    'a' => pixel(m.ascender),
                    't' => y_max,
                    'm' => pixel((m.ascender + m.descender) / 2),
                    's' => 0,
                    'b' => y_min,
                    'd' => pixel(m.descender),
                    _ => return Err(bad()),
                };
            } else {
                x_anchor = match a[0] {
                    'l' => x_min,
                    'm' => (x_min + x_max) / 2,
                    'r' => x_max,
                    's' => 0,
                    _ => return Err(bad()),
                };
                y_anchor = match a[1] {
                    't' => 0,
                    'm' => pixel(position / 2),
                    'b' => pixel(position),
                    _ => return Err(bad()),
                };
            }
        }
        Ok((x_max - x_min, y_max - y_min, -x_anchor + x_min, -(-y_anchor + y_max)))
    }

    fn getsize(&self, args: &[Value]) -> PyResult<Value> {
        let string = args.first().ok_or_else(|| type_error("getsize() takes at least 1 argument (0 given)"))?;
        let a = LayoutArgs::parse(args, 1)?;
        let anchor = opt_str(args.get(5))?;
        let glyphs = self.layout(string, &a)?;
        let (w, h, x, y) = self.bbox(anchor.as_deref(), a.horizontal(), &glyphs, a.load_flags())?;
        Ok(Value::tuple(vec![
            Value::tuple(vec![Value::Int(w), Value::Int(h)]),
            Value::tuple(vec![Value::Int(x), Value::Int(y)]),
        ]))
    }

    fn getlength(&self, args: &[Value]) -> PyResult<Value> {
        let string = args.first().ok_or_else(|| type_error("getlength() takes at least 1 argument (0 given)"))?;
        let a = LayoutArgs::parse(args, 1)?;
        let glyphs = self.layout(string, &a)?;
        let horizontal = a.horizontal();
        let length: i64 = glyphs.iter().map(|g| if horizontal { g.x_advance } else { -g.y_advance }).sum();
        // O C acumula num `int`.
        Ok(Value::Int(i64::from(length as i32)))
    }

    /// `font_render`.
    fn render(&self, vm: &mut Vm, args: &[Value]) -> PyResult<Value> {
        if args.len() < 2 {
            return Err(type_error(format!("render() takes at least 2 arguments ({} given)", args.len())));
        }
        let (string, fill) = (&args[0], &args[1]);
        let a = LayoutArgs::parse(args, 2)?;
        let stroke_width = match args.get(6) {
            Some(v) => float_arg(v)? as f32,
            None => 0.0,
        };
        let anchor = opt_str(args.get(7))?;
        let ink = match args.get(8) {
            Some(v) => int_arg(v)? as u32,
            None => 0,
        };
        let x_start = match args.get(9) {
            Some(v) => float_arg(v)? as f32,
            None => 0.0,
        };
        let y_start = match args.get(10) {
            Some(v) => float_arg(v)? as f32,
            None => 0.0,
        };
        if stroke_width != 0.0 {
            return Err(exc("OSError", "unsupported bitmap pixel mode"));
        }
        let color = a.color();
        let mask = a.mask();
        let glyphs = self.layout(string, &a)?;
        let load_flags = a.load_flags();
        let (mut width, mut height, x_offset, y_offset) = self.bbox(anchor.as_deref(), a.horizontal(), &glyphs, load_flags)?;
        width += f64::from(stroke_width * 2.0 + x_start).ceil() as i64;
        height += f64::from(stroke_width * 2.0 + y_start).ceil() as i64;
        let image = vm.call_value(fill, vec![Value::Int(width), Value::Int(height)], Kw::default())?;
        let shared = core_of(&image)?;
        let x_offset = (x_offset as f32 - stroke_width).round() as i64;
        let y_offset = (y_offset as f32 - stroke_width).round() as i64;
        let result = |image: Value| Value::tuple(vec![image, Value::tuple(vec![Value::Int(x_offset), Value::Int(y_offset)])]);
        if glyphs.is_empty() || width == 0 || height == 0 {
            return Ok(result(image));
        }
        let mut face = self.face.borrow_mut();
        // Primeira passada: a origem do texto dentro da imagem.
        let (mut x, mut y, mut x_min, mut y_max) = (0i64, 0i64, 0i64, 0i64);
        for g in &glyphs {
            let px = pixel(x + g.x_offset);
            let py = pixel(y + g.y_offset);
            let slot = face.load_glyph(g.index, load_flags).map_err(ft_error)?;
            let (left, top) = render_slot(&slot, mask).map_or((0, 0), |b| (i64::from(b.left), i64::from(b.top)));
            y_max = y_max.max(top + py);
            x_min = x_min.min(left + px);
            x += g.x_advance;
            y += g.y_advance;
        }
        x = ((-x_min as f32 + stroke_width + x_start) * 64.0).round() as i64;
        y = ((-y_max as f32 - stroke_width - y_start) * 64.0).round() as i64;
        let mut im = shared.borrow_mut();
        let (xsize, ysize, linesize) = (i64::from(im.xsize), i64::from(im.ysize), im.linesize as usize);
        let ink = ink.to_le_bytes();
        for g in &glyphs {
            let px = pixel(x + g.x_offset);
            let py = pixel(y + g.y_offset);
            let slot = face.load_glyph(g.index, load_flags).map_err(ft_error)?;
            if let Some(bm) = render_slot(&slot, mask) {
                let xx = px + i64::from(bm.left);
                let mut yy = -(py + i64::from(bm.top));
                let x0 = if xx < 0 { -xx } else { 0 };
                let x1 = if xx + i64::from(bm.width) > xsize { xsize - xx } else { i64::from(bm.width) };
                let mut src = 0usize;
                for _ in 0..bm.rows {
                    if yy >= 0 && yy < ysize {
                        let row = yy as usize * linesize;
                        for k in x0..x1 {
                            let alpha = u32::from(bm.buffer[src + k as usize]);
                            if alpha == 0 {
                                continue;
                            }
                            if color {
                                let t = row + ((xx + k) * 4) as usize;
                                let target = &mut im.data[t..t + 4];
                                if target[3] > 0 {
                                    for c in 0..3 {
                                        target[c] = blend(alpha, u32::from(target[c]), u32::from(ink[c]));
                                    }
                                    target[3] = clip8(alpha + muldiv255(u32::from(target[3]), 255 - alpha));
                                } else {
                                    target[..3].copy_from_slice(&ink[..3]);
                                    target[3] = alpha as u8;
                                }
                            } else {
                                let t = row + (xx + k) as usize;
                                let cur = u32::from(im.data[t]);
                                im.data[t] = if cur > 0 { clip8(alpha + muldiv255(cur, 255 - alpha)) } else { alpha as u8 };
                            }
                        }
                    }
                    src = (src as i64 + i64::from(bm.pitch)) as usize;
                    yy += 1;
                }
            }
            x += g.x_advance;
            y += g.y_advance;
        }
        drop(im);
        Ok(result(image))
    }
}

/// `FT_Load_Glyph` com `FT_LOAD_RENDER`; no modo mono, o `FT_Bitmap_Convert` com alinhamento 1 e
/// o `convert_scale` de 255 do `font_render`, que deixam um byte por pixel, 0 ou 255.
fn render_slot(slot: &zft::Slot, mask: bool) -> Option<zft::raster::Bitmap> {
    if !mask {
        return slot.render();
    }
    let mut bm = slot.render_mono()?;
    let (w, pitch) = (bm.width as usize, bm.pitch as usize);
    let mut out = vec![0u8; w * bm.rows as usize];
    for (r, line) in out.chunks_mut(w.max(1)).enumerate().take(bm.rows as usize) {
        for (k, px) in line.iter_mut().enumerate() {
            if bm.buffer[r * pitch + k / 8] & (0x80 >> (k & 7)) != 0 {
                *px = 255;
            }
        }
    }
    bm.buffer = out;
    bm.pitch = w as i32;
    Some(bm)
}

fn not_supported() -> PyException {
    exc("KeyError", "setting text direction, language or font features is not supported without libraqm")
}

fn muldiv255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    ((t >> 8) + t) >> 8
}

fn blend(mask: u32, in1: u32, in2: u32) -> u8 {
    (muldiv255(in1, 255 - mask) + muldiv255(in2, mask)) as u8
}

fn clip8(v: u32) -> u8 {
    v.min(255) as u8
}

impl ExtObject for FontObj {
    fn type_name(&self) -> &'static str {
        "Font"
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn methods(&self) -> &'static [&'static str] {
        &["render", "getsize", "getlength"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let face = self.face.borrow();
        let m = face.size;
        let opt = |s: Option<String>| s.map_or(Value::None, Value::str);
        Some(Ok(match name {
            "family" => opt(face.family_name()),
            "style" => opt(face.style_name()),
            "ascent" => Value::Int(pixel(m.ascender)),
            "descent" => Value::Int(-pixel(m.descender)),
            "height" => Value::Int(pixel(m.height)),
            "x_ppem" => Value::Int(i64::from(m.x_ppem)),
            "y_ppem" => Value::Int(i64::from(m.y_ppem)),
            "glyphs" => Value::Int(i64::from(face.num_glyphs())),
            _ => return None,
        }))
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "render" => self.render(vm, &args),
            "getsize" => self.getsize(&args),
            "getlength" => self.getlength(&args),
            _ => Err(exc("AttributeError", format!("'Font' object has no attribute '{name}'"))),
        }
    }
}

/// Lê o arquivo inteiro, como o `FT_New_Face` faz pelo `FT_Stream`.
fn read_font(path: &[u8]) -> Option<Vec<u8>> {
    let fd = sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0).ok()?;
    let r = sys::read_to_end(fd).ok();
    let _ = sys::close(fd);
    r
}

fn getfont(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("getfont", args, kw, &["filename", "size", "index", "encoding", "font_bytes", "layout_engine"], 2)?;
    let filename: Vec<u8> = match s[0].as_ref() {
        Some(Value::Str(p)) => p.as_str().as_bytes().to_vec(),
        Some(v @ Value::Bytes(_)) => v.bytes_like().map(|b| b.to_vec()).unwrap_or_default(),
        Some(other) => return Err(type_error(format!("argument 1 must be str, not {}", other.type_name()))),
        None => Vec::new(),
    };
    let size = float_arg(s[1].as_ref().unwrap_or(&Value::None))? as f32;
    let index = match s[2].as_ref() {
        Some(v) => int_arg(v)?,
        None => 0,
    };
    let encoding = opt_str(s[3].as_ref())?;
    let font_bytes = match s[4].as_ref() {
        Some(v) => Some(v.bytes_like().ok_or_else(|| type_error(format!("argument 5 must be bytes, not {}", v.type_name())))?.to_vec()),
        None => None,
    };
    if let Some(v) = s[5].as_ref() {
        int_arg(v)?;
    }
    let data = match font_bytes.filter(|b| !b.is_empty()) {
        Some(b) => b,
        None => read_font(&filename).ok_or_else(|| exc("OSError", "cannot open resource"))?,
    };
    let mut face = Face::new(data, usize::try_from(index).unwrap_or(usize::MAX)).map_err(ft_error)?;
    let width = (size * 64.0) as i64;
    face.request_size(width, width).map_err(ft_error)?;
    if let Some(e) = encoding.filter(|e| e.len() == 4) {
        // A única tabela que o porte carrega é a Unicode.
        if e != "unic" {
            return Err(exc("OSError", "invalid argument"));
        }
    }
    Ok(Value::Ext(Rc::new(FontObj { face: RefCell::new(face) })))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("PIL._imagingft")
        .func("getfont", getfont)
        .value("freetype2_version", Value::str("2.13.3"))
        .value("HAVE_RAQM", Value::Bool(false))
        .value("HAVE_FRIBIDI", Value::Bool(false))
        .value("HAVE_HARFBUZZ", Value::Bool(false))
        .build()
}
