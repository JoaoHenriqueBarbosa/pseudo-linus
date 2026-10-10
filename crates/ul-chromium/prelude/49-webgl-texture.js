// ---- Texturas do WebGL como o ANGLE as guarda no SwiftShader do oráculo: cada formato interno vira o formato do
// Vulkan que o ANGLE escolhe (RGB8 vira RGBA8 com alfa 1, luminância e alfa viram R8 e R8G8 com mistura de canais), o
// envio converte como o WebGLImageConversion do Chromium, o mipmap vem do blit linear do SwiftShader e a amostragem é
// o SamplerCore: filtragem em ponto fixo de 16 bits para formatos normalizados e em binário32 para float, inteiro,
// comparação, cubo e borda.

// O formato guardado. `kind`: 'unorm', 'snorm', 'float', 'uint' ou 'int'; `bits`: bits por componente dos
// normalizados (o valor guardado é o do sampleTexel, deslocado para o topo dos 16 bits); `swizzle`: de onde vem cada
// canal (índice do componente guardado, 'zero' ou 'one'); `alphaOne`: o alfa do formato emulado é 1.
const TEX_FORMATS = (() => {
  const f = (kind, comps, bits, swizzle, extra) => ({ kind, comps, bits: bits && Array(comps).fill(bits), swizzle, ...extra });
  const rgba = [0, 1, 2, 3], rgb1 = [0, 1, 2, 'one'], r001 = [0, 'zero', 'zero', 'one'], rg01 = [0, 1, 'zero', 'one'];
  return {
    RGBA8: f('unorm', 4, 8, rgba), RGB8: f('unorm', 4, 8, rgb1, { alphaOne: true }), SRGB8_ALPHA8: f('unorm', 4, 8, rgba, { srgb: true }), SRGB8: f('unorm', 4, 8, rgb1, { srgb: true, alphaOne: true }),
    RGBA4: { kind: 'unorm', comps: 4, bits: [4, 4, 4, 4], swizzle: rgba }, RGB5_A1: { kind: 'unorm', comps: 4, bits: [5, 5, 5, 1], swizzle: rgba }, RGB565: { kind: 'unorm', comps: 3, bits: [5, 6, 5], swizzle: rgb1 },
    RGB10_A2: { kind: 'unorm', comps: 4, bits: [10, 10, 10, 2], swizzle: rgba },
    LUMINANCE: f('unorm', 1, 8, [0, 0, 0, 'one']), ALPHA: f('unorm', 1, 8, ['zero', 'zero', 'zero', 0]), LUMINANCE_ALPHA: f('unorm', 2, 8, [0, 0, 0, 1]),
    R8: f('unorm', 1, 8, r001), RG8: f('unorm', 2, 8, rg01),
    R8_SNORM: f('snorm', 1, 8, r001), RG8_SNORM: f('snorm', 2, 8, rg01), RGB8_SNORM: f('snorm', 4, 8, rgb1, { alphaOne: true }), RGBA8_SNORM: f('snorm', 4, 8, rgba),
    R16F: f('float', 1, 0, r001, { half: true }), RG16F: f('float', 2, 0, rg01, { half: true }), RGB16F: f('float', 4, 0, rgb1, { half: true, alphaOne: true }), RGBA16F: f('float', 4, 0, rgba, { half: true }),
    R32F: f('float', 1, 0, r001), RG32F: f('float', 2, 0, rg01), RGB32F: f('float', 4, 0, rgb1, { alphaOne: true }), RGBA32F: f('float', 4, 0, rgba),
    R11F_G11F_B10F: f('float', 3, 0, rgb1, { packedFloat: true }), RGB9_E5: f('float', 3, 0, rgb1, { packedFloat: true }),
    LUMINANCE32F: f('float', 1, 0, [0, 0, 0, 'one']), ALPHA32F: f('float', 1, 0, ['zero', 'zero', 'zero', 0]), LUMINANCE_ALPHA32F: f('float', 2, 0, [0, 0, 0, 1]),
    LUMINANCE16F: f('float', 1, 0, [0, 0, 0, 'one'], { half: true }), ALPHA16F: f('float', 1, 0, ['zero', 'zero', 'zero', 0], { half: true }), LUMINANCE_ALPHA16F: f('float', 2, 0, [0, 0, 0, 1], { half: true }),
    R8UI: f('uint', 1, 8, r001), RG8UI: f('uint', 2, 8, rg01), RGB8UI: f('uint', 4, 8, rgb1, { alphaOne: true }), RGBA8UI: f('uint', 4, 8, rgba),
    R8I: f('int', 1, 8, r001), RG8I: f('int', 2, 8, rg01), RGB8I: f('int', 4, 8, rgb1, { alphaOne: true }), RGBA8I: f('int', 4, 8, rgba),
    R16UI: f('uint', 1, 16, r001), RG16UI: f('uint', 2, 16, rg01), RGB16UI: f('uint', 4, 16, rgb1, { alphaOne: true }), RGBA16UI: f('uint', 4, 16, rgba),
    R16I: f('int', 1, 16, r001), RG16I: f('int', 2, 16, rg01), RGB16I: f('int', 4, 16, rgb1, { alphaOne: true }), RGBA16I: f('int', 4, 16, rgba),
    R32UI: f('uint', 1, 32, r001), RG32UI: f('uint', 2, 32, rg01), RGB32UI: f('uint', 4, 32, rgb1, { alphaOne: true }), RGBA32UI: f('uint', 4, 32, rgba),
    R32I: f('int', 1, 32, r001), RG32I: f('int', 2, 32, rg01), RGB32I: f('int', 4, 32, rgb1, { alphaOne: true }), RGBA32I: f('int', 4, 32, rgba),
    RGB10_A2UI: { kind: 'uint', comps: 4, bits: [10, 10, 10, 2], swizzle: rgba },
    DEPTH_COMPONENT16: f('unorm', 1, 16, r001, { depth: true }), DEPTH_COMPONENT24: f('float', 1, 0, r001, { depth: true }), DEPTH_COMPONENT32F: f('float', 1, 0, r001, { depth: true }),
    DEPTH24_STENCIL8: f('float', 1, 0, r001, { depth: true, stencil: true }), DEPTH32F_STENCIL8: f('float', 1, 0, r001, { depth: true, stencil: true }),
  };
})();
for (const [name, fmt] of Object.entries(TEX_FORMATS)) fmt.name = name;

