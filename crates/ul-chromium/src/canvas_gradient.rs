//! O shader de gradiente do Skia que o Chromium usa no canvas 2D, refeito em binário32 para os pixels saírem iguais:
//! a matriz que leva o ponto do dispositivo ao parâmetro `t` (`pts_to_unit` do `SkLinearGradient`, a do radial e a
//! do sweep, compostas com a inversa da transformação como o `SkMatrix`), a cor na forma `f*t + b` por intervalo, a
//! interpolação em cores não premultiplicadas, a premultiplicação, o alfa da pintura e o dither 8x8 que o Chromium
//! liga nos gradientes (`SkRasterPipeline`: `dither` e `store_8888` com arredondamento ao par). Medido no oráculo
//! (`testbench/oracle/measured/canvas-gradient.html`): linear, com transformação e `globalAlpha`, várias paradas,
//! radial concêntrica e cônica saem iguais pixel a pixel.

use vello_cpu::kurbo::Affine;
use vello_cpu::{PixelMetadata, Pixmap};
use vello_cpu::peniko::ImageAlphaType;

/// Uma afim do `SkMatrix`, em binário32: [sx, kx, tx, ky, sy, ty].
type Matrix = [f32; 6];

/// O `mad` do pipeline: no x86-64 do oráculo (HSW) é a multiplicação e soma fundidas.
fn mad(a: f32, b: f32, c: f32) -> f32 {
    a.mul_add(b, c)
}

fn to_matrix(m: Affine) -> Matrix {
    let [a, b, c, d, e, f] = m.as_coeffs();
    [a as f32, c as f32, e as f32, b as f32, d as f32, f as f32]
}

/// `SkMatrix::setConcat(a, b)` de afins: os produtos somam em double e o resultado volta a binário32.
fn concat(a: Matrix, b: Matrix) -> Matrix {
    let d = f64::from;
    let dot = |p: f32, q: f32, r: f32, s: f32| (d(p) * d(q) + d(r) * d(s)) as f32;
    [
        dot(a[0], b[0], a[1], b[3]),
        dot(a[0], b[1], a[1], b[4]),
        (d(a[0]) * d(b[2]) + d(a[1]) * d(b[5]) + d(a[2])) as f32,
        dot(a[3], b[0], a[4], b[3]),
        dot(a[3], b[1], a[4], b[4]),
        (d(a[3]) * d(b[2]) + d(a[4]) * d(b[5]) + d(a[5])) as f32,
    ]
}

/// `SkMatrix::invert` de uma afim, com o determinante e os termos em double.
fn invert(m: Matrix) -> Option<Matrix> {
    let [sx, kx, tx, ky, sy, ty] = m.map(f64::from);
    let det = sx * sy - kx * ky;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let inv = 1.0 / det;
    Some([(sy * inv) as f32, (-kx * inv) as f32, ((kx * ty - sy * tx) * inv) as f32, (-ky * inv) as f32, (sx * inv) as f32, ((ky * tx - sx * ty) * inv) as f32])
}

/// `SkMatrix::setSinCos(sin, cos, px, py)`: a rotação em torno do pivô.
fn sin_cos(s: f32, c: f32, px: f32, py: f32) -> Matrix {
    let one_minus_cos = 1.0 - c;
    [c, -s, s * py + one_minus_cos * px, s, c, -s * px + one_minus_cos * py]
}

