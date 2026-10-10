// ---- Canvas 2D: CanvasRenderingContext2D, Path2D, gradientes, padrões, ImageData, TextMetrics, DOMMatrix e
// DOMPoint, ImageBitmap. O estado e a validação seguem o Blink (padrões, serialização de cor e de fonte medidas
// no oráculo, `CANVAS2D_SURFACE`). Os pixels: retângulos alinhados, `clearRect`, `putImageData` e `drawImage`
// de imagem decodificada vão ao bitmap; caminhos e texto ainda não são rasterizados.

const illegalConstructor = (name) => new TypeError(`Failed to construct '${name}': Illegal constructor`);

// ---- Cores do CSS para fillStyle, strokeStyle e shadowColor.
const CSS_NAMED_COLORS = 'aliceblue f0f8ff antiquewhite faebd7 aqua 00ffff aquamarine 7fffd4 azure f0ffff beige f5f5dc bisque ffe4c4 black 000000 blanchedalmond ffebcd blue 0000ff blueviolet 8a2be2 brown a52a2a burlywood deb887 cadetblue 5f9ea0 chartreuse 7fff00 chocolate d2691e coral ff7f50 cornflowerblue 6495ed cornsilk fff8dc crimson dc143c cyan 00ffff darkblue 00008b darkcyan 008b8b darkgoldenrod b8860b darkgray a9a9a9 darkgreen 006400 darkgrey a9a9a9 darkkhaki bdb76b darkmagenta 8b008b darkolivegreen 556b2f darkorange ff8c00 darkorchid 9932cc darkred 8b0000 darksalmon e9967a darkseagreen 8fbc8f darkslateblue 483d8b darkslategray 2f4f4f darkslategrey 2f4f4f darkturquoise 00ced1 darkviolet 9400d3 deeppink ff1493 deepskyblue 00bfff dimgray 696969 dimgrey 696969 dodgerblue 1e90ff firebrick b22222 floralwhite fffaf0 forestgreen 228b22 fuchsia ff00ff gainsboro dcdcdc ghostwhite f8f8ff gold ffd700 goldenrod daa520 gray 808080 green 008000 greenyellow adff2f grey 808080 honeydew f0fff0 hotpink ff69b4 indianred cd5c5c indigo 4b0082 ivory fffff0 khaki f0e68c lavender e6e6fa lavenderblush fff0f5 lawngreen 7cfc00 lemonchiffon fffacd lightblue add8e6 lightcoral f08080 lightcyan e0ffff lightgoldenrodyellow fafad2 lightgray d3d3d3 lightgreen 90ee90 lightgrey d3d3d3 lightpink ffb6c1 lightsalmon ffa07a lightseagreen 20b2aa lightskyblue 87cefa lightslategray 778899 lightslategrey 778899 lightsteelblue b0c4de lightyellow ffffe0 lime 00ff00 limegreen 32cd32 linen faf0e6 magenta ff00ff maroon 800000 mediumaquamarine 66cdaa mediumblue 0000cd mediumorchid ba55d3 mediumpurple 9370db mediumseagreen 3cb371 mediumslateblue 7b68ee mediumspringgreen 00fa9a mediumturquoise 48d1cc mediumvioletred c71585 midnightblue 191970 mintcream f5fffa mistyrose ffe4e1 moccasin ffe4b5 navajowhite ffdead navy 000080 oldlace fdf5e6 olive 808000 olivedrab 6b8e23 orange ffa500 orangered ff4500 orchid da70d6 palegoldenrod eee8aa palegreen 98fb98 paleturquoise afeeee palevioletred db7093 papayawhip ffefd5 peachpuff ffdab9 peru cd853f pink ffc0cb plum dda0dd powderblue b0e0e6 purple 800080 rebeccapurple 663399 red ff0000 rosybrown bc8f8f royalblue 4169e1 saddlebrown 8b4513 salmon fa8072 sandybrown f4a460 seagreen 2e8b57 seashell fff5ee sienna a0522d silver c0c0c0 skyblue 87ceeb slateblue 6a5acd slategray 708090 slategrey 708090 snow fffafa springgreen 00ff7f steelblue 4682b4 tan d2b48c teal 008080 thistle d8bfd8 tomato ff6347 turquoise 40e0d0 violet ee82ee wheat f5deb3 white ffffff whitesmoke f5f5f5 yellow ffff00 yellowgreen 9acd32'.split(' ');
const namedColors = new Map();
for (let i = 0; i < CSS_NAMED_COLORS.length; i += 2) namedColors.set(CSS_NAMED_COLORS[i], CSS_NAMED_COLORS[i + 1]);
// [r, g, b, a] com r, g, b em 0..255 e a em 0..1, ou null.
const parseCssColor = (text) => {
  const value = String(text).trim().toLowerCase();
  if (value === 'transparent') return [0, 0, 0, 0];
  if (value === 'currentcolor') return [0, 0, 0, 1];
  const hex = value.startsWith('#') ? value.slice(1) : namedColors.get(value);
  if (hex !== undefined) {
    if (!/^[0-9a-f]+$/.test(hex) || ![3, 4, 6, 8].includes(hex.length)) return null;
    const full = hex.length <= 4 ? hex.split('').map((c) => c + c).join('') : hex;
    const bytes = full.match(/../g).map((h) => parseInt(h, 16));
    return [bytes[0], bytes[1], bytes[2], bytes.length === 4 ? bytes[3] / 255 : 1];
  }
  const fn = /^(rgba?|hsla?|hwb)\((.*)\)$/.exec(value);
  if (!fn) return null;
  const legacy = fn[2].includes(',');
  const parts = legacy ? fn[2].split(',').map((s) => s.trim()) : fn[2].replace('/', ' / ').trim().split(/\s+/);
  let alphaText = null;
  if (!legacy) { const slash = parts.indexOf('/'); if (slash >= 0) { alphaText = parts[slash + 1]; parts.splice(slash); } }
  else if (parts.length === 4) alphaText = parts.pop();
  if (parts.length !== 3) return null;
  const num = (s, scale) => { const m = /^([-+]?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?)(%)?$/.exec(s); if (!m) return null; return m[2] ? (Number(m[1]) / 100) * scale : Number(m[1]); };
  let alpha = 1;
  if (alphaText !== null && alphaText !== undefined) { const a = num(alphaText, 1); if (a === null) return null; alpha = Math.min(1, Math.max(0, a)); }
  if (fn[1].startsWith('rgb')) {
    const rgb = parts.map((p) => num(p, 255));
    if (rgb.includes(null)) return null;
    return [...rgb.map((v) => Math.round(Math.min(255, Math.max(0, v)))), alpha];
  }
  const hue = /^([-+]?[\d.]+)(deg|rad|grad|turn)?$/.exec(parts[0]);
  if (!hue) return null;
  const h = ((({ deg: 1, rad: 180 / Math.PI, grad: 0.9, turn: 360 })[hue[2] ?? 'deg'] * Number(hue[1])) % 360 + 360) % 360;
  const p1 = num(parts[1], 1), p2 = num(parts[2], 1);
  if (p1 === null || p2 === null || !parts[1].endsWith('%') && legacy || !parts[2].endsWith('%') && legacy) return null;
  let rgb;
  if (fn[1].startsWith('hsl')) {
    const s = Math.min(1, Math.max(0, p1)), l = Math.min(1, Math.max(0, p2));
    const f = (n) => { const k = (n + h / 30) % 12; const a = s * Math.min(l, 1 - l); return l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)); };
    rgb = [f(0), f(8), f(4)];
  } else {
    let [w, b] = [p1, p2];
    if (w + b >= 1) { const gray = w / (w + b); rgb = [gray, gray, gray]; }
    else { const f = (n) => { const k = (n + h / 30) % 12; return 0.5 - 0.5 * Math.max(-1, Math.min(k - 3, 9 - k, 1)); }; rgb = [f(0), f(8), f(4)].map((c) => c * (1 - w - b) + w); }
  }
  return [...rgb.map((c) => Math.round(c * 255)), alpha];
};
// A serialização do canvas: opaco em `#rrggbb`, com alfa em `rgba(r, g, b, a)` com o alfa de 8 bits.
const serializeCanvasColor = ([r, g, b, a]) => {
  const alphaByte = Math.round(a * 255);
  if (alphaByte === 255) return '#' + [r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('');
  const alpha = alphaByte === 0 ? '0' : String(Math.round((alphaByte / 255) * 100) / 100);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
};

// ---- Fonte do canvas: o `font` do CSS, serializado como o Blink.
const CANVAS_FONT_STYLES = new Set(['italic', 'oblique']);
const CANVAS_FONT_VARIANTS = new Set(['small-caps']);
const CANVAS_FONT_WEIGHTS = new Set(['bold', 'bolder', 'lighter']);
const CANVAS_FONT_STRETCHES = new Set(['ultra-condensed', 'extra-condensed', 'condensed', 'semi-condensed', 'semi-expanded', 'expanded', 'extra-expanded', 'ultra-expanded']);
const ABSOLUTE_SIZES = { 'xx-small': 9, 'x-small': 10, small: 13, medium: 16, large: 18, 'x-large': 24, 'xx-large': 32, 'xxx-large': 48 };
const parseCanvasFont = (text) => {
  const source = String(text).trim();
  const m = /^((?:(?:normal|italic|oblique|small-caps|bold|bolder|lighter|[1-9]00|ultra-condensed|extra-condensed|condensed|semi-condensed|semi-expanded|expanded|extra-expanded|ultra-expanded)\s+)*)((?:[\d.]+(?:px|pt|em|rem|%|pc|in|cm|mm|q|ex|ch|vw|vh))|xx-small|x-small|small|medium|large|x-large|xx-large|xxx-large)(?:\s*\/\s*(?:normal|[\d.]+(?:px|em|%)?))?\s+(.+)$/i.exec(source);
  if (!m) return null;
  let style = 'normal', variant = 'normal', weight = 'normal', stretch = 'normal';
  for (const word of m[1].trim().split(/\s+/).filter(Boolean)) {
    const lowered = word.toLowerCase();
    if (CANVAS_FONT_STYLES.has(lowered)) style = lowered;
    else if (CANVAS_FONT_VARIANTS.has(lowered)) variant = lowered;
    else if (CANVAS_FONT_WEIGHTS.has(lowered) || /^[1-9]00$/.test(lowered)) weight = lowered;
    else if (CANVAS_FONT_STRETCHES.has(lowered)) stretch = lowered;
  }
  const sizeText = m[2].toLowerCase();
  let size;
  if (sizeText in ABSOLUTE_SIZES) size = ABSOLUTE_SIZES[sizeText];
  else {
    const unit = /[a-z%]+$/.exec(sizeText)[0];
    const n = parseFloat(sizeText);
    // `em` e `%` são relativos à fonte do elemento canvas; fora do documento, 10px (o padrão do canvas).
    size = { px: n, pt: (n * 4) / 3, pc: n * 16, in: n * 96, cm: (n * 96) / 2.54, mm: (n * 96) / 25.4, q: (n * 96) / 101.6, em: n * 10, rem: n * 16, '%': n / 10, ex: n * 5, ch: n * 5, vw: (n * VIEWPORT.width) / 100, vh: (n * VIEWPORT.height) / 100 }[unit];
  }
  const family = m[3].trim();
  if (!/^(?:(?:"[^"]*"|'[^']*'|[A-Za-z_-][\w -]*)\s*,\s*)*(?:"[^"]*"|'[^']*'|[A-Za-z_-][\w -]*)$/.test(family)) return null;
  const parts = [];
  if (style !== 'normal') parts.push(style);
  if (weight !== 'normal') parts.push(weight === 'bold' ? 'bold' : weight);
  if (variant !== 'normal') parts.push(variant);
  if (stretch !== 'normal') parts.push(stretch);
  parts.push(`${Number(size.toFixed(6))}px`, family.replace(/\s*,\s*/g, ', '));
  return { css: parts.join(' '), style, variant, weight, stretch, size, family };
};

// ---- Path2D: os comandos guardados como no Skia (pontos já transformados quando vêm de addPath).
const pathState = (path) => {
  const state = path?.[P.__path];
  if (!state) throw new TypeError('Illegal invocation');
  return state;
};
const pathFinite = (...values) => values.every((v) => Number.isFinite(v));
const PathMethods = {
  closePath() { const s = pathState(this); if (s.ops.length) { s.ops.push(['Z']); s.current = s.start; } },
  moveTo(x, y) { [x, y] = [Number(x), Number(y)]; if (!pathFinite(x, y)) return; const s = pathState(this); s.ops.push(['M', x, y]); s.current = s.start = [x, y]; },
  lineTo(x, y) { [x, y] = [Number(x), Number(y)]; if (!pathFinite(x, y)) return; const s = pathState(this); if (!s.current) { s.ops.push(['M', x, y]); s.start = [x, y]; } else s.ops.push(['L', x, y]); s.current = [x, y]; },
  quadraticCurveTo(cx, cy, x, y) { const v = [cx, cy, x, y].map(Number); if (!pathFinite(...v)) return; const s = pathState(this); if (!s.current) { s.ops.push(['M', v[0], v[1]]); s.start = [v[0], v[1]]; } s.ops.push(['Q', ...v]); s.current = [v[2], v[3]]; },
  bezierCurveTo(c1x, c1y, c2x, c2y, x, y) { const v = [c1x, c1y, c2x, c2y, x, y].map(Number); if (!pathFinite(...v)) return; const s = pathState(this); if (!s.current) { s.ops.push(['M', v[0], v[1]]); s.start = [v[0], v[1]]; } s.ops.push(['C', ...v]); s.current = [v[4], v[5]]; },
  arcTo(x1, y1, x2, y2, radius) {
    const v = [x1, y1, x2, y2, radius].map(Number);
    if (!pathFinite(...v)) return;
    if (v[4] < 0) throw domException(`Failed to execute 'arcTo' on '${this instanceof Path2D ? 'Path2D' : 'CanvasRenderingContext2D'}': The radius provided (${v[4]}) is negative.`, 'IndexSizeError');
    const s = pathState(this);
    if (!s.current) { s.ops.push(['M', v[0], v[1]]); s.current = s.start = [v[0], v[1]]; }
    s.ops.push(['T', ...v]);
    s.current = [v[0], v[1]];
  },
  arc(x, y, radius, start, end, ccw = false) { PathMethods.ellipse.call(this, x, y, radius, radius, 0, start, end, ccw, 'arc'); },
  ellipse(x, y, rx, ry, rotation, start, end, ccw = false, method = 'ellipse') {
    const v = [x, y, rx, ry, rotation, start, end].map(Number);
    if (!pathFinite(...v)) return;
    if (v[2] < 0 || v[3] < 0) throw domException(`Failed to execute '${method}' on '${this instanceof Path2D ? 'Path2D' : 'CanvasRenderingContext2D'}': The ${method === 'arc' ? 'radius' : v[2] < 0 ? 'major-axis radius' : 'minor-axis radius'} provided (${v[2] < 0 ? v[2] : v[3]}) is negative.`, 'IndexSizeError');
    const s = pathState(this);
    const sx = v[0] + v[2] * Math.cos(v[5]) * Math.cos(v[4]) - v[3] * Math.sin(v[5]) * Math.sin(v[4]);
    const sy = v[1] + v[2] * Math.cos(v[5]) * Math.sin(v[4]) + v[3] * Math.sin(v[5]) * Math.cos(v[4]);
    if (!s.current) { s.ops.push(['M', sx, sy]); s.start = [sx, sy]; } else s.ops.push(['L', sx, sy]);
    s.ops.push(['A', ...v, Boolean(ccw)]);
    s.current = [v[0] + v[2] * Math.cos(v[6]) * Math.cos(v[4]) - v[3] * Math.sin(v[6]) * Math.sin(v[4]), v[1] + v[2] * Math.cos(v[6]) * Math.sin(v[4]) + v[3] * Math.sin(v[6]) * Math.cos(v[4])];
  },
  rect(x, y, w, h) { const v = [x, y, w, h].map(Number); if (!pathFinite(...v)) return; const s = pathState(this); s.ops.push(['M', v[0], v[1]], ['L', v[0] + v[2], v[1]], ['L', v[0] + v[2], v[1] + v[3]], ['L', v[0], v[1] + v[3]], ['Z']); s.current = s.start = [v[0], v[1]]; },
  roundRect(x, y, w, h, radii = 0) {
    const v = [x, y, w, h].map(Number);
    const list = typeof radii === 'object' && radii !== null && typeof radii[Symbol.iterator] === 'function' ? Array.from(radii) : [radii];
    const owner = this instanceof Path2D ? 'Path2D' : 'CanvasRenderingContext2D';
    if (list.length < 1 || list.length > 4) throw new RangeError(`Failed to execute 'roundRect' on '${owner}': ${list.length} radii provided. Between one and four radii are necessary.`);
    for (const r of list) {
      const value = typeof r === 'object' && r !== null ? [Number(r.x ?? 0), Number(r.y ?? 0)] : [Number(r)];
      if (value.some((n) => n < 0)) throw new RangeError(`Failed to execute 'roundRect' on '${owner}': Radius value ${value.find((n) => n < 0)} is negative.`);
    }
    if (!pathFinite(...v)) return;
    const s = pathState(this);
    s.ops.push(['R', ...v, list]);
    s.current = s.start = [v[0], v[1]];
  },
};
class Path2D {
  constructor(source) {
    const state = { ops: [], current: null, start: null };
    hidden(this, '__path', state);
    if (source instanceof Path2D) { const other = pathState(source); state.ops = other.ops.slice(); state.current = other.current; state.start = other.start; }
    else if (source !== undefined) state.ops = parseSvgPath(String(source));
  }
  addPath(path, transform) {
    const s = pathState(this);
    const other = pathState(path);
    const t = transform === undefined ? null : matrixFromInit(transform).v;
    s.ops.push(['P', other.ops.slice(), t ? [t[0], t[1], t[4], t[5], t[12], t[13]] : null]);
    if (other.current) s.current = other.current;
  }
}
for (const [name, fn] of Object.entries(PathMethods)) {
  const length = { closePath: 0, moveTo: 2, lineTo: 2, quadraticCurveTo: 4, bezierCurveTo: 6, arcTo: 5, arc: 5, ellipse: 7, rect: 4, roundRect: 4 }[name];
  const method = { [name](...args) { return fn.apply(this, args.length > (name === 'ellipse' ? 8 : 99) ? args.slice(0, 8) : args); } }[name];
  Object.defineProperty(method, 'length', { value: length });
  Object.defineProperty(Path2D.prototype, name, { value: method, writable: true, enumerable: true, configurable: true });
}
tag(Path2D, 'Path2D');
// O `d` do SVG para o construtor de Path2D: o que é válido até o primeiro erro.
const parseSvgPath = (d) => {
  const ops = [];
  const tokens = d.match(/[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?/g) ?? [];
  let i = 0, x = 0, y = 0, sx = 0, sy = 0, command = null, lastControl = null;
  const nums = (n) => { const out = tokens.slice(i, i + n).map(Number); if (out.length < n || out.some(Number.isNaN)) throw new Error('fim'); i += n; return out; };
  try {
    while (i < tokens.length) {
      if (/[A-Za-z]/.test(tokens[i])) command = tokens[i++];
      else if (command === null) break;
      const rel = command === command.toLowerCase();
      const ox = rel ? x : 0, oy = rel ? y : 0;
      switch (command.toUpperCase()) {
        case 'M': { const [a, b] = nums(2); x = ox + a; y = oy + b; sx = x; sy = y; ops.push(['M', x, y]); command = rel ? 'l' : 'L'; break; }
        case 'L': { const [a, b] = nums(2); x = ox + a; y = oy + b; ops.push(['L', x, y]); break; }
        case 'H': { const [a] = nums(1); x = ox + a; ops.push(['L', x, y]); break; }
        case 'V': { const [a] = nums(1); y = oy + a; ops.push(['L', x, y]); break; }
        case 'C': { const v = nums(6); const p = [ox + v[0], oy + v[1], ox + v[2], oy + v[3], ox + v[4], oy + v[5]]; ops.push(['C', ...p]); lastControl = [p[2], p[3]]; [x, y] = [p[4], p[5]]; continue; }
        case 'S': { const v = nums(4); const c1 = lastControl ? [2 * x - lastControl[0], 2 * y - lastControl[1]] : [x, y]; const p = [...c1, ox + v[0], oy + v[1], ox + v[2], oy + v[3]]; ops.push(['C', ...p]); lastControl = [p[2], p[3]]; [x, y] = [p[4], p[5]]; continue; }
        case 'Q': { const v = nums(4); const p = [ox + v[0], oy + v[1], ox + v[2], oy + v[3]]; ops.push(['Q', ...p]); lastControl = [p[0], p[1]]; [x, y] = [p[2], p[3]]; continue; }
        case 'T': { const v = nums(2); const c = lastControl ? [2 * x - lastControl[0], 2 * y - lastControl[1]] : [x, y]; ops.push(['Q', ...c, ox + v[0], oy + v[1]]); lastControl = c; [x, y] = [ox + v[0], oy + v[1]]; continue; }
        case 'A': { const v = nums(7); ops.push(['SA', x, y, ...v.slice(0, 5), ox + v[5], oy + v[6]]); [x, y] = [ox + v[5], oy + v[6]]; break; }
        case 'Z': ops.push(['Z']); [x, y] = [sx, sy]; break;
        default: throw new Error('comando');
      }
      lastControl = null;
    }
  } catch { /* o resto do caminho é ignorado, como no parser de SVG */ }
  return ops;
};

// ---- Gradientes, padrões, ImageData, TextMetrics.
class CanvasGradient {
  constructor() { throw illegalConstructor('CanvasGradient'); }
  addColorStop(offset, color) {
    const state = this[P.__gradient];
    if (!state) throw new TypeError('Illegal invocation');
    const o = Number(offset);
    if (!Number.isFinite(o) || o < 0 || o > 1) throw domException(`Failed to execute 'addColorStop' on 'CanvasGradient': The provided value (${offset}) is outside the range (0.0, 1.0).`, 'IndexSizeError');
    const parsed = parseCssColor(color);
    if (!parsed) throw domException(`Failed to execute 'addColorStop' on 'CanvasGradient': The value provided ('${color}') could not be parsed as a color.`, 'SyntaxError');
    state.stops.push([o, parsed]);
  }
}
tag(CanvasGradient, 'CanvasGradient');
class CanvasPattern {
  constructor() { throw illegalConstructor('CanvasPattern'); }
  setTransform(transform) { const state = this[P.__pattern]; if (!state) throw new TypeError('Illegal invocation'); state.transform = matrixFromInit(transform).v; }
}
tag(CanvasPattern, 'CanvasPattern');
const imageDataState = new WeakMap();
class ImageData {
  constructor(a, b, c, d) {
    let width, height, data;
    if (ArrayBuffer.isView(a)) {
      if (!(a instanceof Uint8ClampedArray)) throw new TypeError("Failed to construct 'ImageData': Overload resolution failed.");
      data = a;
      if (data.length === 0 || data.length % 4 !== 0) throw domException("Failed to construct 'ImageData': The input data length is not a multiple of 4.", 'InvalidStateError');
      width = Number(b) >>> 0;
      if (width === 0) throw domException("Failed to construct 'ImageData': The source width is zero or not a number.", 'IndexSizeError');
      const pixels = data.length / 4;
      if (pixels % width !== 0) throw domException("Failed to construct 'ImageData': The input data length is not a multiple of (4 * width).", 'IndexSizeError');
      height = c === undefined ? pixels / width : Number(c) >>> 0;
      if (height * width !== pixels) throw domException("Failed to construct 'ImageData': The input data length is not equal to (4 * width * height).", 'IndexSizeError');
      void d;
    } else {
      if (arguments.length < 2) throw new TypeError(`Failed to construct 'ImageData': 2 arguments required, but only ${arguments.length} present.`);
      width = Number(a) >>> 0;
      height = Number(b) >>> 0;
      if (width === 0) throw domException("Failed to construct 'ImageData': The source width is zero or not a number.", 'IndexSizeError');
      if (height === 0) throw domException("Failed to construct 'ImageData': The source height is zero or not a number.", 'IndexSizeError');
      data = new Uint8ClampedArray(width * height * 4);
    }
    imageDataState.set(this, { width, height, data, colorSpace: 'srgb' });
  }
  get width() { return imageDataOf(this).width; }
  get height() { return imageDataOf(this).height; }
  get colorSpace() { return imageDataOf(this).colorSpace; }
  get data() { return imageDataOf(this).data; }
  get pixelFormat() { return 'rgba-unorm8'; }
}
const imageDataOf = (value) => { const s = imageDataState.get(value); if (!s) throw new TypeError('Illegal invocation'); return s; };
tag(ImageData, 'ImageData');
const TEXT_METRIC_FIELDS = ['width', 'actualBoundingBoxLeft', 'actualBoundingBoxRight', 'fontBoundingBoxAscent', 'fontBoundingBoxDescent', 'actualBoundingBoxAscent', 'actualBoundingBoxDescent', 'hangingBaseline', 'alphabeticBaseline', 'ideographicBaseline'];
class TextMetrics { constructor() { throw illegalConstructor('TextMetrics'); } }
for (const field of TEXT_METRIC_FIELDS) Object.defineProperty(TextMetrics.prototype, field, { get() { const s = this[P.__metrics]; if (!s) throw new TypeError('Illegal invocation'); return s[field]; }, enumerable: true, configurable: true });
tag(TextMetrics, 'TextMetrics');

// ---- ImageBitmap e createImageBitmap.
const imageBitmaps = new WeakMap();
class ImageBitmap {
  constructor() { throw illegalConstructor('ImageBitmap'); }
  get width() { return imageBitmapState(this).closed ? 0 : imageBitmapState(this).width; }
  get height() { return imageBitmapState(this).closed ? 0 : imageBitmapState(this).height; }
  close() { const s = imageBitmapState(this); s.closed = true; s.pixels = null; }
}
const imageBitmapState = (bitmap) => { const s = imageBitmaps.get(bitmap); if (!s) throw new TypeError('Illegal invocation'); return s; };
tag(ImageBitmap, 'ImageBitmap');
const makeImageBitmap = (width, height, pixels) => { const bitmap = Object.create(ImageBitmap.prototype); imageBitmaps.set(bitmap, { width, height, pixels, closed: false }); return bitmap; };
const createImageBitmap = function createImageBitmap(source, ...rest) {
  if (arguments.length < 1) return Promise.reject(new TypeError("Failed to execute 'createImageBitmap' on 'Window': 1 argument required, but only 0 present."));
  const crop = rest.length >= 4 ? rest.slice(0, 4).map(Number) : null;
  const finish = (width, height, pixels) => {
    if (!width || !height) throw domException("Failed to execute 'createImageBitmap' on 'Window': The source image width is 0.", 'InvalidStateError');
    if (!crop) return makeImageBitmap(width, height, pixels);
    const [sx, sy, sw, sh] = crop;
    const out = new Uint8Array(Math.abs(sw) * Math.abs(sh) * 4);
    if (pixels) for (let y = 0; y < Math.abs(sh); y++) for (let x = 0; x < Math.abs(sw); x++) {
      const fx = sx + x, fy = sy + y;
      if (fx < 0 || fy < 0 || fx >= width || fy >= height) continue;
      out.set(pixels.subarray((fy * width + fx) * 4, (fy * width + fx) * 4 + 4), (y * Math.abs(sw) + x) * 4);
    }
    return makeImageBitmap(Math.abs(sw), Math.abs(sh), out);
  };
  try {
    if (source instanceof Blob) {
      return source.arrayBuffer().then((buffer) => {
        const decoded = decodeImageBytes(new Uint8Array(buffer));
        if (!decoded) throw domException("Failed to execute 'createImageBitmap' on 'Window': The source image could not be decoded.", 'InvalidStateError');
        return finish(decoded.width, decoded.height, decoded.pixels);
      });
    }
    if (source instanceof HTMLImageElement) {
      const decoded = decodedImage(source);
      if (!decoded) return Promise.reject(domException("Failed to execute 'createImageBitmap' on 'Window': The image element contains no image data.", 'InvalidStateError'));
      return Promise.resolve(finish(decoded.width, decoded.height, decoded.pixels));
    }
    if (source instanceof HTMLCanvasElement) return Promise.resolve(finish(source.width, source.height, canvasPixels(source)));
    if (source instanceof ImageData) { const s = imageDataOf(source); return Promise.resolve(finish(s.width, s.height, new Uint8Array(s.data))); }
    if (source instanceof ImageBitmap) { const s = imageBitmapState(source); return Promise.resolve(finish(s.width, s.height, s.pixels)); }
  } catch (error) { return Promise.reject(error); }
  return Promise.reject(new TypeError("Failed to execute 'createImageBitmap' on 'Window': The provided value is not of type '(Blob or HTMLCanvasElement or HTMLImageElement or HTMLVideoElement or ImageBitmap or ImageData or OffscreenCanvas or SVGImageElement or VideoFrame)'."));
};

// ---- Imagens decodificadas: a ponte guarda os pixels e devolve [alça, largura, altura]; aqui só as dimensões.
const decodeImageBytes = (bytes) => {
  let latin1 = '';
  for (let i = 0; i < bytes.length; i += 8192) latin1 += String.fromCharCode.apply(null, bytes.subarray(i, i + 8192));
  const result = op('decodeImage', latin1);
  return result === null ? null : { width: result[1], height: result[2], pixels: null };
};
const decodedImage = (img) => { const image = img[P.__image]; return image ? { width: image[1], height: image[2], pixels: null } : null; };

// ---- O contexto 2D.
const CANVAS_COMPOSITE = new Set(['source-over', 'source-in', 'source-out', 'source-atop', 'destination-over', 'destination-in', 'destination-out', 'destination-atop', 'lighter', 'copy', 'xor', 'multiply', 'screen', 'overlay', 'darken', 'lighten', 'color-dodge', 'color-burn', 'hard-light', 'soft-light', 'difference', 'exclusion', 'hue', 'saturation', 'color', 'luminosity', 'plus-lighter']);
const CANVAS_ENUMS = {
  lineCap: ['butt', 'round', 'square'], lineJoin: ['round', 'bevel', 'miter'], textAlign: ['start', 'end', 'left', 'right', 'center'],
  textBaseline: ['top', 'hanging', 'middle', 'alphabetic', 'ideographic', 'bottom'], direction: ['ltr', 'rtl', 'inherit'], fontKerning: ['auto', 'normal', 'none'],
  fontStretch: ['ultra-condensed', 'extra-condensed', 'condensed', 'semi-condensed', 'normal', 'semi-expanded', 'expanded', 'extra-expanded', 'ultra-expanded'],
  fontVariantCaps: ['normal', 'small-caps', 'all-small-caps', 'petite-caps', 'all-petite-caps', 'unicase', 'titling-caps'], textRendering: ['auto', 'optimizeSpeed', 'optimizeLegibility', 'geometricPrecision'],
  imageSmoothingQuality: ['low', 'medium', 'high'],
};
const canvas2dDefaults = () => ({
  transform: [1, 0, 0, 1, 0, 0], fillStyle: [0, 0, 0, 1], strokeStyle: [0, 0, 0, 1], globalAlpha: 1, globalCompositeOperation: 'source-over',
  lineWidth: 1, lineCap: 'butt', lineJoin: 'miter', miterLimit: 10, lineDash: [], lineDashOffset: 0,
  shadowOffsetX: 0, shadowOffsetY: 0, shadowBlur: 0, shadowColor: [0, 0, 0, 0],
  font: parseCanvasFont('10px sans-serif'), textAlign: 'start', textBaseline: 'alphabetic', direction: 'ltr', letterSpacing: '0px', wordSpacing: '0px',
  fontKerning: 'auto', fontStretch: 'normal', fontVariantCaps: 'normal', textRendering: 'auto', lang: 'inherit',
  imageSmoothingEnabled: true, imageSmoothingQuality: 'low', filter: 'none', clip: null,
});
const canvas2dStates = new WeakMap();
const c2d = (ctx) => { const s = canvas2dStates.get(ctx); if (!s) throw new TypeError('Illegal invocation'); return s; };
const styleValue = (value) => (Array.isArray(value) ? serializeCanvasColor(value) : value);
class CanvasRenderingContext2D { constructor() { throw illegalConstructor('CanvasRenderingContext2D'); } }
tag(CanvasRenderingContext2D, 'CanvasRenderingContext2D');
const canvas2dProperties = {
  canvas: { get() { return c2d(this).canvas; } },
  globalAlpha: { get() { return c2d(this).state.globalAlpha; }, set(v) { const n = Number(v); if (Number.isFinite(n) && n >= 0 && n <= 1) c2d(this).state.globalAlpha = n; } },
  globalCompositeOperation: { get() { return c2d(this).state.globalCompositeOperation; }, set(v) { const t = String(v); if (CANVAS_COMPOSITE.has(t)) c2d(this).state.globalCompositeOperation = t; } },
  fillStyle: { get() { return styleValue(c2d(this).state.fillStyle); }, set(v) { const s = c2d(this); const parsed = canvasStyle(v); if (parsed !== null) s.state.fillStyle = parsed; } },
  strokeStyle: { get() { return styleValue(c2d(this).state.strokeStyle); }, set(v) { const s = c2d(this); const parsed = canvasStyle(v); if (parsed !== null) s.state.strokeStyle = parsed; } },
  shadowColor: { get() { return serializeCanvasColor(c2d(this).state.shadowColor); }, set(v) { const parsed = parseCssColor(v); if (parsed) c2d(this).state.shadowColor = parsed; } },
  font: { get() { return c2d(this).state.font.css; }, set(v) { const parsed = parseCanvasFont(v); if (parsed) c2d(this).state.font = parsed; } },
  letterSpacing: { get() { return c2d(this).state.letterSpacing; }, set(v) { const m = /^\s*(-?[\d.]+)(px|em|rem|pt)\s*$/i.exec(String(v)); if (m) c2d(this).state.letterSpacing = `${Number(m[1])}${m[2].toLowerCase()}`; } },
  wordSpacing: { get() { return c2d(this).state.wordSpacing; }, set(v) { const m = /^\s*(-?[\d.]+)(px|em|rem|pt)\s*$/i.exec(String(v)); if (m) c2d(this).state.wordSpacing = `${Number(m[1])}${m[2].toLowerCase()}`; } },
  filter: { get() { return c2d(this).state.filter; }, set(v) { c2d(this).state.filter = String(v); } },
  lang: { get() { return c2d(this).state.lang; }, set(v) { c2d(this).state.lang = String(v); } },
  imageSmoothingEnabled: { get() { return c2d(this).state.imageSmoothingEnabled; }, set(v) { c2d(this).state.imageSmoothingEnabled = Boolean(v); } },
  lineDashOffset: { get() { return c2d(this).state.lineDashOffset; }, set(v) { const n = Number(v); if (Number.isFinite(n)) c2d(this).state.lineDashOffset = n; } },
};
for (const key of ['lineWidth', 'miterLimit']) canvas2dProperties[key] = { get() { return c2d(this).state[key]; }, set(v) { const n = Number(v); if (Number.isFinite(n) && n > 0) c2d(this).state[key] = n; } };
for (const key of ['shadowOffsetX', 'shadowOffsetY']) canvas2dProperties[key] = { get() { return c2d(this).state[key]; }, set(v) { const n = Number(v); if (Number.isFinite(n)) c2d(this).state[key] = n; } };
canvas2dProperties.shadowBlur = { get() { return c2d(this).state.shadowBlur; }, set(v) { const n = Number(v); if (Number.isFinite(n) && n >= 0) c2d(this).state.shadowBlur = n; } };
for (const [key, values] of Object.entries(CANVAS_ENUMS)) canvas2dProperties[key] = { get() { return c2d(this).state[key]; }, set(v) { const t = String(v); if (values.includes(t)) c2d(this).state[key] = t; } };
const canvasStyle = (value) => {
  if (value instanceof CanvasGradient || value instanceof CanvasPattern) return value;
  return parseCssColor(value);
};
const canvasTransformPoint = (t, x, y) => [t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5]];
// Composição source-over de uma cor sólida com cobertura, em 8 bits pré-multiplicados como o Skia, e volta.
const blendPixel = (pixels, at, [r, g, b, a], coverage) => {
  const sa = a * coverage;
  if (sa <= 0) return;
  const da = pixels[at + 3] / 255;
  const outA = sa + da * (1 - sa);
  if (outA <= 0) { pixels.fill(0, at, at + 4); return; }
  for (let c = 0; c < 3; c++) {
    const src = [r, g, b][c] / 255;
    const dst = pixels[at + c] / 255;
    pixels[at + c] = Math.round(((src * sa + dst * da * (1 - sa)) / outA) * 255);
  }
  pixels[at + 3] = Math.round(outA * 255);
};
// Retângulo alinhado aos eixos com cobertura de área (o AA analítico do Skia para retângulos).
const fillAxisRect = (s, x0, y0, x1, y1, color) => {
  const { width, height } = s;
  if (x1 < x0) [x0, x1] = [x1, x0];
  if (y1 < y0) [y0, y1] = [y1, y0];
  const px0 = Math.max(0, Math.floor(x0)), px1 = Math.min(width, Math.ceil(x1));
  const py0 = Math.max(0, Math.floor(y0)), py1 = Math.min(height, Math.ceil(y1));
  for (let y = py0; y < py1; y++) {
    const cy = Math.min(y + 1, y1) - Math.max(y, y0);
    for (let x = px0; x < px1; x++) {
      const cx = Math.min(x + 1, x1) - Math.max(x, x0);
      blendPixel(s.pixels, (y * width + x) * 4, color, cx * cy);
    }
  }
};
const axisAligned = (t) => t[1] === 0 && t[2] === 0;
const canvasEnsureBitmap = (s) => {
  const width = s.canvas.width, height = s.canvas.height;
  if (s.width !== width || s.height !== height) { s.width = width; s.height = height; s.pixels = new Uint8Array(width * height * 4); }
};
const canvasNotFinite = (...values) => !values.every((v) => Number.isFinite(v));
const CANVAS_2D_IMPL = {
  save() { const s = c2d(this); s.stack.push({ ...s.state, transform: s.state.transform.slice(), lineDash: s.state.lineDash.slice() }); },
  restore() { const s = c2d(this); if (s.stack.length) s.state = s.stack.pop(); },
  reset() { const s = c2d(this); s.state = canvas2dDefaults(); s.stack = []; s.path = { ops: [], current: null, start: null }; hidden(this, '__path', s.path); s.pixels = new Uint8Array(s.width * s.height * 4); },
  isContextLost() { c2d(this); return false; },
  getContextAttributes() { const s = c2d(this); return { alpha: s.attrs.alpha, colorSpace: s.attrs.colorSpace, colorType: 'unorm8', desynchronized: s.attrs.desynchronized, toneMapping: { mode: 'standard' }, willReadFrequently: s.attrs.willReadFrequently }; },
  scale(x, y) { const s = c2d(this); [x, y] = [Number(x), Number(y)]; if (canvasNotFinite(x, y)) return; const t = s.state.transform; s.state.transform = [t[0] * x, t[1] * x, t[2] * y, t[3] * y, t[4], t[5]]; },
  rotate(angle) { const s = c2d(this); const a = Number(angle); if (canvasNotFinite(a)) return; const [c, n] = [Math.cos(a), Math.sin(a)]; const t = s.state.transform; s.state.transform = [t[0] * c + t[2] * n, t[1] * c + t[3] * n, t[2] * c - t[0] * n, t[3] * c - t[1] * n, t[4], t[5]]; },
  translate(x, y) { const s = c2d(this); [x, y] = [Number(x), Number(y)]; if (canvasNotFinite(x, y)) return; const t = s.state.transform; s.state.transform = [t[0], t[1], t[2], t[3], t[4] + t[0] * x + t[2] * y, t[5] + t[1] * x + t[3] * y]; },
  transform(a, b, c, d, e, f) { const s = c2d(this); const v = [a, b, c, d, e, f].map(Number); if (canvasNotFinite(...v)) return; const t = s.state.transform; s.state.transform = [t[0] * v[0] + t[2] * v[1], t[1] * v[0] + t[3] * v[1], t[0] * v[2] + t[2] * v[3], t[1] * v[2] + t[3] * v[3], t[0] * v[4] + t[2] * v[5] + t[4], t[1] * v[4] + t[3] * v[5] + t[5]]; },
  setTransform(...args) {
    const s = c2d(this);
    if (args.length >= 6) { const v = args.slice(0, 6).map(Number); if (canvasNotFinite(...v)) return; s.state.transform = v; return; }
    if (args.length > 1) throw new TypeError(`Failed to execute 'setTransform' on 'CanvasRenderingContext2D': Valid arities are: [0, 1, 6], but ${args.length} arguments provided.`);
    const m = matrixFromInit(args[0] ?? {}).v;
    if (canvasNotFinite(m[0], m[1], m[4], m[5], m[12], m[13])) return;
    s.state.transform = [m[0], m[1], m[4], m[5], m[12], m[13]];
  },
  getTransform() { return makeMatrix(DOMMatrix, matrix2D(...c2d(this).state.transform), true); },
  resetTransform() { c2d(this).state.transform = [1, 0, 0, 1, 0, 0]; },
  createLinearGradient(x0, y0, x1, y1) {
    const v = [x0, y0, x1, y1].map(Number);
    if (canvasNotFinite(...v)) throw new TypeError("Failed to execute 'createLinearGradient' on 'CanvasRenderingContext2D': The provided double value is non-finite.");
    const g = Object.create(CanvasGradient.prototype); hidden(g, '__gradient', { kind: 'linear', points: v, stops: [] }); return g;
  },
  createRadialGradient(x0, y0, r0, x1, y1, r1) {
    const v = [x0, y0, r0, x1, y1, r1].map(Number);
    if (canvasNotFinite(...v)) throw new TypeError("Failed to execute 'createRadialGradient' on 'CanvasRenderingContext2D': The provided double value is non-finite.");
    if (v[2] < 0 || v[5] < 0) throw domException(`Failed to execute 'createRadialGradient' on 'CanvasRenderingContext2D': The ${v[2] < 0 ? 'r0' : 'r1'} provided is less than 0.`, 'IndexSizeError');
    const g = Object.create(CanvasGradient.prototype); hidden(g, '__gradient', { kind: 'radial', points: v, stops: [] }); return g;
  },
  createConicGradient(angle, x, y) {
    const v = [angle, x, y].map(Number);
    if (canvasNotFinite(...v)) throw new TypeError("Failed to execute 'createConicGradient' on 'CanvasRenderingContext2D': The provided double value is non-finite.");
    const g = Object.create(CanvasGradient.prototype); hidden(g, '__gradient', { kind: 'conic', points: v, stops: [] }); return g;
  },
  createPattern(image, repetition) {
    const rep = repetition === null || repetition === undefined || repetition === '' ? 'repeat' : String(repetition);
    if (!['repeat', 'repeat-x', 'repeat-y', 'no-repeat'].includes(rep)) throw domException(`Failed to execute 'createPattern' on 'CanvasRenderingContext2D': The provided type ('${repetition}') is not one of 'repeat', 'no-repeat', 'repeat-x', or 'repeat-y'.`, 'SyntaxError');
    const source = canvasImageSource(image, 'createPattern');
    if (source === null) return null;
    const p = Object.create(CanvasPattern.prototype); hidden(p, '__pattern', { source, repetition: rep, transform: null }); return p;
  },
  clearRect(x, y, w, h) {
    const s = c2d(this);
    const v = [x, y, w, h].map(Number);
    if (canvasNotFinite(...v)) return;
    canvasEnsureBitmap(s);
    const t = s.state.transform;
    if (!axisAligned(t)) return;
    const [ax, ay] = canvasTransformPoint(t, v[0], v[1]);
    const [bx, by] = canvasTransformPoint(t, v[0] + v[2], v[1] + v[3]);
    const [x0, x1] = [Math.min(ax, bx), Math.max(ax, bx)];
    const [y0, y1] = [Math.min(ay, by), Math.max(ay, by)];
    for (let py = Math.max(0, Math.floor(y0)); py < Math.min(s.height, Math.ceil(y1)); py++) {
      const cy = Math.min(py + 1, y1) - Math.max(py, y0);
      for (let px = Math.max(0, Math.floor(x0)); px < Math.min(s.width, Math.ceil(x1)); px++) {
        const cx = Math.min(px + 1, x1) - Math.max(px, x0);
        const keep = 1 - cx * cy;
        const at = (py * s.width + px) * 4;
        if (keep <= 0) s.pixels.fill(0, at, at + 4);
        else s.pixels[at + 3] = Math.round(s.pixels[at + 3] * keep);
      }
    }
  },
  fillRect(x, y, w, h) {
    const s = c2d(this);
    const v = [x, y, w, h].map(Number);
    if (canvasNotFinite(...v) || v[2] === 0 || v[3] === 0) return;
    canvasEnsureBitmap(s);
    const t = s.state.transform;
    const style = s.state.fillStyle;
    if (!axisAligned(t) || !Array.isArray(style) || s.state.globalCompositeOperation !== 'source-over' || s.state.clip) return;
    const [ax, ay] = canvasTransformPoint(t, v[0], v[1]);
    const [bx, by] = canvasTransformPoint(t, v[0] + v[2], v[1] + v[3]);
    fillAxisRect(s, ax, ay, bx, by, [style[0], style[1], style[2], style[3] * s.state.globalAlpha]);
  },
  strokeRect(x, y, w, h) {
    const s = c2d(this);
    const v = [x, y, w, h].map(Number);
    if (canvasNotFinite(...v)) return;
    canvasEnsureBitmap(s);
    const t = s.state.transform;
    const style = s.state.strokeStyle;
    const half = s.state.lineWidth / 2;
    if (!axisAligned(t) || t[0] !== t[3] || !Array.isArray(style) || s.state.globalCompositeOperation !== 'source-over' || s.state.clip || s.state.lineJoin !== 'miter' || s.state.lineDash.length) return;
    const color = [style[0], style[1], style[2], style[3] * s.state.globalAlpha];
    const [x0, y0] = canvasTransformPoint(t, v[0], v[1]);
    const [x1, y1] = canvasTransformPoint(t, v[0] + v[2], v[1] + v[3]);
    const h2 = half * Math.abs(t[0]);
    const [l, r] = [Math.min(x0, x1), Math.max(x0, x1)];
    const [tp, bt] = [Math.min(y0, y1), Math.max(y0, y1)];
    fillAxisRect(s, l - h2, tp - h2, r + h2, tp + h2, color);
    fillAxisRect(s, l - h2, bt - h2, r + h2, bt + h2, color);
    fillAxisRect(s, l - h2, tp + h2, l + h2, bt - h2, color);
    fillAxisRect(s, r - h2, tp + h2, r + h2, bt - h2, color);
  },
  beginPath() { const s = c2d(this); s.path = { ops: [], current: null, start: null }; hidden(this, '__path', s.path); },
  fill(pathOrRule, rule) {
    const s = c2d(this);
    const fillRule = String(pathOrRule instanceof Path2D ? rule ?? 'nonzero' : pathOrRule ?? 'nonzero');
    if (fillRule !== 'nonzero' && fillRule !== 'evenodd') throw new TypeError(`Failed to execute 'fill' on 'CanvasRenderingContext2D': The provided value '${fillRule}' is not a valid enum value of type CanvasFillRule.`);
    canvasEnsureBitmap(s);
  },
  stroke(path) { const s = c2d(this); if (path !== undefined && !(path instanceof Path2D)) throw new TypeError("Failed to execute 'stroke' on 'CanvasRenderingContext2D': parameter 1 is not of type 'Path2D'."); canvasEnsureBitmap(s); },
  clip(pathOrRule, rule) {
    const s = c2d(this);
    const fillRule = String(pathOrRule instanceof Path2D ? rule ?? 'nonzero' : pathOrRule ?? 'nonzero');
    if (fillRule !== 'nonzero' && fillRule !== 'evenodd') throw new TypeError(`Failed to execute 'clip' on 'CanvasRenderingContext2D': The provided value '${fillRule}' is not a valid enum value of type CanvasFillRule.`);
    s.state.clip = { path: pathOrRule instanceof Path2D ? pathState(pathOrRule).ops.slice() : s.path.ops.slice(), rule: fillRule, transform: s.state.transform.slice() };
  },
  isPointInPath() { c2d(this); return false; },
  isPointInStroke() { c2d(this); return false; },
  drawFocusIfNeeded() { c2d(this); },
  fillText(text, x, y, maxWidth) { c2d(this); void text; void x; void y; void maxWidth; },
  strokeText(text, x, y, maxWidth) { c2d(this); void text; void x; void y; void maxWidth; },
  measureText(text) {
    const s = c2d(this);
    const metrics = Object.create(TextMetrics.prototype);
    hidden(metrics, '__metrics', canvasMeasureText(s, String(text)));
    return metrics;
  },
  setLineDash(segments) {
    const s = c2d(this);
    if (segments === null || typeof segments !== 'object' || typeof segments[Symbol.iterator] !== 'function') throw new TypeError("Failed to execute 'setLineDash' on 'CanvasRenderingContext2D': The provided value cannot be converted to a sequence.");
    const list = Array.from(segments, Number);
    if (list.some((n) => !Number.isFinite(n) || n < 0)) return;
    s.state.lineDash = list.length % 2 ? [...list, ...list] : list;
  },
  getLineDash() { return c2d(this).state.lineDash.slice(); },
  drawImage(image, ...args) {
    const s = c2d(this);
    if (![2, 4, 8].includes(args.length)) throw new TypeError(`Failed to execute 'drawImage' on 'CanvasRenderingContext2D': Valid arities are: [3, 5, 9], but ${args.length + 1} arguments provided.`);
    const source = canvasImageSource(image, 'drawImage');
    if (source === null) return;
    canvasEnsureBitmap(s);
    const v = args.map(Number);
    if (canvasNotFinite(...v)) return;
    const [sx, sy, sw, sh, dx, dy, dw, dh] = v.length === 8 ? v : v.length === 4 ? [0, 0, source.width, source.height, ...v] : [0, 0, source.width, source.height, v[0], v[1], source.width, source.height];
    const t = s.state.transform;
    // Cópia exata: sem escala, alinhada a pixels inteiros, sem alfa global nem composição especial.
    if (!source.pixels || sw !== dw || sh !== dh || t[0] !== 1 || t[1] !== 0 || t[2] !== 0 || t[3] !== 1 || s.state.clip || s.state.globalCompositeOperation !== 'source-over') return;
    const ox = dx + t[4], oy = dy + t[5];
    if (!Number.isInteger(ox) || !Number.isInteger(oy) || !Number.isInteger(sx) || !Number.isInteger(sy)) return;
    for (let y = 0; y < sh; y++) for (let x = 0; x < sw; x++) {
      const tx = ox + x, ty = oy + y, fx = sx + x, fy = sy + y;
      if (tx < 0 || ty < 0 || tx >= s.width || ty >= s.height || fx < 0 || fy < 0 || fx >= source.width || fy >= source.height) continue;
      const from = (fy * source.width + fx) * 4;
      const p = source.pixels;
      blendPixel(s.pixels, (ty * s.width + tx) * 4, [p[from], p[from + 1], p[from + 2], (p[from + 3] / 255) * s.state.globalAlpha], 1);
    }
  },
  createImageData(a, b, settings) {
    c2d(this);
    if (a instanceof ImageData) return new ImageData(a.width, a.height);
    const [w, h] = [Math.abs(Math.trunc(Number(a))), Math.abs(Math.trunc(Number(b)))];
    if (w === 0 || h === 0) throw domException(`Failed to execute 'createImageData' on 'CanvasRenderingContext2D': The source ${w === 0 ? 'width' : 'height'} is 0.`, 'IndexSizeError');
    void settings;
    return new ImageData(w, h);
  },
  getImageData(sx, sy, sw, sh) {
    const s = c2d(this);
    const [x, y, w, h] = [sx, sy, sw, sh].map((v) => Math.trunc(Number(v)));
    if (w === 0 || h === 0) throw domException(`Failed to execute 'getImageData' on 'CanvasRenderingContext2D': The source ${w === 0 ? 'width' : 'height'} is 0.`, 'IndexSizeError');
    canvasEnsureBitmap(s);
    const [x0, y0, ww, hh] = [w < 0 ? x + w : x, h < 0 ? y + h : y, Math.abs(w), Math.abs(h)];
    const out = new ImageData(ww, hh);
    const data = out.data;
    for (let row = 0; row < hh; row++) for (let col = 0; col < ww; col++) {
      const fx = x0 + col, fy = y0 + row;
      if (fx < 0 || fy < 0 || fx >= s.width || fy >= s.height) continue;
      const from = (fy * s.width + fx) * 4;
      data.set(s.pixels.subarray(from, from + 4), (row * ww + col) * 4);
    }
    return out;
  },
  putImageData(imagedata, dx, dy, dirtyX, dirtyY, dirtyWidth, dirtyHeight) {
    const s = c2d(this);
    if (!(imagedata instanceof ImageData)) throw new TypeError("Failed to execute 'putImageData' on 'CanvasRenderingContext2D': parameter 1 is not of type 'ImageData'.");
    canvasEnsureBitmap(s);
    const src = imageDataOf(imagedata);
    let [x0, y0, w, h] = dirtyX === undefined ? [0, 0, src.width, src.height] : [dirtyX, dirtyY, dirtyWidth, dirtyHeight].map((v) => Math.trunc(Number(v)));
    if (w < 0) { x0 += w; w = -w; }
    if (h < 0) { y0 += h; h = -h; }
    [dx, dy] = [Math.trunc(Number(dx)), Math.trunc(Number(dy))];
    for (let row = Math.max(0, y0); row < Math.min(src.height, y0 + h); row++) for (let col = Math.max(0, x0); col < Math.min(src.width, x0 + w); col++) {
      const tx = dx + col, ty = dy + row;
      if (tx < 0 || ty < 0 || tx >= s.width || ty >= s.height) continue;
      const from = (row * src.width + col) * 4;
      s.pixels.set(src.data.subarray(from, from + 4), (ty * s.width + tx) * 4);
    }
  },
};
for (const name of ['closePath', 'moveTo', 'lineTo', 'quadraticCurveTo', 'bezierCurveTo', 'arcTo', 'arc', 'ellipse', 'rect', 'roundRect']) CANVAS_2D_IMPL[name] = function (...args) { c2d(this); return PathMethods[name].apply(this, args); };
// Membros na ordem medida, com o número de argumentos de cada método.
{
  const members = CANVAS2D_SURFACE.classes.find((c) => c.name === 'CanvasRenderingContext2D').members;
  const lengths = Object.fromEntries(members.filter((m) => m[1] === 'm').map((m) => [m[0], m[2]]));
  for (const key of members.filter((m) => m[1] !== 'm').map((m) => m[0])) Object.defineProperty(CanvasRenderingContext2D.prototype, key, { ...canvas2dProperties[key], enumerable: true, configurable: true });
  for (const name of Object.keys(lengths)) {
    const impl = CANVAS_2D_IMPL[name];
    const arity = lengths[name];
    const method = {
      [name](...args) {
        c2d(this);
        if (args.length < arity) throw new TypeError(`Failed to execute '${name}' on 'CanvasRenderingContext2D': ${arity} argument${arity === 1 ? '' : 's'} required, but only ${args.length} present.`);
        return impl.apply(this, args);
      },
    }[name];
    Object.defineProperty(method, 'length', { value: arity });
    Object.defineProperty(CanvasRenderingContext2D.prototype, name, { value: method, writable: true, enumerable: true, configurable: true });
  }
}
// A fonte de imagem de drawImage e createPattern: largura, altura e pixels (ou null para "não desenhar nada").
const canvasImageSource = (image, method) => {
  if (image instanceof HTMLImageElement) {
    const decoded = decodedImage(image);
    if (!decoded) return null;
    return decoded;
  }
  if (image instanceof HTMLCanvasElement) {
    if (image.width === 0 || image.height === 0) throw domException(`Failed to execute '${method}' on 'CanvasRenderingContext2D': The image argument is a canvas element with a width or height of 0.`, 'InvalidStateError');
    return { width: image.width, height: image.height, pixels: canvasPixels(image) };
  }
  if (image instanceof ImageBitmap) {
    const s = imageBitmapState(image);
    if (s.closed) throw domException(`Failed to execute '${method}' on 'CanvasRenderingContext2D': The image source is detached.`, 'InvalidStateError');
    return { width: s.width, height: s.height, pixels: s.pixels };
  }
  if (image instanceof HTMLVideoElement || image instanceof SVGElement) return null;
  throw new TypeError(`Failed to execute '${method}' on 'CanvasRenderingContext2D': The provided value is not of type '(CSSImageValue or HTMLCanvasElement or HTMLImageElement or HTMLVideoElement or ImageBitmap or OffscreenCanvas or SVGImageElement or VideoFrame)'.`);
};
// As medidas de texto vêm das fontes do sistema, escolhidas como o Chromium escolhe (ponte `measureText`).
const canvasMeasureText = (s, text) => {
  const font = s.state.font;
  const result = op('measureText', text, font.family, font.weight, font.style, font.size);
  const [width, left, right, ascent, descent, fontAscent, fontDescent] = result;
  const baselineShift = { alphabetic: 0, top: fontAscent, hanging: fontAscent * 0.8, middle: (fontAscent - fontDescent) / 2, ideographic: -fontDescent, bottom: -fontDescent }[s.state.textBaseline];
  return {
    width, actualBoundingBoxLeft: left, actualBoundingBoxRight: right,
    fontBoundingBoxAscent: fontAscent - baselineShift, fontBoundingBoxDescent: fontDescent + baselineShift,
    actualBoundingBoxAscent: text === '' ? 0 : ascent - baselineShift, actualBoundingBoxDescent: text === '' ? 0 : descent + baselineShift,
    hangingBaseline: Math.fround(fontAscent * 0.8) - baselineShift, alphabeticBaseline: -baselineShift, ideographicBaseline: -fontDescent - baselineShift,
  };
};
const canvas2dGetContext = (canvas, attributes) => {
  const context = Object.create(CanvasRenderingContext2D.prototype);
  const attrs = { alpha: true, colorSpace: 'srgb', desynchronized: false, willReadFrequently: false };
  if (attributes !== null && attributes !== undefined && typeof attributes === 'object') {
    if (attributes.alpha !== undefined) attrs.alpha = Boolean(attributes.alpha);
    if (attributes.desynchronized !== undefined) attrs.desynchronized = Boolean(attributes.desynchronized);
    if (attributes.willReadFrequently !== undefined) attrs.willReadFrequently = Boolean(attributes.willReadFrequently);
  }
  const state = { canvas, attrs, state: canvas2dDefaults(), stack: [], path: { ops: [], current: null, start: null }, width: canvas.width, height: canvas.height, pixels: new Uint8Array(canvas.width * canvas.height * 4) };
  canvas2dStates.set(context, state);
  hidden(context, '__path', state.path);
  return context;
};
// Os pixels de um canvas 2D, RGBA não pré-multiplicado; WebGL e canvas sem contexto, um bitmap transparente.
const canvasContextOf = (canvas) => canvas[P.__context]?.[1];
const canvasPixels = (canvas) => {
  const context = canvasContextOf(canvas);
  if (context && canvas2dStates.has(context)) { const s = canvas2dStates.get(context); canvasEnsureBitmap(s); return s.pixels; }
  return new Uint8Array(canvas.width * canvas.height * 4);
};
// O resize do canvas zera o bitmap e o estado do contexto 2D (a especificação manda reiniciar o contexto).
const canvasResized = (canvas) => {
  const context = canvasContextOf(canvas);
  if (context && canvas2dStates.has(context)) {
    const s = canvas2dStates.get(context);
    s.state = canvas2dDefaults();
    s.stack = [];
    s.path = { ops: [], current: null, start: null };
    hidden(context, '__path', s.path);
    s.width = -1;
    canvasEnsureBitmap(s);
  }
};