// Os pares formato/tipo válidos de cada formato interno com tamanho (tabela 3.2 do ES 3.0) e os formatos sem tamanho
// do WebGL 1 com as extensões.
const TEX_SIZED = {
  R8: [['RED', 'UNSIGNED_BYTE']], R8_SNORM: [['RED', 'BYTE']], R16F: [['RED', 'HALF_FLOAT'], ['RED', 'FLOAT']], R32F: [['RED', 'FLOAT']],
  R8UI: [['RED_INTEGER', 'UNSIGNED_BYTE']], R8I: [['RED_INTEGER', 'BYTE']], R16UI: [['RED_INTEGER', 'UNSIGNED_SHORT']], R16I: [['RED_INTEGER', 'SHORT']], R32UI: [['RED_INTEGER', 'UNSIGNED_INT']], R32I: [['RED_INTEGER', 'INT']],
  RG8: [['RG', 'UNSIGNED_BYTE']], RG8_SNORM: [['RG', 'BYTE']], RG16F: [['RG', 'HALF_FLOAT'], ['RG', 'FLOAT']], RG32F: [['RG', 'FLOAT']],
  RG8UI: [['RG_INTEGER', 'UNSIGNED_BYTE']], RG8I: [['RG_INTEGER', 'BYTE']], RG16UI: [['RG_INTEGER', 'UNSIGNED_SHORT']], RG16I: [['RG_INTEGER', 'SHORT']], RG32UI: [['RG_INTEGER', 'UNSIGNED_INT']], RG32I: [['RG_INTEGER', 'INT']],
  RGB8: [['RGB', 'UNSIGNED_BYTE']], SRGB8: [['RGB', 'UNSIGNED_BYTE']], RGB565: [['RGB', 'UNSIGNED_BYTE'], ['RGB', 'UNSIGNED_SHORT_5_6_5']], RGB8_SNORM: [['RGB', 'BYTE']],
  R11F_G11F_B10F: [['RGB', 'UNSIGNED_INT_10F_11F_11F_REV'], ['RGB', 'HALF_FLOAT'], ['RGB', 'FLOAT']], RGB9_E5: [['RGB', 'UNSIGNED_INT_5_9_9_9_REV'], ['RGB', 'HALF_FLOAT'], ['RGB', 'FLOAT']],
  RGB16F: [['RGB', 'HALF_FLOAT'], ['RGB', 'FLOAT']], RGB32F: [['RGB', 'FLOAT']],
  RGB8UI: [['RGB_INTEGER', 'UNSIGNED_BYTE']], RGB8I: [['RGB_INTEGER', 'BYTE']], RGB16UI: [['RGB_INTEGER', 'UNSIGNED_SHORT']], RGB16I: [['RGB_INTEGER', 'SHORT']], RGB32UI: [['RGB_INTEGER', 'UNSIGNED_INT']], RGB32I: [['RGB_INTEGER', 'INT']],
  RGBA8: [['RGBA', 'UNSIGNED_BYTE']], SRGB8_ALPHA8: [['RGBA', 'UNSIGNED_BYTE']], RGBA8_SNORM: [['RGBA', 'BYTE']],
  RGB5_A1: [['RGBA', 'UNSIGNED_BYTE'], ['RGBA', 'UNSIGNED_SHORT_5_5_5_1'], ['RGBA', 'UNSIGNED_INT_2_10_10_10_REV']], RGBA4: [['RGBA', 'UNSIGNED_BYTE'], ['RGBA', 'UNSIGNED_SHORT_4_4_4_4']],
  RGB10_A2: [['RGBA', 'UNSIGNED_INT_2_10_10_10_REV']], RGBA16F: [['RGBA', 'HALF_FLOAT'], ['RGBA', 'FLOAT']], RGBA32F: [['RGBA', 'FLOAT']],
  RGBA8UI: [['RGBA_INTEGER', 'UNSIGNED_BYTE']], RGBA8I: [['RGBA_INTEGER', 'BYTE']], RGB10_A2UI: [['RGBA_INTEGER', 'UNSIGNED_INT_2_10_10_10_REV']],
  RGBA16UI: [['RGBA_INTEGER', 'UNSIGNED_SHORT']], RGBA16I: [['RGBA_INTEGER', 'SHORT']], RGBA32I: [['RGBA_INTEGER', 'INT']], RGBA32UI: [['RGBA_INTEGER', 'UNSIGNED_INT']],
  DEPTH_COMPONENT16: [['DEPTH_COMPONENT', 'UNSIGNED_SHORT'], ['DEPTH_COMPONENT', 'UNSIGNED_INT']], DEPTH_COMPONENT24: [['DEPTH_COMPONENT', 'UNSIGNED_INT']], DEPTH_COMPONENT32F: [['DEPTH_COMPONENT', 'FLOAT']],
  DEPTH24_STENCIL8: [['DEPTH_STENCIL', 'UNSIGNED_INT_24_8']], DEPTH32F_STENCIL8: [['DEPTH_STENCIL', 'FLOAT_32_UNSIGNED_INT_24_8_REV']],
};
const HALF_FLOAT_OES = 36193;
// O formato guardado de um envio sem tamanho (internalformat igual ao format), pelo tipo.
const texUnsized = (s, format, type) => {
  const name = enumName(s, format, ['RGBA', 'RGB', 'LUMINANCE', 'ALPHA', 'LUMINANCE_ALPHA']);
  const ext = (n) => s.extensions.has(n);
  if (!name) {
    if (format === 35904 && ext('EXT_sRGB') && type === GL.UNSIGNED_BYTE) return 'SRGB8';
    if (format === 35906 && ext('EXT_sRGB') && type === GL.UNSIGNED_BYTE) return 'SRGB8_ALPHA8';
    if (format === GL.DEPTH_COMPONENT && (type === GL.UNSIGNED_SHORT || type === GL.UNSIGNED_INT) && (s.version === 2 || ext('WEBGL_depth_texture'))) return type === GL.UNSIGNED_SHORT ? 'DEPTH_COMPONENT16' : 'DEPTH_COMPONENT24';
    if (format === GL.DEPTH_STENCIL && type === GL.UNSIGNED_INT_24_8 && (s.version === 2 || ext('WEBGL_depth_texture'))) return 'DEPTH24_STENCIL8';
    return null;
  }
  if (type === GL.UNSIGNED_BYTE) return { RGBA: 'RGBA8', RGB: 'RGB8', LUMINANCE: 'LUMINANCE', ALPHA: 'ALPHA', LUMINANCE_ALPHA: 'LUMINANCE_ALPHA' }[name];
  if (type === GL.UNSIGNED_SHORT_4_4_4_4) return name === 'RGBA' ? 'RGBA4' : null;
  if (type === GL.UNSIGNED_SHORT_5_5_5_1) return name === 'RGBA' ? 'RGB5_A1' : null;
  if (type === GL.UNSIGNED_SHORT_5_6_5) return name === 'RGB' ? 'RGB565' : null;
  if (type === GL.FLOAT && (s.version === 2 || ext('OES_texture_float'))) return { RGBA: 'RGBA32F', RGB: 'RGB32F', LUMINANCE: 'LUMINANCE32F', ALPHA: 'ALPHA32F', LUMINANCE_ALPHA: 'LUMINANCE_ALPHA32F' }[name];
  if ((type === HALF_FLOAT_OES && ext('OES_texture_half_float')) || (type === GL.HALF_FLOAT && s.version === 2)) return { RGBA: 'RGBA16F', RGB: 'RGB16F', LUMINANCE: 'LUMINANCE16F', ALPHA: 'ALPHA16F', LUMINANCE_ALPHA: 'LUMINANCE_ALPHA16F' }[name];
  return null;
};
// Resolve (internalformat, format, type) no formato guardado, ou o erro do GL.
const texResolveFormat = (s, internalformat, format, type) => {
  const sized = Object.keys(TEX_SIZED).find((k) => GL[k] === internalformat);
  if (sized && s.version === 2) {
    const formatName = enumName(s, format, ['RED', 'RG', 'RGB', 'RGBA', 'RED_INTEGER', 'RG_INTEGER', 'RGB_INTEGER', 'RGBA_INTEGER', 'DEPTH_COMPONENT', 'DEPTH_STENCIL']);
    const typeName = Object.keys(TEX_TYPES).find((k) => GL[k] === type);
    if (!formatName || !typeName) return { error: GL.INVALID_ENUM };
    if (!TEX_SIZED[sized].some(([fm, ty]) => fm === formatName && ty === typeName)) return { error: GL.INVALID_OPERATION };
    return { fmt: TEX_FORMATS[sized] };
  }
  if (internalformat !== format) return { error: s.version === 2 && enumName(s, internalformat, ['RGBA', 'RGB', 'LUMINANCE', 'ALPHA', 'LUMINANCE_ALPHA']) ? GL.INVALID_OPERATION : GL.INVALID_VALUE };
  if (!Object.keys(TEX_TYPES).some((k) => GL[k] === type) && type !== HALF_FLOAT_OES) return { error: GL.INVALID_ENUM };
  const name = texUnsized(s, format, type);
  if (!name) return { error: enumName(s, format, ['RGBA', 'RGB', 'LUMINANCE', 'ALPHA', 'LUMINANCE_ALPHA', 'DEPTH_COMPONENT', 'DEPTH_STENCIL']) || format === 35904 || format === 35906 ? GL.INVALID_OPERATION : GL.INVALID_ENUM };
  return { fmt: TEX_FORMATS[name] };
};

// Os tipos de envio: bytes por pixel (os empacotados) ou por componente.
const TEX_TYPES = {
  UNSIGNED_BYTE: { size: 1 }, BYTE: { size: 1 }, UNSIGNED_SHORT: { size: 2 }, SHORT: { size: 2 }, UNSIGNED_INT: { size: 4 }, INT: { size: 4 }, FLOAT: { size: 4 }, HALF_FLOAT: { size: 2 },
  UNSIGNED_SHORT_5_6_5: { packed: 2, fields: [[11, 5], [5, 6], [0, 5]] }, UNSIGNED_SHORT_4_4_4_4: { packed: 2, fields: [[12, 4], [8, 4], [4, 4], [0, 4]] }, UNSIGNED_SHORT_5_5_5_1: { packed: 2, fields: [[11, 5], [6, 5], [1, 5], [0, 1]] },
  UNSIGNED_INT_2_10_10_10_REV: { packed: 4, fields: [[0, 10], [10, 10], [20, 10], [30, 2]] }, UNSIGNED_INT_10F_11F_11F_REV: { packed: 4 }, UNSIGNED_INT_5_9_9_9_REV: { packed: 4 },
  UNSIGNED_INT_24_8: { packed: 4 }, FLOAT_32_UNSIGNED_INT_24_8_REV: { packed: 8 },
};
const TEX_FORMAT_COMPONENTS = { RED: 1, RG: 2, RGB: 3, RGBA: 4, RED_INTEGER: 1, RG_INTEGER: 2, RGB_INTEGER: 3, RGBA_INTEGER: 4, LUMINANCE: 1, ALPHA: 1, LUMINANCE_ALPHA: 2, DEPTH_COMPONENT: 1, DEPTH_STENCIL: 2 };
const texComponents = (format) => TEX_FORMAT_COMPONENTS[Object.keys(TEX_FORMAT_COMPONENTS).find((k) => GL[k] === format)] ?? (format === 35904 ? 3 : 4);
const texTypeInfo = (type) => (type === HALF_FLOAT_OES ? TEX_TYPES.HALF_FLOAT : TEX_TYPES[Object.keys(TEX_TYPES).find((k) => GL[k] === type)]);
const texBytesPerPixel = (format, type) => { const t = texTypeInfo(type); return t.packed || t.size * texComponents(format); };

// binário16 para binário32 e de volta (arredondamento ao par, como o float32ToFloat16 do ANGLE).
const halfToFloat = (h) => {
  const s = h & 0x8000 ? -1 : 1, e = (h >> 10) & 31, m = h & 1023;
  if (e === 0) return s * m * 2 ** -24;
  if (e === 31) return m ? NaN : s * Infinity;
  return s * (1 + m / 1024) * 2 ** (e - 15);
};
const floatToHalf = (x) => {
  const v = new DataView(new ArrayBuffer(4));
  v.setFloat32(0, x);
  const bits = v.getUint32(0), sign = (bits >>> 16) & 0x8000, abs = bits & 0x7fffffff;
  if (abs >= 0x7f800000) return sign | 0x7c00 | (abs > 0x7f800000 ? 0x200 : 0);
  if (abs >= 0x477ff000) return sign | 0x7c00;
  if (abs < 0x38800000) {
    const shift = 113 - (abs >>> 23);
    if (shift > 24) return sign;
    const m = (abs & 0x7fffff) | 0x800000;
    const r = m >>> (shift + 13), rest = m & ((1 << (shift + 13)) - 1), half = 1 << (shift + 12);
    return sign | (r + (rest > half || (rest === half && r & 1) ? 1 : 0));
  }
  const r = abs - 0x38000000;
  return sign | ((r + 0xfff + ((r >>> 13) & 1)) >>> 13);
};
// Os floats empacotados de 11, 10 e 9 bits com expoente comum.
const smallFloat = (v, mbits) => { const e = v >> mbits, m = v & ((1 << mbits) - 1); return e === 0 ? m * 2 ** (-14 - mbits) : e === 31 ? (m ? NaN : Infinity) : (1 + m / (1 << mbits)) * 2 ** (e - 15); };
const toSmallFloat = (x, mbits) => { const h = floatToHalf(Math.max(0, x)); return h >> (10 - mbits); };