/// `pts_to_unit_matrix` do `SkLinearGradient`: leva `p0` a `t = 0` e `p1` a `t = 1` no eixo x.
fn pts_to_unit(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Matrix> {
    let (vx, vy) = (x1 - x0, y1 - y0);
    let mag = (vx * vx + vy * vy).sqrt();
    if !(mag > 0.0) || !mag.is_finite() {
        return None;
    }
    let inv = 1.0 / mag;
    let (vx, vy) = (vx * inv, vy * inv);
    let mut m = sin_cos(-vy, vx, x0, y0);
    m[2] -= x0;
    m[5] -= y0;
    Some(m.map(|v| v * inv))
}

/// `SkMatrix::setRotate(graus, px, py)`, com o seno e o cosseno que encostam em zero (`SkScalarSinSnapToZero`).
fn rotate_about(degrees: f32, px: f32, py: f32) -> Matrix {
    let radians = degrees * (std::f32::consts::PI / 180.0);
    let snap = |v: f32| if v.abs() <= 1.0 / 4096.0 { 0.0 } else { v };
    sin_cos(snap(radians.sin()), snap(radians.cos()), px, py)
}

/// `xy_to_unit_angle`: o ângulo em voltas, pelo polinômio de grau 7 do Skia no lugar do `atan2`.
fn unit_angle(x: f32, y: f32) -> f32 {
    let (xa, ya) = (x.abs(), y.abs());
    let slope = xa.min(ya) / xa.max(ya);
    let s = slope * slope;
    let p = mad(s, -7.054_738_234_728_574_752_807_617_187_5e-3, 2.476_101_927_459_239_959_716_796_875e-2);
    let p = mad(s, p, -5.185_396_969_318_389_892_578_125e-2);
    let p = mad(s, p, 0.159_121_170_639_991_760_253_906_25);
    let mut phi = slope * p;
    if xa < ya {
        phi = 0.25 - phi;
    }
    if x < 0.0 {
        phi = 0.5 - phi;
    }
    if y < 0.0 {
        phi = 1.0 - phi;
    }
    if phi.is_nan() { 0.0 } else { phi }
}

/// Como o ponto mapeado vira `t`.
enum Kind {
    Linear,
    /// `xy_to_radius` e, com raio inicial, a escala e o deslocamento do "tiny twist" do radial concêntrico.
    Radial(Option<(f32, f32)>),
    Sweep,
}

/// Um gradiente pronto para pintar: a matriz total (do dispositivo ao espaço do gradiente), as paradas em cor não
/// premultiplicada e o alfa da pintura (`globalAlpha`).
pub struct SkiaGradient {
    kind: Kind,
    total: Matrix,
    stops: Vec<(f32, [f32; 4])>,
    alpha: f32,
}

impl SkiaGradient {
    /// O gradiente do prelúdio (['linear', x0, y0, x1, y1, paradas], ['radial', ...] ou ['conic', ângulo, x, y,
    /// paradas]) sob a transformação `ctm`. `stops` = (posição, [r, g, b, a] de 0 a 1), já em ordem. `None` para o
    /// radial de dois centros, que o Skia resolve por outro caminho (`SkTwoPointConicalGradient` focal e strip).
    pub fn new(kind: &str, points: &[f64], stops: Vec<(f32, [f32; 4])>, ctm: Affine, alpha: f64) -> Option<SkiaGradient> {
        let p = |i: usize| points.get(i).copied().unwrap_or(0.0) as f32;
        let (kind, gradient, local) = match kind {
            "linear" => (Kind::Linear, pts_to_unit(p(0), p(1), p(2), p(3))?, None),
            "radial" => {
                let (cx0, cy0, r0, cx1, cy1, r1) = (p(0), p(1), p(2), p(3), p(4), p(5));
                if cx0 != cx1 || cy0 != cy1 {
                    return None;
                }
                let radius = r0.max(r1);
                let scale = 1.0 / radius;
                let twist = (r0 != 0.0).then(|| (radius / (r1 - r0), -r0 / (r1 - r0)));
                (Kind::Radial(twist), [scale, 0.0, -cx1 * scale, 0.0, scale, -cy1 * scale], None)
            }
            "conic" => {
                // O Blink guarda a rotação do canvas em graus somando 90 (em double, com o resultado em binário32) e
                // tira os 90 ao montar o sweep do Skia, que começa no eixo x.
                let rotation = (points.first().copied().unwrap_or(0.0).to_degrees() + 90.0) as f32;
                let (cx, cy) = (p(1), p(2));
                (Kind::Sweep, [1.0, 0.0, -cx, 0.0, 1.0, -cy], Some(rotate_about(rotation - 90.0, cx, cy)))
            }
            _ => return None,
        };
        let ctm = to_matrix(ctm);
        let device = match local {
            Some(local) => concat(ctm, local),
            None => ctm,
        };
        let total = concat(gradient, invert(device)?);
        Some(SkiaGradient { kind, total, stops: with_end_stops(stops), alpha: alpha as f32 })
    }

    /// A cor não premultiplicada em `t`: dois extremos em 0 e 1 vão pelo `evenly_spaced_2_stop_gradient`
    /// (`t*(c1 - c0) + c0`); o resto pelo `gradient`, com `f` e `b` por intervalo.
    fn color(&self, t: f32) -> [f32; 4] {
        let t = t.clamp(0.0, 1.0);
        if let [(0.0, c0), (1.0, c1)] = self.stops[..] {
            return std::array::from_fn(|k| mad(t, c1[k] - c0[k], c0[k]));
        }
        let mut chosen = self.stops[0].1;
        for pair in self.stops.windows(2) {
            let ((tl, cl), (tr, cr)) = (pair[0], pair[1]);
            if tl < tr && t >= tl {
                let f: [f32; 4] = std::array::from_fn(|k| (cr[k] - cl[k]) / (tr - tl));
                chosen = std::array::from_fn(|k| mad(t, f[k], cl[k] - f[k] * tl));
            }
        }
        chosen
    }

    /// Os pixels premultiplicados do retângulo do dispositivo (`x`, `y`, `w`, `h`), como o Skia os grava.
    pub fn rasterize(&self, x: u32, y: u32, w: u16, h: u16) -> Pixmap {
        let mut data = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
        let m = self.total;
        for row in y..y + u32::from(h) {
            for col in x..x + u32::from(w) {
                let (px, py) = (col as f32 + 0.5, row as f32 + 0.5);
                let u = mad(px, m[0], mad(py, m[1], m[2]));
                let v = mad(px, m[3], mad(py, m[4], m[5]));
                let t = match self.kind {
                    Kind::Linear => u,
                    Kind::Radial(twist) => {
                        let r = (u * u + v * v).sqrt();
                        twist.map_or(r, |(scale, bias)| mad(r, scale, bias))
                    }
                    Kind::Sweep => unit_angle(u, v),
                };
                let c = self.color(t);
                let mut p = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
                if self.alpha != 1.0 {
                    p = p.map(|v| v * self.alpha);
                }
                let d = dither(col, row);
                for k in 0..3 {
                    p[k] = mad(d, 1.0 / 255.0, p[k]).max(0.0).min(p[3]);
                }
                data.extend(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8));
            }
        }
        Pixmap::from_parts(data, w, h, PixelMetadata { may_have_transparency: true, alpha_type: ImageAlphaType::AlphaPremultiplied })
    }
}

/// As posições como o `SkGradientBaseShader` as guarda: sem parada em 0 ou em 1, a primeira e a última cor se
/// repetem nas pontas.
fn with_end_stops(mut stops: Vec<(f32, [f32; 4])>) -> Vec<(f32, [f32; 4])> {
    if stops[0].0 > 0.0 {
        stops.insert(0, (0.0, stops[0].1));
    }
    let last = stops[stops.len() - 1];
    if last.0 < 1.0 {
        stops.push((1.0, last.1));
    }
    stops
}

/// O valor da matriz de dither 8x8 do `SkRasterPipeline` em (`x`, `y`), em (-0,5, +0,5).
fn dither(x: u32, y: u32) -> f32 {
    let (x, y) = (x, y ^ x);
    let m = (y & 1) << 5 | (x & 1) << 4 | (y & 2) << 2 | (x & 2) << 1 | (y & 4) >> 1 | (x & 4) >> 2;
    mad(m as f32, 2.0 / 128.0, -63.0 / 128.0)
}