// Os pixels de um envio ficam planos, `n` componentes por pixel em `data` (Float64Array), sem objeto por pixel: uma
// textura de 852x492 da vite.dev com um array por pixel esgotava a memória antes de terminar o envio.
const texPixels = (n, count) => ({ n, data: new Float64Array(n * count) });

// Lê os pixels de um envio em componentes do formato de origem: inteiros para os tipos inteiros, campos para os
// empacotados, binário32 para os de ponto flutuante. A primeira linha da fonte é t = 0.
const texReadSource = (s, data, w, h, d, format, type, offsetBytes) => {
  const info = texTypeInfo(type);
  const comps = texComponents(format);
  const bpp = texBytesPerPixel(format, type);
  const p = s.params;
  const align = p.get('UNPACK_ALIGNMENT');
  const rowLength = (s.version === 2 && p.get('UNPACK_ROW_LENGTH')) || w;
  const imageHeight = (s.version === 2 && p.get('UNPACK_IMAGE_HEIGHT')) || h;
  const skipPixels = s.version === 2 ? p.get('UNPACK_SKIP_PIXELS') : 0, skipRows = s.version === 2 ? p.get('UNPACK_SKIP_ROWS') : 0, skipImages = s.version === 2 ? p.get('UNPACK_SKIP_IMAGES') : 0;
  const rowBytes = Math.ceil((rowLength * bpp) / align) * align;
  const view = new DataView(data.buffer, data.byteOffset + (offsetBytes || 0));
  const special = type === GL.UNSIGNED_INT_10F_11F_11F_REV || type === GL.UNSIGNED_INT_5_9_9_9_REV ? 3 : type === GL.UNSIGNED_INT_24_8 || type === GL.FLOAT_32_UNSIGNED_INT_24_8_REV ? 2 : 0;
  const n = special || (info.packed ? info.fields.length : comps);
  const out = texPixels(n, w * h * d), px = out.data;
  const fields = info.packed && info.fields;
  const read = type === GL.UNSIGNED_BYTE ? (q) => view.getUint8(q) : type === GL.BYTE ? (q) => view.getInt8(q) : type === GL.UNSIGNED_SHORT ? (q) => view.getUint16(q, true) : type === GL.SHORT ? (q) => view.getInt16(q, true)
    : type === GL.UNSIGNED_INT ? (q) => view.getUint32(q, true) : type === GL.INT ? (q) => view.getInt32(q, true) : type === GL.FLOAT ? (q) => view.getFloat32(q, true) : (q) => halfToFloat(view.getUint16(q, true));
  for (let z = 0; z < d; z++) for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = ((skipImages + z) * imageHeight + skipRows + y) * rowBytes + (skipPixels + x) * bpp;
    const i = ((z * h + y) * w + x) * n;
    if (type === GL.UNSIGNED_INT_10F_11F_11F_REV) { const v = view.getUint32(o, true); px[i] = smallFloat(v & 2047, 6); px[i + 1] = smallFloat((v >>> 11) & 2047, 6); px[i + 2] = smallFloat((v >>> 22) & 1023, 5); }
    else if (type === GL.UNSIGNED_INT_5_9_9_9_REV) { const v = view.getUint32(o, true), e = (v >>> 27) - 24; px[i] = (v & 511) * 2 ** e; px[i + 1] = ((v >>> 9) & 511) * 2 ** e; px[i + 2] = ((v >>> 18) & 511) * 2 ** e; }
    else if (type === GL.UNSIGNED_INT_24_8) { const v = view.getUint32(o, true); px[i] = (v >>> 8) / 16777215; px[i + 1] = v & 255; }
    else if (type === GL.FLOAT_32_UNSIGNED_INT_24_8_REV) { px[i] = view.getFloat32(o, true); px[i + 1] = view.getUint32(o + 4, true) & 255; }
    else if (fields) { const v = info.packed === 2 ? view.getUint16(o, true) : view.getUint32(o, true); for (let c = 0; c < n; c++) px[i + c] = (v >>> fields[c][0]) & ((1 << fields[c][1]) - 1); }
    else for (let c = 0; c < n; c++) px[i + c] = read(o + c * info.size);
  }
  return out;
};
// Os bytes que um envio de w x h x d lê da fonte, com o alinhamento e os saltos.
const texSourceBytes = (s, w, h, d, format, type) => {
  if (!w || !h || !d) return 0;
  const p = s.params, bpp = texBytesPerPixel(format, type), align = p.get('UNPACK_ALIGNMENT');
  const rowLength = (s.version === 2 && p.get('UNPACK_ROW_LENGTH')) || w;
  const imageHeight = (s.version === 2 && p.get('UNPACK_IMAGE_HEIGHT')) || h;
  const skipPixels = s.version === 2 ? p.get('UNPACK_SKIP_PIXELS') : 0, skipRows = s.version === 2 ? p.get('UNPACK_SKIP_ROWS') : 0, skipImages = s.version === 2 ? p.get('UNPACK_SKIP_IMAGES') : 0;
  const rowBytes = Math.ceil((rowLength * bpp) / align) * align;
  return ((skipImages + d - 1) * imageHeight + skipRows + h - 1) * rowBytes + (skipPixels + w) * bpp;
};

// UNPACK_FLIP_Y e UNPACK_PREMULTIPLY_ALPHA sobre os pixels de origem, como o WebGLImageConversion: a pré-multiplicação
// dos tipos de 8 bits e empacotados passa por RGBA8 com o fator em binário32 e truncamento.
const texUnpackOptions = (s, pixels, w, h, d, format, type) => {
  const n = pixels.n;
  if (s.params.get('UNPACK_FLIP_Y_WEBGL')) {
    const flipped = texPixels(n, w * h * d), row = w * n;
    for (let z = 0; z < d; z++) for (let y = 0; y < h; y++) flipped.data.set(pixels.data.subarray(((z * h + h - 1 - y) * w) * n, ((z * h + h - 1 - y) * w) * n + row), ((z * h + y) * w) * n);
    pixels = flipped;
  }
  const comps = texComponents(format);
  const alphaIndex = format === GL.RGBA ? 3 : format === GL.LUMINANCE_ALPHA ? 1 : -1;
  if (!s.params.get('UNPACK_PREMULTIPLY_ALPHA_WEBGL') || alphaIndex < 0) return pixels;
  const info = texTypeInfo(type), px = pixels.data;
  if (type === GL.FLOAT || type === GL.HALF_FLOAT || type === HALF_FLOAT_OES) {
    for (let i = 0; i < px.length; i += n) for (let c = 0; c < n; c++) if (c !== alphaIndex) px[i + c] = fr(px[i + c] * px[i + alphaIndex]);
    return pixels;
  }
  const fields = info.packed ? info.fields.map(([, bits]) => bits) : Array(comps).fill(8);
  const to8 = (c, bits) => (bits === 8 ? c : bits === 1 ? c * 255 : (c << (8 - bits)) | (c >> (2 * bits - 8)));
  for (let i = 0; i < px.length; i += n) {
    const scale = fr(to8(px[i + alphaIndex], fields[alphaIndex]) / 255);
    for (let c = 0; c < n; c++) {
      const v = c === alphaIndex ? to8(px[i + c], fields[c]) : Math.trunc(fr(to8(px[i + c], fields[c]) * scale));
      px[i + c] = fields[c] === 8 ? v : v >> (8 - fields[c]);
    }
  }
  return pixels;
};

// Converte pixels de origem nos componentes guardados do formato (as funções de carga do ANGLE). O que depende só do
// formato e do tipo sai do laço: a função devolvida grava o pixel `si` de `src` no texel `di` de `dst`.
const texStorer = (fmt, type) => {
  const n = fmt.comps;
  const alphaSource = fmt.name === 'ALPHA' || fmt.name === 'ALPHA32F' || fmt.name === 'ALPHA16F';
  const info = texTypeInfo(type);
  const srcBits = info && info.packed && info.fields ? info.fields.map(([, b]) => b) : null;
  const smallFloats = fmt.name === 'R11F_G11F_B10F' && type !== GL.UNSIGNED_INT_10F_11F_11F_REV;
  return (src, si, srcN, dst, di) => {
    for (let c = 0; c < n; c++) {
      const alphaFill = fmt.alphaOne && c === 3;
      const v = alphaSource ? (c === 0 ? src[si] : undefined) : c < srcN ? src[si + c] : undefined;
      let out;
      switch (fmt.kind) {
        case 'unorm': {
          const bits = fmt.bits[c];
          if (alphaFill) { out = (0xffff << (16 - bits)) & 0xffff; break; }
          if (fmt.depth) { out = type === GL.UNSIGNED_INT ? v >>> 16 : v; break; }
          const from = srcBits ? srcBits[c] : 8;
          const value = from === bits ? v : from > bits ? v >> (from - bits) : v << (bits - from);
          out = (value << (16 - bits)) & 0xffff;
          break;
        }
        case 'snorm': out = alphaFill ? 0x7f00 : (v << 8) & 0xffff; break;
        case 'float': {
          let x = alphaFill ? 1 : v === undefined ? 0 : fmt.depth && type === GL.UNSIGNED_INT ? fr((v >>> 8) / 16777215) : v;
          if (fmt.half) x = halfToFloat(floatToHalf(x));
          else if (smallFloats) x = smallFloat(toSmallFloat(x, c === 2 ? 5 : 6), c === 2 ? 5 : 6);
          out = fr(x);
          break;
        }
        default: out = alphaFill ? 1 : v === undefined ? 0 : v;
      }
      dst[di * n + c] = out;
    }
  };
};

// ---- Fontes DOM: a primeira linha enviada é a de cima da imagem; sem pré-multiplicar, os pixels não pré-multiplicados
// da fonte; com, a pré-multiplicação arredondada do Skia (o WebGL de origem entrega o próprio buffer, já
// pré-multiplicado).
const texDomPixels = (s, source) => {
  const premultiply = s.params.get('UNPACK_PREMULTIPLY_ALPHA_WEBGL');
  let w, h, rgba, premultiplied = false;
  if (source instanceof ImageData) { const st = imageDataOf(source); w = st.width; h = st.height; rgba = new Uint8Array(st.data); }
  else if (typeof HTMLImageElement !== 'undefined' && source instanceof HTMLImageElement) { const dec = decodedImage(source); if (!dec) return null; w = dec.width; h = dec.height; rgba = dec.pixels; }
  else if (typeof HTMLCanvasElement !== 'undefined' && source instanceof HTMLCanvasElement) {
    w = source.width; h = source.height;
    const context = canvasContextOf(source), gl = context && context[P.__gl];
    if (gl && premultiply && !gl.lost && w && h) {
      const raw = glFlush(gl);
      rgba = new Uint8Array(w * h * 4);
      for (let y = 0; y < h; y++) rgba.set(raw.subarray((h - 1 - y) * w * 4, (h - y) * w * 4), y * w * 4);
      if (!gl.attrs.alpha) for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;
      premultiplied = true;
    } else rgba = canvasPixels(source);
  } else if (typeof ImageBitmap !== 'undefined' && source instanceof ImageBitmap) { const st = imageBitmapState(source); w = st.width; h = st.height; rgba = st.pixels; }
  else return null;
  const out = new Uint8Array(w * h * 4), flip = s.params.get('UNPACK_FLIP_Y_WEBGL');
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const i = (y * w + x) * 4, o = ((flip ? h - 1 - y : y) * w + x) * 4;
    const a = rgba[i + 3];
    for (let c = 0; c < 3; c++) { const v = rgba[i + c]; out[o + c] = premultiply && !premultiplied ? Math.trunc((v * a + 128 + ((v * a + 128) >> 8)) / 256) : v; }
    out[o + 3] = a;
  }
  return { w, h, rgba: out };
};
// Os pixels RGBA8 de uma fonte DOM no formato de envio pedido (RGB, LUMINANCE, ...), como o pack do Chromium.
const texDomToFormat = (rgba, count, format, type) => {
  const f = enumName(null, format, ['RGBA', 'RGB', 'LUMINANCE', 'ALPHA', 'LUMINANCE_ALPHA', 'RED', 'RG', 'RED_INTEGER', 'RG_INTEGER', 'RGB_INTEGER', 'RGBA_INTEGER']) || 'RGBA';
  let pick = { RGBA: [0, 1, 2, 3], RGB: [0, 1, 2], LUMINANCE: [0], ALPHA: [3], LUMINANCE_ALPHA: [0, 3], RED: [0], RG: [0, 1], RED_INTEGER: [0], RG_INTEGER: [0, 1], RGB_INTEGER: [0, 1, 2], RGBA_INTEGER: [0, 1, 2, 3] }[f];
  if (format === 35904) pick = [0, 1, 2];
  const info = texTypeInfo(type);
  const float = type === GL.FLOAT || type === GL.HALF_FLOAT || type === HALF_FLOAT_OES;
  const shifts = !float && info.packed && info.fields ? pick.map((_, i) => 8 - info.fields[i][1]) : null;
  const n = pick.length, out = texPixels(n, count), px = out.data;
  for (let i = 0; i < count; i++) for (let k = 0; k < n; k++) {
    const c = rgba[i * 4 + pick[k]];
    px[i * n + k] = float ? fr(c / 255) : shifts ? c >> shifts[k] : c;
  }
  return out;
};

// ---- O armazenamento: cada imagem é { w, h, d, fmt, data }, com `data` plano (Float64Array, `fmt.comps` componentes
// guardados por texel); a fila de desenho fixa as imagens que usa e uma escrita posterior troca a imagem fixada por
// uma cópia.
const texImageKey = (face, level) => face + ':' + level;
const texFace = (target) => (target >= GL.TEXTURE_CUBE_MAP_POSITIVE_X && target <= GL.TEXTURE_CUBE_MAP_NEGATIVE_Z ? target - GL.TEXTURE_CUBE_MAP_POSITIVE_X : 0);
const texSetImage = (t, face, level, image) => { t.images ??= new Map(); t.images.set(texImageKey(face, level), image); t.version = (t.version || 0) + 1; };
const texWritable = (s, t, face, level) => {
  const key = texImageKey(face, level);
  const img = t.images && t.images.get(key);
  glBeforeWrite(s, img);
  t.version = (t.version || 0) + 1;
  return img;
};
// Os componentes ficam planos em `data`, `fmt.comps` por texel.
const texNewImage = (fmt, w, h, d) => {
  const data = new Float64Array(w * h * d * fmt.comps);
  if (fmt.alphaOne) { const zero = new Array(fmt.comps); texStorer(fmt, GL.UNSIGNED_BYTE)([], 0, 0, zero, 0); for (let i = 3; i < data.length; i += 4) data[i] = zero[3]; }
  return { w, h, d, fmt, data, pinned: null };
};
// O texel `i` de uma imagem, como array de componentes.
const texGet = (img, i) => { const n = img.fmt.comps, out = new Array(n); for (let c = 0; c < n; c++) out[c] = img.data[i * n + c]; return out; };

// texImage2D/texImage3D/texSubImage*: as fontes (typed array, buffer de desempacotamento, ImageData, imagem, canvas)
// viram componentes guardados.
const texUpload = (ctx, s, args) => {
  const { t, face, level, fmt, x0, y0, z0, w, h, d, format, type, source, offset, sub } = args;
  let pixels;
  if (source && typeof source === 'object' && !ArrayBuffer.isView(source)) {
    const dom = texDomPixels(s, source);
    if (!dom) return;
    pixels = texDomToFormat(dom.rgba, dom.w * dom.h, format, type);
  } else if (source || s.params.get('PIXEL_UNPACK_BUFFER_BINDING')) {
    const buffer = source ? null : objOf(s.params.get('PIXEL_UNPACK_BUFFER_BINDING'));
    const data = source ? new Uint8Array(source.buffer, source.byteOffset, source.byteLength) : buffer.data;
    pixels = texUnpackOptions(s, texReadSource(s, data, w, h, d, format, type, source ? (offset || 0) * (source.BYTES_PER_ELEMENT || 1) : offset || 0), w, h, d, format, type);
  }
  let img = sub ? texWritable(s, t, face, level) : null;
  if (!sub) { img = texNewImage(fmt, w, h, d); texSetImage(t, face, level, img); }
  if (!img || !pixels) return;
  const store = texStorer(img.fmt, type), n = pixels.n;
  for (let z = 0; z < d; z++) for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) store(pixels.data, ((z * h + y) * w + x) * n, n, img.data, ((z0 + z) * img.h + y0 + y) * img.w + x0 + x);
};

// ---- O generateMipmap do ANGLE no Vulkan: vkCmdBlitImage com filtro linear de cada nível para o seguinte, no
// Blitter do SwiftShader (bilinear em binário32 sobre os valores inteiros do formato, escrita com arredondamento ao
// par); os formatos inteiros não filtram.
const texBlitLevel = (src, w, h, d) => {
  const fmt = src.fmt, n = fmt.comps, sd = src.data, sw = src.w, sh = src.h;
  const out = texNewImage(fmt, w, h, d);
  const rx = fr(sw / w), ry = fr(sh / h), rz = fr(src.d / d);
  const x0 = fr(0.5 * rx), y0 = fr(0.5 * ry), z0 = fr(0.5 * rz);
  const filter3D = src.d !== d;
  // Sem closure por texel: o zjsc ainda não coleta, e cada uma ficaria viva até o fim da página.
  const at = (x, y, z, c) => blitRead(fmt, c, sd[((z * sh + y) * sw + x) * n + c]);
  const bil = (X0, X1, Y0, Y1, fx, fy, ix, iy, z, c) => fr(fr(fr(fr(at(X0, Y0, z, c) * ix) + fr(at(X1, Y0, z, c) * fx)) * iy) + fr(fr(fr(at(X0, Y1, z, c) * ix) + fr(at(X1, Y1, z, c) * fx)) * fy));
  for (let k = 0; k < d; k++) for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) {
    const X = fr(x0 + fr(i * rx)), Y = fr(y0 + fr(j * ry)), Z = fr(z0 + fr(k * rz));
    const xs = fr(X - 0.5), ys = fr(Y - 0.5), zs = fr(Z - 0.5);
    const X0 = Math.max(Math.trunc(xs), 0), Y0 = Math.max(Math.trunc(ys), 0), Z0 = Math.max(Math.trunc(zs), 0);
    const X1 = X0 + 1 >= sw ? X0 : X0 + 1, Y1 = Y0 + 1 >= sh ? Y0 : Y0 + 1, Z1 = Z0 + 1 >= src.d ? Z0 : Z0 + 1;
    const fx = fr(xs - X0), fy = fr(ys - Y0), ix = fr(1 - fx), iy = fr(1 - fy), fz = fr(zs - Z0), iz = fr(1 - fz);
    for (let c = 0; c < n; c++) {
      const color = filter3D ? fr(fr(bil(X0, X1, Y0, Y1, fx, fy, ix, iy, Z0, c) * iz) + fr(bil(X0, X1, Y0, Y1, fx, fy, ix, iy, Z1, c) * fz)) : bil(X0, X1, Y0, Y1, fx, fy, ix, iy, k, c);
      out.data[((k * h + j) * w + i) * n + c] = blitWrite(fmt, c, color);
    }
  }
  return out;
};
const blitRead = (fmt, c, v) => (fmt.kind === 'unorm' ? v >> (16 - fmt.bits[c]) : fmt.kind === 'snorm' ? ((v << 16) >> 24) : v);
const blitWrite = (fmt, c, v) => {
  if (fmt.kind === 'unorm') { const max = (1 << fmt.bits[c]) - 1; return (Math.min(max, Math.max(0, roundEven(v))) << (16 - fmt.bits[c])) & 0xffff; }
  if (fmt.kind === 'snorm') return (Math.min(127, Math.max(-127, roundEven(v))) << 8) & 0xffff;
  return fmt.half ? halfToFloat(floatToHalf(v)) : fr(v);
};

// ---- Completude e o estado do amostrador, como o ANGLE monta o VkSampler e a vista.
const TEX_MIP_FILTERS = new Set([GL.NEAREST_MIPMAP_NEAREST, GL.LINEAR_MIPMAP_NEAREST, GL.NEAREST_MIPMAP_LINEAR, GL.LINEAR_MIPMAP_LINEAR]);
const texIsPot = (x) => x > 0 && (x & (x - 1)) === 0;
const texFilterable = (s, fmt) => {
  if (fmt.kind === 'uint' || fmt.kind === 'int') return false;
  if (fmt.kind === 'float' && !fmt.half && !fmt.packedFloat && !fmt.depth) return s.extensions.has('OES_texture_float_linear');
  if (fmt.kind === 'float' && fmt.half && s.version !== 2) return s.extensions.has('OES_texture_half_float_linear');
  return true;
};
const TEX_TYPES_BY_TARGET = { TEXTURE_2D: '2D', TEXTURE_CUBE_MAP: 'CUBE', TEXTURE_3D: '3D', TEXTURE_2D_ARRAY: 'ARRAY' };
// A vista amostrável de uma textura no momento do desenho, ou `null` se incompleta.
const texView = (s, texture, target, samplerObject) => {
  const t = objOf(texture);
  if (!t || !t.images) return null;
  const params = samplerObject ? new Map([...t.params, ...objOf(samplerObject).params]) : t.params;
  const type = TEX_TYPES_BY_TARGET[target];
  const faces = type === 'CUBE' ? 6 : 1;
  const immutable = t.params.get('TEXTURE_IMMUTABLE_FORMAT');
  const base = s.version === 2 ? Math.min(t.params.get('TEXTURE_BASE_LEVEL'), immutable ? t.params.get('TEXTURE_IMMUTABLE_LEVELS') - 1 : Infinity) : 0;
  const maxLevelParam = s.version === 2 ? t.params.get('TEXTURE_MAX_LEVEL') : 1000;
  const img0 = t.images.get(texImageKey(0, base));
  if (!img0 || !img0.w || !img0.h || !img0.d) return null;
  const fmt = img0.fmt;
  const minFilter = params.get('TEXTURE_MIN_FILTER'), magFilter = params.get('TEXTURE_MAG_FILTER');
  const mip = TEX_MIP_FILTERS.has(minFilter);
  for (let f = 1; f < faces; f++) { const o = t.images.get(texImageKey(f, base)); if (!o || o.w !== img0.w || o.h !== img0.h || o.fmt !== fmt) return null; }
  if (type === 'CUBE' && img0.w !== img0.h) return null;
  let last = base;
  if (mip) {
    const maxDim = Math.max(img0.w, img0.h, type === '3D' ? img0.d : 1);
    const q = Math.min(base + Math.floor(Math.log2(maxDim)), immutable ? t.params.get('TEXTURE_IMMUTABLE_LEVELS') - 1 : Infinity, maxLevelParam);
    for (let level = base + 1; level <= q; level++) {
      const k = level - base;
      const want = [Math.max(1, img0.w >> k), Math.max(1, img0.h >> k), type === '3D' ? Math.max(1, img0.d >> k) : img0.d];
      for (let f = 0; f < faces; f++) { const o = t.images.get(texImageKey(f, level)); if (!o || o.w !== want[0] || o.h !== want[1] || o.d !== want[2] || o.fmt !== fmt) return null; }
    }
    last = q;
  }
  const wrapS = params.get('TEXTURE_WRAP_S'), wrapT = params.get('TEXTURE_WRAP_T'), wrapR = params.get('TEXTURE_WRAP_R');
  if (s.version !== 2 && (!texIsPot(img0.w) || !texIsPot(img0.h)) && (mip || wrapS !== GL.CLAMP_TO_EDGE || wrapT !== GL.CLAMP_TO_EDGE)) return null;
  const compare = fmt.depth && s.version === 2 && params.get('TEXTURE_COMPARE_MODE') === GL.COMPARE_REF_TO_TEXTURE;
  const nearestOnly = magFilter === GL.NEAREST && (minFilter === GL.NEAREST || minFilter === GL.NEAREST_MIPMAP_NEAREST);
  if (!nearestOnly && !compare && (!texFilterable(s, fmt) || (fmt.depth && s.version === 2))) return null;
  // Os níveis da vista, da base à última; o descritor repete o último além dela.
  const levels = [];
  for (let level = base; level <= last; level++) levels.push(Array.from({ length: faces }, (_, f) => t.images.get(texImageKey(f, level))));
  for (const lv of levels) for (const img of lv) glPin(s, img);
  const linear = (f) => f === GL.LINEAR || f === GL.LINEAR_MIPMAP_NEAREST || f === GL.LINEAR_MIPMAP_LINEAR;
  const magLinear = linear(magFilter), minLinear = linear(minFilter);
  const filter = magLinear ? (minLinear ? 'LINEAR' : 'MIN_POINT_MAG_LINEAR') : minLinear ? 'MIN_LINEAR_MAG_POINT' : 'POINT';
  const mipmapFilter = minFilter === GL.NEAREST_MIPMAP_LINEAR || minFilter === GL.LINEAR_MIPMAP_LINEAR ? 'LINEAR' : 'POINT';
  const clampLod = (x) => Math.min(Math.max(x, 0), 14);
  let minLod = mip ? clampLod(fr(params.get('TEXTURE_MIN_LOD') ?? -1000)) : 0, maxLod = mip ? clampLod(fr(params.get('TEXTURE_MAX_LOD') ?? 1000)) : 0.25;
  if (levels.length === 1 && filter !== 'MIN_POINT_MAG_LINEAR' && filter !== 'MIN_LINEAR_MAG_POINT') minLod = maxLod = 0;
  const wrap = (m) => (m === GL.REPEAT ? 'WRAP' : m === GL.MIRRORED_REPEAT ? 'MIRROR' : 'CLAMP');
  const compareFunc = params.get('TEXTURE_COMPARE_FUNC');
  return {
    type, fmt, levels, filter, mipmapFilter, minLod, maxLod, compare, compareFunc,
    modes: type === 'CUBE' ? ['SEAMLESS', 'SEAMLESS', 'CUBEFACE'] : [wrap(wrapS), wrap(wrapT), type === '3D' ? wrap(wrapR) : type === 'ARRAY' ? 'LAYER' : 'UNUSED'],
  };
};
// A textura incompleta do ANGLE: um texel (0, 0, 0, 1) do tipo do amostrador.
const texIncomplete = (samplerType) => {
  const fmt = /^u/.test(samplerType) ? TEX_FORMATS.RGBA8UI : /^i/.test(samplerType) ? TEX_FORMATS.RGBA8I : /Shadow$/.test(samplerType) ? TEX_FORMATS.DEPTH_COMPONENT32F : TEX_FORMATS.RGBA8;
  const texel = fmt.kind === 'unorm' ? [0, 0, 0, 0xff00] : fmt.kind === 'float' ? [0] : [0, 0, 0, 1];
  const type = /Cube/.test(samplerType) ? 'CUBE' : /3D/.test(samplerType) ? '3D' : /Array/.test(samplerType) ? 'ARRAY' : '2D';
  const img = { w: 1, h: 1, d: 1, fmt, data: Float64Array.from(texel) };
  return {
    type, fmt, levels: [Array.from({ length: type === 'CUBE' ? 6 : 1 }, () => img)], filter: 'POINT', mipmapFilter: 'POINT', minLod: 0, maxLod: 0,
    compare: /Shadow$/.test(samplerType), compareFunc: GL.LEQUAL, modes: type === 'CUBE' ? ['SEAMLESS', 'SEAMLESS', 'CUBEFACE'] : ['CLAMP', 'CLAMP', type === '3D' ? 'CLAMP' : type === 'ARRAY' ? 'LAYER' : 'UNUSED'],
  };
};
// As texturas de cada unidade no momento do desenho.
const glTextureSnapshot = (s) => s.units.map((u) => ({
  TEXTURE_2D: u.TEXTURE_2D && texView(s, u.TEXTURE_2D, 'TEXTURE_2D', u.sampler),
  TEXTURE_CUBE_MAP: u.TEXTURE_CUBE_MAP && texView(s, u.TEXTURE_CUBE_MAP, 'TEXTURE_CUBE_MAP', u.sampler),
  TEXTURE_3D: u.TEXTURE_3D && texView(s, u.TEXTURE_3D, 'TEXTURE_3D', u.sampler),
  TEXTURE_2D_ARRAY: u.TEXTURE_2D_ARRAY && texView(s, u.TEXTURE_2D_ARRAY, 'TEXTURE_2D_ARRAY', u.sampler),
}));

// ---- Os pontos de entrada do contexto.
const TEX_ARRAY_TYPES = {
  [GL.UNSIGNED_BYTE]: ['Uint8Array', 'Uint8ClampedArray'], [GL.BYTE]: ['Int8Array'], [GL.UNSIGNED_SHORT]: ['Uint16Array'], [GL.SHORT]: ['Int16Array'], [GL.UNSIGNED_INT]: ['Uint32Array'], [GL.INT]: ['Int32Array'],
  [GL.FLOAT]: ['Float32Array'], [GL.HALF_FLOAT]: ['Uint16Array'], [HALF_FLOAT_OES]: ['Uint16Array'], [GL.UNSIGNED_SHORT_5_6_5]: ['Uint16Array'], [GL.UNSIGNED_SHORT_4_4_4_4]: ['Uint16Array'], [GL.UNSIGNED_SHORT_5_5_5_1]: ['Uint16Array'],
  [GL.UNSIGNED_INT_2_10_10_10_REV]: ['Uint32Array'], [GL.UNSIGNED_INT_10F_11F_11F_REV]: ['Uint32Array'], [GL.UNSIGNED_INT_5_9_9_9_REV]: ['Uint32Array'], [GL.UNSIGNED_INT_24_8]: ['Uint32Array'], [GL.FLOAT_32_UNSIGNED_INT_24_8_REV]: [],
};
const texMaxSize = (s, target, is3D) => {
  const p = s.table.params;
  if (target >= GL.TEXTURE_CUBE_MAP_POSITIVE_X && target <= GL.TEXTURE_CUBE_MAP_NEGATIVE_Z) return [p.MAX_CUBE_MAP_TEXTURE_SIZE, p.MAX_CUBE_MAP_TEXTURE_SIZE, 1];
  if (target === GL.TEXTURE_3D) return [p.MAX_3D_TEXTURE_SIZE, p.MAX_3D_TEXTURE_SIZE, p.MAX_3D_TEXTURE_SIZE];
  if (target === GL.TEXTURE_2D_ARRAY) return [p.MAX_TEXTURE_SIZE, p.MAX_TEXTURE_SIZE, p.MAX_ARRAY_TEXTURE_LAYERS];
  return [p.MAX_TEXTURE_SIZE, p.MAX_TEXTURE_SIZE, is3D ? p.MAX_3D_TEXTURE_SIZE : 1];
};
// A fonte de um envio: confere o tipo do typed array, o tamanho e o buffer de desempacotamento; devolve
// { source, offset } ou o erro.
const texCheckSource = (s, dims, format, type, source, srcOffset) => {
  const pbo = s.params.get('PIXEL_UNPACK_BUFFER_BINDING');
  if (typeof source === 'number') {
    if (!pbo) return { error: GL.INVALID_OPERATION };
    const need = texSourceBytes(s, ...dims, format, type);
    if (source < 0) return { error: GL.INVALID_VALUE };
    if (source + need > objOf(pbo).data.byteLength) return { error: GL.INVALID_OPERATION };
    return { source: null, offset: source };
  }
  if (pbo) return { error: GL.INVALID_OPERATION };
  if (source === null || source === undefined) return { source: null };
  if (!ArrayBuffer.isView(source)) return { source };
  const allowed = TEX_ARRAY_TYPES[type] || [];
  if (!allowed.includes(Object.prototype.toString.call(source).slice(8, -1))) return { error: GL.INVALID_OPERATION };
  const offset = srcOffset || 0;
  const need = texSourceBytes(s, ...dims, format, type);
  if (offset * source.BYTES_PER_ELEMENT + need > source.byteLength) return { error: GL.INVALID_OPERATION };
  return { source, offset };
};
const texImage = (ctx, s, t, target, level, internalformat, size, border, format, type, source, srcOffset) => {
  if (t.params.get('TEXTURE_IMMUTABLE_FORMAT')) { glError(ctx, GL.INVALID_OPERATION); return; }
  let dims = size;
  if (!dims) { const d = sourceSize(source); if (!d) { glError(ctx, GL.INVALID_VALUE); return; } dims = [d[0], d[1], 1]; }
  const is3D = target === GL.TEXTURE_3D || target === GL.TEXTURE_2D_ARRAY;
  if (level < 0 || dims.some((x) => x < 0) || border !== 0) { glError(ctx, GL.INVALID_VALUE); return; }
  const max = texMaxSize(s, target, is3D);
  const levelMax = (m, i) => (i === 2 && target === GL.TEXTURE_2D_ARRAY ? m : m >> level);
  if (dims.some((x, i) => x > levelMax(max[i], i))) { glError(ctx, GL.INVALID_VALUE); return; }
  if (target >= GL.TEXTURE_CUBE_MAP_POSITIVE_X && target <= GL.TEXTURE_CUBE_MAP_NEGATIVE_Z && dims[0] !== dims[1]) { glError(ctx, GL.INVALID_VALUE); return; }
  if (s.version !== 2 && level > 0 && (!texIsPot(dims[0]) || !texIsPot(dims[1]))) { glError(ctx, GL.INVALID_VALUE); return; }
  const r = texResolveFormat(s, internalformat, format, type);
  if (r.error) { glError(ctx, r.error); return; }
  if (r.fmt.depth && (target === GL.TEXTURE_3D || (s.version !== 2 && level > 0))) { glError(ctx, GL.INVALID_OPERATION); return; }
  const src = size ? texCheckSource(s, dims, format, type, source, srcOffset) : { source };
  if (src.error) { glError(ctx, src.error); return; }
  if (r.fmt.depth && s.version !== 2 && src.source) { glError(ctx, GL.INVALID_OPERATION); return; }
  texUpload(ctx, s, { t, face: texFace(target), level, fmt: r.fmt, x0: 0, y0: 0, z0: 0, w: dims[0], h: dims[1], d: dims[2], format, type, source: src.source, offset: src.offset, sub: false });
  if (texFace(target) === 0) t.levels.set(level, [dims[0], dims[1]]);
};
// O par formato/tipo de um envio parcial tem de servir ao formato guardado.
const texCompatible = (s, fmt, format, type) => {
  if (s.version === 2 && TEX_SIZED[fmt.name]) {
    const formatName = Object.keys(TEX_FORMAT_COMPONENTS).find((k) => GL[k] === format), typeName = Object.keys(TEX_TYPES).find((k) => GL[k] === type);
    if (TEX_SIZED[fmt.name].some(([fm, ty]) => fm === formatName && ty === typeName)) return true;
  }
  return texUnsized(s, format, type) === fmt.name;
};
const texSubImage = (ctx, s, t, target, level, offsets, size, format, type, source, srcOffset) => {
  const face = texFace(target);
  const img = t.images && t.images.get(texImageKey(face, level));
  let dims = size;
  if (!dims) { const d = sourceSize(source); if (!d) { glError(ctx, GL.INVALID_VALUE); return; } dims = [d[0], d[1], 1]; }
  if (level < 0 || dims.some((x) => x < 0) || offsets.some((x) => x < 0)) { glError(ctx, GL.INVALID_VALUE); return; }
  if (!img) { glError(ctx, GL.INVALID_OPERATION); return; }
  if (offsets[0] + dims[0] > img.w || offsets[1] + dims[1] > img.h || offsets[2] + dims[2] > img.d) { glError(ctx, GL.INVALID_VALUE); return; }
  if (!Object.keys(TEX_FORMAT_COMPONENTS).some((k) => GL[k] === format) && format !== 35904 && format !== 35906) { glError(ctx, GL.INVALID_ENUM); return; }
  if (!texTypeInfo(type)) { glError(ctx, GL.INVALID_ENUM); return; }
  if (!texCompatible(s, img.fmt, format, type)) { glError(ctx, GL.INVALID_OPERATION); return; }
  const src = size ? texCheckSource(s, dims, format, type, source, srcOffset) : { source };
  if (src.error) { glError(ctx, src.error); return; }
  if (size && src.source === null && src.offset === undefined) { glError(ctx, GL.INVALID_VALUE); return; }
  if (!dims[0] || !dims[1] || !dims[2]) return;
  texUpload(ctx, s, { t, face, level, fmt: img.fmt, x0: offsets[0], y0: offsets[1], z0: offsets[2], w: dims[0], h: dims[1], d: dims[2], format, type, source: src.source, offset: src.offset, sub: true });
};
const texStorage = (ctx, target, levels, internalformat, dims, is3D) => {
  const s = glState(ctx);
  if (s.lost) return;
  const name = enumName(s, target, is3D ? ['TEXTURE_3D', 'TEXTURE_2D_ARRAY'] : ['TEXTURE_2D', 'TEXTURE_CUBE_MAP']);
  if (!name) { glError(ctx, GL.INVALID_ENUM); return; }
  const texture = boundTexture(s, name);
  if (!texture) { glError(ctx, GL.INVALID_OPERATION); return; }
  const t = objOf(texture);
  if (t.params.get('TEXTURE_IMMUTABLE_FORMAT')) { glError(ctx, GL.INVALID_OPERATION); return; }
  const fmtName = Object.keys(TEX_SIZED).find((k) => GL[k] === internalformat);
  if (!fmtName) { glError(ctx, GL.INVALID_ENUM); return; }
  if (levels < 1 || dims.some((x) => x < 1)) { glError(ctx, GL.INVALID_VALUE); return; }
  const third = name === 'TEXTURE_3D';
  if (levels > Math.floor(Math.log2(Math.max(dims[0], dims[1], third ? dims[2] : 1))) + 1) { glError(ctx, GL.INVALID_OPERATION); return; }
  const fmt = TEX_FORMATS[fmtName];
  let [w, h, d] = dims;
  for (let level = 0; level < levels; level++) {
    for (let f = 0; f < (name === 'TEXTURE_CUBE_MAP' ? 6 : 1); f++) texSetImage(t, f, level, texNewImage(fmt, w, h, d));
    t.levels.set(level, [w, h]);
    w = Math.max(1, w >> 1); h = Math.max(1, h >> 1); if (third) d = Math.max(1, d >> 1);
  }
  t.params.set('TEXTURE_IMMUTABLE_FORMAT', true);
  t.params.set('TEXTURE_IMMUTABLE_LEVELS', levels);
};
// copyTexImage2D e copyTexSubImage2D a partir do buffer de desenho (RGBA de 8 bits, linha 0 embaixo); fora dele,
// zero.
const texCopyFromFramebuffer = (s, img, x0, y0, z0, x, y, w, h) => {
  const pixels = glFlush(s);
  const tw = s.target.width, th = s.target.height;
  const pick = { LUMINANCE: [0], ALPHA: [3], LUMINANCE_ALPHA: [0, 3], RGB8: [0, 1, 2], R8: [0], RG8: [0, 1] }[img.fmt.name] || [0, 1, 2, 3];
  const store = texStorer(img.fmt, GL.UNSIGNED_BYTE), px = new Array(pick.length);
  for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) {
    const sx = x + i, sy = y + j;
    if (sx < 0 || sy < 0 || sx >= tw || sy >= th) continue;
    const o = (sy * tw + sx) * 4;
    for (let k = 0; k < pick.length; k++) px[k] = pick[k] === 3 && !s.attrs.alpha ? 255 : pixels[o + pick[k]];
    store(px, 0, px.length, img.data, (z0 * img.h + y0 + j) * img.w + x0 + i);
  }
};

// ---- O SamplerCore.
const SAMPLER_TARGET = (type) => (/Cube/.test(type) ? 'TEXTURE_CUBE_MAP' : /3D/.test(type) ? 'TEXTURE_3D' : /Array/.test(type) ? 'TEXTURE_2D_ARRAY' : 'TEXTURE_2D');
const mulHigh = (a, b) => Math.floor((a * b) / 65536) & 0xffff;
const mulHighS = (a, b) => Math.floor((((a << 16) >> 16) * ((b << 16) >> 16)) / 65536) & 0xffff;
const truncInt = (x) => (Number.isFinite(x) ? Math.trunc(x) | 0 : x > 0 ? 0x7fffffff : -0x80000000);
// O fator do sampleTexel para 1.0 de cada componente (getComponentScale).
const texScale = (fmt, c) => { const bits = fmt.kind === 'unorm' || fmt.kind === 'snorm' ? fmt.bits[Math.min(c, fmt.bits.length - 1)] : 16; return ((0xffff << (16 - bits)) & 0xffff) & (fmt.kind === 'snorm' ? 0x7fff : 0xffff); };
// O nível de um índice de lod (o descritor repete o último).
const texLevel = (view, i) => view.levels[Math.min(Math.max(i, 0), view.levels.length - 1)];
// log2(sqrt(x)) e log2(x) pelo truque do expoente, em binário32.
const LOD_BIAS = fr(0x3f800000);
const log2sqrt = (x) => { const sq = fr(x * x); return fr(fr(fr(f32Bits(sq)) - LOD_BIAS) * 2 ** -25); };
const log2Lod = (x) => { const sq = fr(x * x); return fr(fr(fr(f32Bits(sq)) - LOD_BIAS) * 2 ** -24); };
const f32View = new DataView(new ArrayBuffer(4));
const f32Bits = (x) => { f32View.setFloat32(0, x); return f32View.getInt32(0); };
const bitsF32 = (i) => { f32View.setInt32(0, i); return f32View.getFloat32(0); };

// A face e as coordenadas (U, V) de uma direção do cubo, e o eixo maior M (cubeFace).
const cubeFace = (x, y, z) => {
  const ax = Math.abs(x), ay = Math.abs(y), az = Math.abs(z);
  const xy = ax > ay, yz = ay > az, zx = az > ax;
  const xMajor = xy && !zx, yMajor = yz && !xy, zMajor = zx && !yz;
  const neg = (xMajor && x < 0) || (yMajor && y < 0) || (zMajor && z < 0);
  const face = (neg ? 1 : 0) | (yMajor ? 2 : 0) | (zMajor ? 4 : 0);
  const flip = (v) => (neg ? -v : v);
  const U = xMajor ? flip(-z) : zMajor ? flip(x) : x;
  const V = !yMajor ? -y : flip(z);
  const M = fr(fr(1 / Math.max(ax, ay, az)) * 0.5);
  return { face, U: fr(fr(U * M) + 0.5), V: fr(fr(V * M) + 0.5) };
};

// As bordas de um nível do cubo (updateBorders do SwiftShader): cada face ganha uma moldura de 1 texel copiada das
// vizinhas e os cantos com a média dos três texels ao lado.
const cubeBorders = (faces) => {
  if (faces.bordered) return faces.bordered;
  const n = faces[0].w, fmt = faces[0].fmt, W = n + 2, k = fmt.comps;
  const out = faces.map((img) => { const data = new Float64Array(W * W * k); for (let y = 0; y < n; y++) data.set(img.data.subarray(y * n * k, (y + 1) * n * k), ((y + 1) * W + 1) * k); return { w: W, h: W, d: 1, fmt, data }; });
  const [posX, negX, posY, negY, posZ, negZ] = [0, 1, 2, 3, 4, 5];
  const TOP = 0, BOTTOM = 1, LEFT = 2, RIGHT = 3;
  const copy = (dst, dstEdge, src, srcEdge) => {
    const reverse = srcEdge === dstEdge || (srcEdge === TOP && dstEdge === RIGHT) || (srcEdge === RIGHT && dstEdge === TOP) || (srcEdge === BOTTOM && dstEdge === LEFT) || (srcEdge === LEFT && dstEdge === BOTTOM);
    const srcH = srcEdge === TOP || srcEdge === BOTTOM, dstH = dstEdge === TOP || dstEdge === BOTTOM;
    let sx = srcEdge === RIGHT ? n - 1 : 0, sy = srcEdge === BOTTOM ? n - 1 : 0;
    let dx = dstEdge === RIGHT ? n : -1, dy = dstEdge === BOTTOM ? n : -1;
    if (dstH) dx += reverse ? n : 1; else dy += reverse ? n : 1;
    for (let i = 0; i < n; i++) {
      out[dst].data.set(faces[src].data.subarray((sy * n + sx) * k, (sy * n + sx + 1) * k), ((dy + 1) * W + dx + 1) * k);
      if (srcH) sx++; else sy++;
      const step = reverse ? -1 : 1;
      if (dstH) dx += step; else dy += step;
    }
  };
  copy(posX, BOTTOM, negY, RIGHT); copy(posY, BOTTOM, posZ, TOP); copy(posZ, BOTTOM, negY, TOP); copy(negX, BOTTOM, negY, LEFT); copy(negY, BOTTOM, negZ, BOTTOM); copy(negZ, BOTTOM, negY, BOTTOM);
  copy(posX, TOP, posY, RIGHT); copy(posY, TOP, negZ, TOP); copy(posZ, TOP, posY, BOTTOM); copy(negX, TOP, posY, LEFT); copy(negY, TOP, posZ, BOTTOM); copy(negZ, TOP, posY, TOP);
  copy(posX, RIGHT, negZ, LEFT); copy(posY, RIGHT, posX, TOP); copy(posZ, RIGHT, posX, LEFT); copy(negX, RIGHT, posZ, LEFT); copy(negY, RIGHT, posX, BOTTOM); copy(negZ, RIGHT, negX, LEFT);
  copy(posX, LEFT, posZ, RIGHT); copy(posY, LEFT, negX, TOP); copy(posZ, LEFT, negX, RIGHT); copy(negX, LEFT, negZ, RIGHT); copy(negY, LEFT, negX, BOTTOM); copy(negZ, LEFT, posX, RIGHT);
  const third = fr(1 / 3);
  for (const { data } of out) {
    const at = (x, y, c) => blitRead(fmt, c, data[((y + 1) * W + x + 1) * k + c]);
    const corner = (x0, x1, y0, y1) => {
      for (let c = 0; c < k; c++) data[((y0 + 1) * W + x0 + 1) * k + c] = blitWrite(fmt, c, fr(fr(fr(at(x0, y1, c) + at(x1, y0, c)) + at(x1, y1, c)) * third));
    };
    corner(-1, 0, -1, 0); corner(-1, 0, n, n - 1); corner(n, n - 1, -1, 0); corner(n, n - 1, n, n - 1);
  }
  faces.bordered = out;
  return out;
};

// O texel de um nível em (x, y, z); no cubo, `z` é a face e (x, y) já contam a moldura (o endereçamento SEAMLESS soma
// 1, como o ponteiro do descritor no canto da moldura).
const texTexel = (view, level, x, y, z) => {
  if (view.type === 'CUBE') { const b = cubeBorders(level)[z]; return texGet(b, y * b.w + x); }
  const img = level[0];
  return texGet(img, (z * img.h + y) * img.w + x);
};

// address() de 16 bits: a coordenada em fração de 16 bits.
const address16 = (u, mode) => {
  if (mode === 'UNUSED') return 0;
  if (mode === 'CLAMP') return truncInt(fr(Math.min(Math.max(u, 0), 65535 / 65536) * 65536)) & 0xffff;
  if (mode === 'MIRROR') { let v = truncInt(fr(u * 65536)); const m = (v << 15) >> 31; v ^= m; return v & 0xffff; }
  return truncInt(fr(u * 65536)) & 0xffff;
};
// computeIndices + sampleTexel de 16 bits: os componentes no ponto fixo do formato.
const texel16 = (view, level, u, v, w, layer) => {
  const img = level[0];
  const x = mulHigh(u, img.w), y = view.type === '2D' || view.type === '3D' || view.type === 'ARRAY' ? mulHigh(v, img.h) : 0;
  const z = view.type === '3D' ? mulHigh(w, img.d) : view.type === 'ARRAY' ? layer : 0;
  let texel = texGet(img, (z * img.h + y) * img.w + x);
  if (view.fmt.srgb) texel = texel.map((c, i) => (i < 3 ? SRGB_FF00[c >> 8] : c));
  return texel;
};
const SRGB_FF00 = Array.from({ length: 256 }, (_, i) => {
  const c = fr(i / 255);
  const lin = c <= fr(0.04045) ? fr(c / fr(12.92)) : fr(Math.pow(fr(fr(c + fr(0.055)) / fr(1.055)), fr(2.4)));
  return Math.trunc(fr(fr(lin * 0xff00) + 0.5));
});

// O POINT de 16 bits (sampleQuad2D/sample3D com FILTER_POINT): o texel do endereço em fração de 16 bits.
const sampleLevel16 = (view, level, u, v, w, layer) => {
  const [mu, mv, mw] = view.modes;
  return texel16(view, level, address16(u, mu), address16(v, mv), view.type === '3D' ? address16(w, mw) : 0, layer);
};

// address() de 32 bits: o texel inteiro dos dois lados e a fração.
const address32 = (coord, mode, dim, view, lod, fetch) => {
  if (mode === 'UNUSED') return [0, 0, 0];
  if (fetch) { const x = coord | 0; const c = Math.min(Math.max(x, 0), dim - 1); return [c === x ? c : -1, c === x ? c : -1, 0]; }
  if (mode === 'CUBEFACE') return [coord, coord, 0];
  if (mode === 'LAYER') return [0, 0, 0];
  const one = bitsF32(0x3f7fffff);
  let c = coord;
  if (mode === 'CLAMP' || mode === 'SEAMLESS') c = Math.min(Math.max(c, 0), one);
  else if (mode === 'MIRROR') { c = fr(c * 0.5); c = fr(2 * Math.abs(fr(c - roundEven(c)))); c = Math.min(c, one); }
  else c = fr(c - Math.floor(c));
  c = fr(c * dim);
  if (view.filter === 'POINT') { let x = truncInt(c); if (mode === 'SEAMLESS') x += 1; return [x, x, 0]; }
  let filterOn = true;
  if (view.filter === 'MIN_LINEAR_MAG_POINT') filterOn = lod > 0;
  if (view.filter === 'MIN_POINT_MAG_LINEAR') filterOn = lod <= 0;
  if (view.filter === 'MIN_POINT_MAG_LINEAR' || view.filter === 'MIN_LINEAR_MAG_POINT') c = fr(c - (filterOn ? 0.5 : 0)); else c = fr(c - 0.5);
  const fl = Math.floor(c);
  let x0 = truncInt(fl);
  const f = fr(c - fl);
  if (mode === 'SEAMLESS') x0 += 1;
  let x1 = x0 + (filterOn ? 1 : 0);
  if (mode === 'SEAMLESS') return [x0, x1, f];
  if (mode === 'WRAP') { if (x0 < 0) x0 = dim - 1; if (!(x1 < dim)) x1 = 0; }
  else { x0 = Math.max(x0, 0); x1 = Math.min(x1, dim - 1); }
  return [x0, x1, f];
};
// Um texel do caminho de 32 bits: componentes em binário32 (os normalizados no ponto fixo, sem escala), com a borda
// transparente para coordenada fora (texelFetch) e a comparação de profundidade.
const texel32 = (view, level, x, y, z, layer, dref) => {
  const fmt = view.fmt;
  let c;
  if (x < 0 || y < 0 || z < 0) c = [0, 0, 0, 0];
  else {
    const raw = view.type === 'CUBE' ? texTexel(view, level, x, y, z) : texTexel(view, level, x, y, view.type === 'ARRAY' ? layer : z);
    c = raw.map((v) => (fmt.kind === 'snorm' ? (v << 16) >> 16 : v));
    if (fmt.srgb) c = c.map((v, i) => (i < 3 ? SRGB_FF00[v >> 8] : v));
    while (c.length < 4) c.push(0);
  }
  if (view.compare) {
    let ref = dref, d = c[0];
    if (fmt.kind !== 'float') { ref = Math.min(Math.max(ref, 0), 1); d = fr(d * fr(1 / 0xffff)); }
    const ops = { [GL.LEQUAL]: ref <= d, [GL.GEQUAL]: ref >= d, [GL.LESS]: ref < d, [GL.GREATER]: ref > d, [GL.EQUAL]: ref === d, [GL.NOTEQUAL]: ref !== d, [GL.ALWAYS]: true, [GL.NEVER]: false };
    return [ops[view.compareFunc] ? 1 : 0, 0, 0, 1];
  }
  return c;
};
const sampleLevel32 = (view, level, u, v, w, layer, dref, lod, fetch) => {
  const img = level[0];
  const [mu, mv, mw] = view.modes;
  const [x0, x1, fu] = address32(u, mu, img.w, view, lod, fetch);
  const [y0, y1, fv] = address32(v, mv, img.h, view, lod, fetch);
  const z = view.type === '3D' ? address32(w, mw, img.d, view, lod, fetch) : view.type === 'CUBE' ? [w, w, 0] : [0, 0, 0];
  const lay = view.type === 'ARRAY' ? (fetch ? (() => { const a = layer | 0; return a >= 0 && a < img.d ? a : -1; })() : layer) : 0;
  if (view.filter === 'POINT' || fetch) return texel32(view, level, x0, y0, z[0], lay < 0 ? 0 : lay, dref).map((c) => (lay < 0 ? 0 : c));
  const lerp = (a, b, f) => a.map((x, i) => fr(x + fr(f * fr(b[i] - x))));
  const slice = (zz) => {
    const c00 = texel32(view, level, x0, y0, zz, lay, dref), c10 = texel32(view, level, x1, y0, zz, lay, dref);
    const c01 = texel32(view, level, x0, y1, zz, lay, dref), c11 = texel32(view, level, x1, y1, zz, lay, dref);
    return lerp(lerp(c00, c10, fu), lerp(c01, c11, fu), fv);
  };
  if (view.type === '3D') return lerp(slice(z[0]), slice(z[1]), z[2]);
  return slice(z[0]);
};

// sampleTexture128 para uma faixa (lane) do quad: `coords` são as coordenadas das quatro faixas (para o lod implícito),
// `own` as desta faixa.
const texSample = (view, method, own, quad, lodOrBias, dref) => {
  const fmt = view.fmt;
  let [u, v, w] = own;
  const a = view.type === 'ARRAY' ? own[2] : 0;
  if (view.type === 'CUBE') { const cf = cubeFace(own[0], own[1], own[2]); u = cf.U; v = cf.V; w = cf.face; }
  const singleMip = view.minLod === view.maxLod;
  let lod;
  if (singleMip && method !== 'Query') lod = view.minLod;
  else if (method === 'Implicit' || method === 'Bias') {
    const base = view.levels[0][0];
    if (!quad) lod = -Infinity;
    else if (view.type === 'CUBE') {
      // computeLodCube: cada faixa multiplica a direção pelo seu 0,5 / eixo maior; vale a maior soma de Manhattan
      // em dois eixos das faixas 1 e 2 contra a 0.
      const P = quad.map((q) => { const m = fr(fr(1 / Math.max(Math.abs(q[0]), Math.abs(q[1]), Math.abs(q[2]))) * 0.5); return [fr(q[0] * m), fr(q[1] * m), fr(q[2] * m)]; });
      const d = [0, 1, 2].map((k) => P.map((p) => Math.abs(fr(p[k] - P[0][k]))));
      const best = [1, 2].map((lane) => Math.max(fr(d[0][lane] + d[1][lane]), fr(d[0][lane] + d[2][lane]), fr(d[1][lane] + d[2][lane])));
      lod = log2Lod(fr(Math.max(best[0], best[1]) * base.w));
    } else if (view.type === '3D') {
      const dims = [base.w, base.h, base.d];
      const sum = [1, 2].map((lane) => [0, 1, 2].reduce((acc, k) => { const x = fr(fr(quad[lane][k] - quad[0][k]) * dims[k]); return fr(acc + fr(x * x)); }, 0));
      lod = log2sqrt(Math.max(sum[0], sum[1]));
    } else {
      const dudx = fr(fr(quad[1][0] - quad[0][0]) * base.w), dudy = fr(fr(quad[2][0] - quad[0][0]) * base.w);
      const dvdx = fr(fr(quad[1][1] - quad[0][1]) * base.h), dvdy = fr(fr(quad[2][1] - quad[0][1]) * base.h);
      const x = fr(fr(dudx * dudx) + fr(dvdx * dvdx)), y = fr(fr(dudy * dudy) + fr(dvdy * dvdy));
      lod = log2sqrt(Math.max(x, y));
    }
    let bias = 0;
    if (method === 'Bias') bias = fr(Math.min(Math.max(fr(bias + lodOrBias), -15), 15));
    lod = fr(lod + bias);
  } else if (method === 'Lod') lod = fr(lodOrBias);
  else if (method === 'Fetch') lod = Math.min(Math.max(lodOrBias | 0, view.minLod), view.maxLod);
  else lod = 0;
  if (method !== 'Fetch' && !(singleMip && method !== 'Query')) lod = Math.min(Math.max(lod, view.minLod), view.maxLod);
  // O ANGLE pede filtragem de alta precisão (VK_GOOGLE_sampler_filtering_precision): todo filtro que não é POINT vai
  // pelo caminho de 32 bits, como float, inteiro, cubo, comparação e texelFetch.
  const use32 = fmt.kind === 'float' || fmt.kind === 'uint' || fmt.kind === 'int' || view.type === 'CUBE' || view.compare || method === 'Fetch' || view.filter !== 'POINT';
  const selectLevel = (second) => {
    if (method === 'Fetch') return texLevel(view, lod);
    const ilod = view.mipmapFilter === 'POINT' ? roundEven(lod) : Math.trunc(lod);
    return texLevel(view, ilod + (second ? 1 : 0));
  };
  let c;
  if (use32) {
    c = sampleLevel32(view, selectLevel(false), u, v, w, a, dref, lod, method === 'Fetch');
    if (method !== 'Fetch' && view.mipmapFilter === 'LINEAR') {
      const cc = sampleLevel32(view, selectLevel(true), u, v, w, a, dref, lod, false);
      const f = fr(lod - Math.floor(lod));
      c = c.map((x, i) => fr(fr(fr(cc[i] - x) * f) + x));
    }
  } else {
    const layer = view.type === 'ARRAY' ? Math.min(Math.max(roundEven(a), 0), view.levels[0][0].d - 1) : 0;
    let cs = sampleLevel16(view, selectLevel(false), u, v, w, layer);
    if (view.mipmapFilter === 'LINEAR') {
      const cc = sampleLevel16(view, selectLevel(true), u, v, w, layer);
      const utri = truncInt(fr(lod * 65536)) & 0xffff;
      if (fmt.kind === 'snorm') { const stri = utri >> 1; cs = cs.map((x, i) => { const sum = (mulHighS(x, 0x7fff - stri) + mulHighS(cc[i], stri)) & 0xffff; return (sum * 2) & 0xffff; }); }
      else cs = cs.map((x, i) => (mulHigh(x, ~utri & 0xffff) + mulHigh(cc[i], utri)) & 0xffff);
    }
    c = cs.map((x) => (fmt.kind === 'snorm' ? (x << 16) >> 16 : x));
  }
  while (c.length < 4) c.push(0);
  if ((fmt.kind === 'unorm' || fmt.kind === 'snorm') && !view.compare) c = c.map((x, i) => fr(x * fr(1 / texScale(fmt, i))));
  if (fmt.kind === 'snorm') c = c.map((x) => Math.max(x, -1));
  if (view.compare) return c;
  return fmt.swizzle.map((sw) => (sw === 'zero' ? 0 : sw === 'one' ? 1 : c[sw]));
};
