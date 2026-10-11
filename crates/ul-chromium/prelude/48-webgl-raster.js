// ---- Rasterização do WebGL como o SwiftShader do oráculo (ANGLE sobre Vulkan): os comandos que mexem nos pixels
// (`clear`, `drawArrays`, `drawElements`) entram numa fila com o estado do momento e só rasterizam quando a página lê
// os pixels (`readPixels`, `toDataURL`, `drawImage`, textura de canvas). Um `clear` que cobre tudo descarta o que veio
// antes, então a fila de uma página que desenha a cada quadro e nunca lê fica curta.
//
// O pipeline: o sombreador de vértice roda uma vez por índice; triângulos são recortados contra os planos próximo e
// distante e contra w > 0, passam pela janela de visualização, têm os vértices presos à grade de 1/16 de pixel (4 bits
// de subpixel, como o SwiftShader: medido com faixas de largura conhecida no MSAA) e cobrem uma amostra quando ela cai
// dentro pela regra de canto superior esquerdo do
// Vulkan. Com `antialias` (o padrão) o buffer tem 4 amostras por pixel no padrão do Vulkan e o sombreador de fragmento
// roda uma vez por pixel, no centro; a resolução tira a média das amostras com arredondamento, como o resolve do
// SwiftShader. Cores são `float` convertidas para 8 bits com arredondamento ao mais próximo.

const SAMPLES_4X = [[0.375, 0.125], [0.875, 0.375], [0.125, 0.625], [0.625, 0.875]];
const SAMPLES_1X = [[0.5, 0.5]];

// O alvo de desenho do buffer padrão: cor por amostra, profundidade e estêncil por amostra, mais a cor resolvida.
const glTarget = (s) => {
  const w = s.canvas.width, h = s.canvas.height;
  let t = s.target;
  if (!t || t.width !== w || t.height !== h) {
    const samples = s.attrs.antialias ? SAMPLES_4X : SAMPLES_1X;
    const n = samples.length;
    t = s.target = { width: w, height: h, samples, color: new Uint8Array(w * h * 4 * n), depth: s.attrs.depth ? new Float32Array(w * h * n).fill(1) : null, stencil: s.attrs.stencil ? new Uint8Array(w * h * n) : null, resolved: null, max: BITS_8_MAX, alpha: s.attrs.alpha, float: null, depthQuant: DEPTH_EXACT };
    glDropQueue(s);
  }
  return t;
};

// ---- Framebuffers. O SwiftShader guarda cada formato com os bits dele (RGBA4 lido de volta dá múltiplos de 17), e o
// ANGLE não inverte o y fora do buffer padrão: o alvo de um framebuffer guarda as linhas espelhadas (a de cima do GL
// primeiro, como o renderbuffer) e o desenho entra espelhado, para o espaço do Vulkan do rasterizador coincidir com o
// do GL.
const BITS_8_MAX = [255, 255, 255, 255];
const GL_COLOR_BITS = { RGBA4: [4, 4, 4, 4], RGB5_A1: [5, 5, 5, 1], RGB565: [5, 6, 5, 0], RGBA8: [8, 8, 8, 8], RGB8: [8, 8, 8, 0] };
// Um valor de `bits` bits expandido para 8, como a conversão de leitura do ANGLE.
const expandTo8 = (q, bits) => (bits === 8 ? q : bits === 0 ? 255 : bits === 1 ? q * 255 : bits === 4 ? q * 17 : (q << (8 - bits)) | (q >> (2 * bits - 8)));
// Cor em ponto flutuante: nada limita a [0, 1], e cada canal guarda os bits do binário32 do valor arredondado ao
// formato (o R11F_G11F_B10F trunca a mantissa e leva negativo a zero; o canal ausente guarda 0, o alfa 1). O half do
// SwiftShader não confere o estouro: de 65520 a 131072 o expoente 31 fica com a mantissa, um NaN que a leitura devolve
// com a carga (0x7c06 vira 0x7f80c000), e os bits guardados preservam isso.
const FLOAT_MAX = [1, 1, 1, 1];
const renderHalf = (x) => {
  const bits = f32Bits(x), sign = (bits >>> 16) & 0x8000, abs = bits & 0x7fffffff;
  if (abs >= 0x38800000 && abs < 0x48000000) { const r = abs - 0x38000000; return sign | ((r + 0xfff + ((r >>> 13) & 1)) >>> 13); }
  return floatToHalf(x);
};
const halfBits = (h) => ((h >> 10) & 31) === 31 ? (((h & 0x8000) << 16) | 0x7f800000 | ((h & 1023) << 13)) >>> 0 : f32Bits(halfToFloat(h)) >>> 0;
const glFloatQuantizers = (fmt) => [0, 1, 2, 3].map((c) => {
  if (c >= fmt.comps || (fmt.alphaOne && c === 3)) { const b = f32Bits(c === 3 ? 1 : 0) >>> 0; return () => b; }
  if (fmt.half) return (v) => halfBits(renderHalf(v));
  if (fmt.packedFloat) { const m = c === 2 ? 5 : 6; return (v) => f32Bits(smallFloat(toSmallFloat(v, m), m)) >>> 0; }
  return (v) => f32Bits(v) >>> 0;
});
// DEPTH_COMPONENT16: o z de cada amostra e o guardado são roundEven(z * 65535) em binário32, sobre 65535 (medido
// no oráculo; o truncamento erra as faixas).
const D16_QUANT = (z) => fr(roundEven(fr(z * 65535)) / 65535);
const DEPTH_EXACT = (z) => z;
// O alvo de um framebuffer completo a partir dos anexos fotografados no comando, ou `null` se o formato de cor ainda
// não tem caminho de escrita (cor em ponto flutuante).
const glFramebufferTarget = (attachments) => {
  const att = (...names) => names.map((n) => attachments.get(GL[n])).find(Boolean);
  const color = att('COLOR_ATTACHMENT0'), depth = att('DEPTH_ATTACHMENT', 'DEPTH_STENCIL_ATTACHMENT'), stencil = att('STENCIL_ATTACHMENT', 'DEPTH_STENCIL_ATTACHMENT');
  const any = color || depth || stencil, first = objOf(any.object);
  const [w, h] = first.size || first.levels.get(any.level);
  const t = { width: w, height: h, samples: SAMPLES_1X, upright: true, color: null, depth: null, stencil: null, max: BITS_8_MAX, alpha: true, bits: [8, 8, 8, 8], image: null, float: null, depthQuant: DEPTH_EXACT };
  if (color) {
    const o = objOf(color.object);
    if (o.size) {
      const bits = GL_COLOR_BITS[o.formatName] || [8, 8, 8, 8];
      Object.assign(t, { color: o.color, bits, max: bits.map((b) => (b ? (1 << b) - 1 : 255)), alpha: bits[3] > 0 });
    } else {
      const img = o.images.get(texImageKey(texFace(color.textarget), color.level));
      if (img.fmt.depth || (img.fmt.kind !== 'unorm' && img.fmt.kind !== 'float')) return null;
      const n = img.fmt.comps;
      if (img.fmt.kind === 'float') {
        Object.assign(t, { image: img, float: glFloatQuantizers(img.fmt), color: new Uint32Array(w * h * 4), bits: [0, 1, 2, 3].map((c) => (c < n ? 32 : 0)), max: FLOAT_MAX, alpha: n === 4 && !img.fmt.alphaOne });
        const absent = t.float.map((q) => q(0));
        // O número do JS não guarda a carga do NaN: a imagem leva a parte os bits de cada NaN que o desenho escreveu.
        const nan = img.nanBits;
        for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let c = 0; c < 4; c++) {
          const i = (y * w + x) * n + c, v = c < n ? img.data[i] : 0;
          t.color[((h - 1 - y) * w + x) * 4 + c] = c >= n ? absent[c] : Number.isNaN(v) && nan && nan.has(i) ? nan.get(i) : f32Bits(v) >>> 0;
        }
      } else {
        const bits = [0, 1, 2, 3].map((c) => (c < n && !(img.fmt.alphaOne && c === 3) ? img.fmt.bits[c] : 0));
        Object.assign(t, { image: img, color: new Uint8Array(w * h * 4), bits, max: bits.map((b) => (b ? (1 << b) - 1 : 255)), alpha: bits[3] > 0 });
        for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let c = 0; c < 4; c++) {
          t.color[((h - 1 - y) * w + x) * 4 + c] = bits[c] ? img.data[(y * w + x) * n + c] >> (16 - bits[c]) : 255;
        }
      }
    }
  }
  if (depth) {
    const o = objOf(depth.object);
    t.depth = o.depth;
    if (o.formatName === 'DEPTH_COMPONENT16') t.depthQuant = D16_QUANT;
  }
  if (stencil) t.stencil = objOf(stencil.object).stencil;
  return t;
};
// Devolve a cor do alvo à textura anexada.
const glStoreTarget = (t) => {
  const img = t.image, n = img.fmt.comps, { width: w, height: h } = t;
  if (t.float) img.nanBits = null;
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let c = 0; c < n; c++) {
    const v = t.color[((h - 1 - y) * w + x) * 4 + c], i = (y * w + x) * n + c;
    if (t.float) {
      img.data[i] = f32FromBits(v);
      if (Number.isNaN(img.data[i])) (img.nanBits ??= new Map()).set(i, v);
    } else if (t.bits[c]) img.data[i] = (v << (16 - t.bits[c])) & 0xffff;
  }
};
// O readPixels de um framebuffer: roda a fila e devolve RGBA com a linha 0 embaixo (8 bits, ou os bits do binário32
// num anexo de ponto flutuante, com `float` marcado).
const glReadFramebuffer = (s, f) => {
  glFlush(s);
  const t = glFramebufferTarget(f.attachments);
  if (!t || !t.color) return null;
  const { width: w, height: h } = t, pixels = t.float ? new Uint32Array(w * h * 4) : new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) for (let c = 0; c < 4; c++) {
    const v = t.color[((h - 1 - y) * w + x) * 4 + c];
    pixels[(y * w + x) * 4 + c] = t.float ? v : expandTo8(v, t.bits[c]);
  }
  return { pixels, width: w, height: h, float: !!t.float };
};
// A fotografia do framebuffer de desenho no comando, com a imagem de cor fixada.
const glDrawFramebuffer = (s) => {
  const fb = s.params.get('DRAW_FRAMEBUFFER_BINDING');
  if (!fb) return null;
  const attachments = new Map(objOf(fb).attachments);
  const color = attachments.get(GL.COLOR_ATTACHMENT0);
  if (color && color.level !== undefined) glPin(s, objOf(color.object).images.get(texImageKey(texFace(color.textarget), color.level)));
  return { attachments };
};

// O valor que um parâmetro tinha quando o comando entrou na fila.
const glSnapshot = (s) => new Map(s.params);

// Um buffer ou imagem lido por um desenho pendente não muda por baixo dele: antes da escrita, a fila roda (o
// resultado é o mesmo da execução imediata do Chromium). Copiar na escrita deixava a fila reter uma cópia inteira da
// textura por texSubImage até a página ler os pixels. O pino aponta para a fila que o guarda; descartar a fila
// (rodada ou apagada) a esvazia e solta de uma vez todos os seus pinos.
const glPin = (s, o) => { if (o) o.pinned = s.queue; };
const glDropQueue = (s) => { if (s.queue) s.queue.length = 0; s.queue = []; };
const glBeforeWrite = (s, o) => { if (o && o.pinned && o.pinned.length) glFlush(s); };

const glEnqueue = (s, entry) => {
  glTarget(s);
  // Um clear inteiro do buffer padrão apaga o que estava na fila, desde que nada dela vá para um framebuffer.
  if (entry.kind === 'clear' && !entry.fb && glClearsAll(s, entry) && s.queue.every((e) => !e.fb)) glDropQueue(s);
  s.queue.push(entry);
  if (!entry.fb) s.target.resolved = null;
};
const glClearsAll = (s, entry) => {
  const p = entry.params;
  if (p.get('SCISSOR_TEST')) return false;
  if (!(entry.mask & GL.COLOR_BUFFER_BIT) || !p.get('COLOR_WRITEMASK').every(Boolean)) return false;
  if (s.attrs.depth && (!(entry.mask & GL.DEPTH_BUFFER_BIT) || !p.get('DEPTH_WRITEMASK'))) return false;
  if (s.attrs.stencil && (!(entry.mask & GL.STENCIL_BUFFER_BIT) || (p.get('STENCIL_WRITEMASK') & 255) !== 255)) return false;
  return true;
};

const glQueueClear = (s, mask) => glEnqueue(s, { kind: 'clear', mask, params: glSnapshot(s), fb: glDrawFramebuffer(s) });

const glQueueDraw = (ctx, mode, first, count, instances, elements) => {
  const s = glState(ctx);
  const program = objOf(s.params.get('CURRENT_PROGRAM'));
  const attribs = s.vao.attribs.map((a) => { const b = a.buffer && objOf(a.buffer); glPin(s, b); return { ...a, data: b ? b.data : null }; });
  if (elements) glPin(s, elements.buffer);
  glEnqueue(s, {
    kind: 'draw', mode, first, count, instances, params: glSnapshot(s), fb: glDrawFramebuffer(s),
    program: { sources: program.sources, attribs: program.attribs, uniforms: program.uniforms, values: new Map(program.uniformValues), cache: program },
    attribs, current: s.current.slice(), textures: glTextureSnapshot(s),
    elements: elements ? { data: elements.buffer.data, type: elements.type, offset: elements.offset } : null,
  });
};

// Roda a fila e devolve os pixels resolvidos (RGBA de 8 bits, linha 0 embaixo).
const glFlush = (s) => {
  const t = glTarget(s);
  if (s.queue.length) {
    for (const entry of s.queue) {
      const target = entry.fb ? glFramebufferTarget(entry.fb.attachments) : t;
      if (!target) continue;
      (entry.kind === 'clear' ? glRunClear : glRunDraw)(s, target, entry);
      if (target.image) glStoreTarget(target);
    }
    glDropQueue(s);
    t.resolved = null;
  }
  if (!t.resolved) t.resolved = glResolve(t);
  return t.resolved;
};

// `float` para `max` (2^bits - 1) como o SwiftShader: o produto em binário32, arredondado ao par mais próximo (0,3 vira
// 76,5 em 8 bits e sai 76; 0,1 vira 25,5 e sai 26).
const unormN = (c, max) => (c >= 1 ? max : c > 0 ? roundEven(fr(c * max)) : 0);
// O clear vai pelo fastClear do Blitter: (max * c + 0,5) truncado, em binário32 (0,7 vira 178,5 e sai 179).
const unormClear = (c, max) => (c >= 1 ? max : c > 0 ? Math.trunc(fr(fr(c * max) + 0.5)) : 0);
// As linhas [y0, y1) de um retângulo do GL no alvo (espelhadas num framebuffer).
const glRows = (t, y0, y1) => (t.upright ? [t.height - y1, t.height - y0] : [y0, y1]);

const glRunClear = (s, t, entry) => {
  const p = entry.params;
  let [x0, y0, x1, y1] = [0, 0, t.width, t.height];
  if (p.get('SCISSOR_TEST')) { const b = p.get('SCISSOR_BOX'); x0 = Math.max(0, b[0]); y0 = Math.max(0, b[1]); x1 = Math.min(t.width, b[0] + b[2]); y1 = Math.min(t.height, b[1] + b[3]); }
  [y0, y1] = glRows(t, y0, y1);
  const n = t.samples.length;
  if (t.color && entry.mask & GL.COLOR_BUFFER_BIT) {
    const color = p.get('COLOR_CLEAR_VALUE');
    const mask = p.get('COLOR_WRITEMASK');
    const bytes = Array.from(color, (c, i) => (t.float ? t.float[i](c) : i === 3 && !t.alpha ? t.max[3] : unormClear(c, t.max[i])));
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < n; k++) { const i = ((y * t.width + x) * n + k) * 4; for (let c = 0; c < 4; c++) if (mask[c]) t.color[i + c] = bytes[c]; }
  }
  if (t.depth && entry.mask & GL.DEPTH_BUFFER_BIT && p.get('DEPTH_WRITEMASK')) {
    const d = t.depthQuant(Math.min(1, Math.max(0, p.get('DEPTH_CLEAR_VALUE'))));
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < n; k++) t.depth[(y * t.width + x) * n + k] = d;
  }
  if (t.stencil && entry.mask & GL.STENCIL_BUFFER_BIT) {
    const wm = p.get('STENCIL_WRITEMASK') & 255, v = p.get('STENCIL_CLEAR_VALUE') & wm;
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < n; k++) { const i = (y * t.width + x) * n + k; t.stencil[i] = (t.stencil[i] & ~wm) | v; }
  }
};

// A média das amostras de cada pixel, aos pares e arredondando para cima, como o resolve do SwiftShader.
const glResolve = (t) => {
  const n = t.samples.length;
  const out = new Uint8Array(t.width * t.height * 4);
  for (let p = 0; p < t.width * t.height; p++) for (let c = 0; c < 4; c++) {
    if (n === 1) { out[p * 4 + c] = t.color[p * 4 + c]; continue; }
    const base = p * n * 4 + c;
    const a = (t.color[base] + t.color[base + 4] + 1) >> 1;
    const b = (t.color[base + 8] + t.color[base + 12] + 1) >> 1;
    out[p * 4 + c] = (a + b + 1) >> 1;
  }
  return out;
};

// ---- Programas: os dois estágios compilados uma vez por ligação.
const glProgramStages = (entry) => {
  const cache = entry.program.cache;
  if (cache.compiled && cache.compiled.sources === entry.program.sources) return cache.compiled;
  const [vsSource, fsSource] = entry.program.sources;
  const vs = glslCompile(vsSource, 'vertex');
  const fs = glslCompile(fsSource, 'fragment');
  const varyings = [];
  for (const [name, info] of fs.globals) {
    if (info.qualifier !== 'in') continue;
    const out = vs.globals.get(name);
    if (out && out.qualifier === 'out') varyings.push({ name, vs: out.slot, fs: info.slot, flat: info.flat || out.flat, t: info.t });
  }
  const fragOuts = [...fs.globals.values()].filter((g) => g.qualifier === 'out');
  cache.compiled = { sources: entry.program.sources, vs, fs, varyings, fragOut: fragOuts.length ? fragOuts.find((g) => g.location <= 0) || fragOuts[0] : fs.globals.get('gl_FragColor'), usesFragData: !fragOuts.length };
  return cache.compiled;
};

// Os globais iniciais de um estágio: zeros, constantes, inicializadores e os uniformes do programa.
const glStageGlobals = (stage, entry) => {
  const g = stage.gvalues.map(glslClone);
  const e = { g, l: [], rt: null };
  for (const init of stage.inits) init(e);
  const p = entry.program;
  p.uniforms.forEach((u, index) => {
    const path = u.base;
    const root = /^[A-Za-z_]\w*/.exec(path)[0];
    const info = stage.globals.get(root);
    if (!info || info.qualifier !== 'uniform') return;
    for (let el = 0; el < u.size; el++) {
      const raw = p.values.get(index + ':' + el);
      if (!raw) continue;
      const kind = glslKind(u.typeName === 'BOOL' ? 'bool' : (Object.entries(GLSL_TYPES).find(([, v]) => v === u.typeName) || ['float'])[0]);
      const glslType = (Object.entries(GLSL_TYPES).find(([, v]) => v === u.typeName) || ['float'])[0];
      const value = GLSL_SAMPLER.test(glslType) ? raw[0] | 0 : raw.length === 1 ? GLSL_CONV[kind || 'float'](raw[0]) : raw.map(GLSL_CONV[kind || 'float']);
      glAssignPath(g, info.slot, path.slice(root.length), u.size > 1 || /\[0\]$/.test(u.name) ? el : null, value);
    }
  });
  return g;
};
// Escreve `value` no global `slot` seguindo o resto do nome (`.campo`, `[i]`); `element` é o índice do array final.
const glAssignPath = (g, slot, rest, element, value) => {
  const steps = [];
  const re = /\.(\w+)|\[(\d+)\]/g;
  let m;
  while ((m = re.exec(rest)) !== null) steps.push(m[1] !== undefined ? m[1] : Number(m[2]));
  if (element !== null) steps.push(element);
  if (!steps.length) { g[slot] = value; return; }
  let target = g[slot];
  for (let i = 0; i < steps.length - 1; i++) target = target[steps[i]];
  target[steps[steps.length - 1]] = value;
};

// O valor de um atributo para um vértice: do buffer (normalizado ou não) ou o valor corrente.
const ATTRIB_READERS = {
  [GL.FLOAT]: [4, (v, o) => v.getFloat32(o, true)], [GL.BYTE]: [1, (v, o) => v.getInt8(o)], [GL.UNSIGNED_BYTE]: [1, (v, o) => v.getUint8(o)],
  [GL.SHORT]: [2, (v, o) => v.getInt16(o, true)], [GL.UNSIGNED_SHORT]: [2, (v, o) => v.getUint16(o, true)], [GL.INT]: [4, (v, o) => v.getInt32(o, true)], [GL.UNSIGNED_INT]: [4, (v, o) => v.getUint32(o, true)],
};
const NORMALIZE = { [GL.BYTE]: (x) => Math.max(x / 127, -1), [GL.UNSIGNED_BYTE]: (x) => x / 255, [GL.SHORT]: (x) => Math.max(x / 32767, -1), [GL.UNSIGNED_SHORT]: (x) => x / 65535, [GL.INT]: (x) => Math.max(x / 2147483647, -1), [GL.UNSIGNED_INT]: (x) => x / 4294967295 };
const glFetchAttrib = (a, current, vertex, instance) => {
  if (!a || !a.enabled || !a.data) return Array.from(current);
  const index = a.divisor ? Math.floor(instance / a.divisor) : vertex;
  const [bytes, read] = ATTRIB_READERS[a.type];
  const stride = a.stride || bytes * a.size;
  const view = new DataView(a.data.buffer, a.data.byteOffset, a.data.byteLength);
  const out = [0, 0, 0, 1];
  for (let c = 0; c < a.size; c++) {
    const x = read(view, a.offset + index * stride + c * bytes);
    out[c] = a.normalized && NORMALIZE[a.type] ? fr(NORMALIZE[a.type](x)) : fr(x);
  }
  return out;
};

// ---- Desenho.
// O comando visto no alvo espelhado de um framebuffer: tesoura refletida e a face da frente trocada (o Y de janela
// reflete em glWindow e as linhas do viewport em glRasterTriangle).
const glMirrored = (entry, height) => {
  const p = new Map(entry.params), box = p.get('SCISSOR_BOX');
  p.set('SCISSOR_BOX', [box[0], height - box[1] - box[3], box[2], box[3]]);
  p.set('FRONT_FACE', p.get('FRONT_FACE') === GL.CCW ? GL.CW : GL.CCW);
  return { ...entry, params: p };
};
const glRunDraw = (s, t, original) => {
  const entry = t.upright ? glMirrored(original, t.height) : original;
  let stages;
  try { stages = glProgramStages(entry); } catch (error) { if (error instanceof GlslCompileError) return; throw error; }
  const { vs, fs, varyings } = stages;
  const vsGlobals = glStageGlobals(vs, entry);
  const fsGlobals = glStageGlobals(fs, entry);
  const vsMain = vs.functions.get('main')[0], fsMain = fs.functions.get('main')[0];
  const inputs = entry.program.attribs.map((a) => ({ slot: vs.globals.get(a.name)?.slot, location: a.location, t: vs.globals.get(a.name)?.t })).filter((a) => a.slot !== undefined);
  const posSlot = vs.globals.get('gl_Position').slot, idSlot = vs.globals.get('gl_VertexID').slot, instSlot = vs.globals.get('gl_InstanceID').slot, sizeSlot = vs.globals.get('gl_PointSize').slot;
  const rt = glRuntime(entry, 'fragment'), vrt = glRuntime(entry, 'vertex');
  rt.upright = !!t.upright;
  const indices = glIndices(entry);
  for (let instance = 0; instance < entry.instances; instance++) {
    const cache = new Map();
    const shade = (index) => {
      let v = cache.get(index);
      if (v) return v;
      const g = vsGlobals.map(glslClone);
      for (const a of inputs) {
        const raw = glFetchAttrib(entry.attribs[a.location], entry.current[a.location], index, instance);
        const d = glslDims(a.t);
        const kind = glslKind(a.t);
        g[a.slot] = d.scalar ? GLSL_CONV[kind](raw[0]) : raw.slice(0, d.n).map(GLSL_CONV[kind]);
      }
      g[idSlot] = index; g[instSlot] = instance;
      vsMain.body({ g, l: new Array(vsMain.count), rt: vrt });
      v = { pos: g[posSlot].slice(), vary: varyings.map((x) => glslClone(g[x.vs])), size: g[sizeSlot] };
      cache.set(index, v);
      return v;
    };
    for (const prim of glPrimitives(entry.mode, indices)) {
      if (prim.length === 3) glTriangle(s, t, entry, stages, fsGlobals, fsMain, prim.map(shade), rt, prim.provoking);
      else if (prim.length === 2) glLine(s, t, entry, stages, fsGlobals, fsMain, prim.map(shade), shade(prim[1]), rt);
      else glPoint(s, t, entry, stages, fsGlobals, fsMain, shade(prim[0]), rt);
    }
  }
};

const glIndices = (entry) => {
  if (!entry.elements) return Array.from({ length: entry.count }, (_, i) => entry.first + i);
  const { data, type, offset } = entry.elements;
  const size = type === GL.UNSIGNED_BYTE ? 1 : type === GL.UNSIGNED_SHORT ? 2 : 4;
  const view = new DataView(data.buffer, data.byteOffset + offset, entry.count * size);
  return Array.from({ length: entry.count }, (_, i) => (size === 1 ? view.getUint8(i) : size === 2 ? view.getUint16(i * 2, true) : view.getUint32(i * 4, true)));
};

// Triângulos de cada modo na ordem do Vulkan (faixa ímpar: i, i + 2, i + 1; leque: i, i + 1, 0), com o índice do vértice
// provocador do GL (o último do triângulo no GL); linhas como pares (o provocador é o segundo) e pontos sozinhos.
const glPrimitives = (mode, idx) => {
  const out = [];
  const tri = (a, b, c, provoking) => out.push(Object.assign([a, b, c], { provoking }));
  if (mode === GL.TRIANGLES) for (let i = 0; i + 2 < idx.length; i += 3) tri(idx[i], idx[i + 1], idx[i + 2], 2);
  else if (mode === GL.TRIANGLE_STRIP) for (let i = 0; i + 2 < idx.length; i++) { if (i % 2) tri(idx[i], idx[i + 2], idx[i + 1], 1); else tri(idx[i], idx[i + 1], idx[i + 2], 2); }
  else if (mode === GL.TRIANGLE_FAN) for (let i = 1; i + 1 < idx.length; i++) tri(idx[i], idx[i + 1], idx[0], 1);
  else if (mode === GL.LINES) for (let i = 0; i + 1 < idx.length; i += 2) out.push([idx[i], idx[i + 1]]);
  else if (mode === GL.LINE_STRIP || mode === GL.LINE_LOOP) {
    for (let i = 0; i + 1 < idx.length; i++) out.push([idx[i], idx[i + 1]]);
    if (mode === GL.LINE_LOOP && idx.length > 1) out.push([idx[idx.length - 1], idx[0]]);
  } else if (mode === GL.POINTS) for (const i of idx) out.push([i]);
  return out;
};

// Recorte contra w > 0 e contra os planos próximo e distante (-w <= z <= w), por Sutherland-Hodgman.
const glClip = (verts) => {
  const planes = [(v) => v.pos[3] - 1e-6, (v) => v.pos[2] + v.pos[3], (v) => v.pos[3] - v.pos[2]];
  let poly = verts;
  for (const dist of planes) {
    if (poly.every((v) => dist(v) >= 0)) continue;
    const next = [];
    for (let i = 0; i < poly.length; i++) {
      const a = poly[i], b = poly[(i + 1) % poly.length];
      const da = dist(a), db = dist(b);
      if (da >= 0) next.push(a);
      if ((da >= 0) !== (db >= 0)) {
        const k = da / (da - db);
        next.push({ pos: a.pos.map((x, j) => fr(x + (b.pos[j] - x) * k)), vary: a.vary.map((x, j) => glLerp(x, b.vary[j], k)), weights: a.weights.map((w, j) => w + (b.weights[j] - w) * k) });
      }
    }
    poly = next;
    if (poly.length < 3) return [];
  }
  return poly;
};
const glLerp = (a, b, k) => (Array.isArray(a) ? a.map((x, i) => glLerp(x, b[i], k)) : typeof a === 'number' ? fr(a + (b - a) * k) : a);

const SUBPIXEL = 16;
// A janela como o SetupRoutine do SwiftShader: X = RoundInt(X0xF + x * rhw * WxF), em 1/16 de pixel e binário32. Num
// framebuffer (`mirror` é a altura do alvo), o Y arredondado reflete nas linhas espelhadas do alvo; a posição de
// recorte, que decide a rotação do setup, fica a do GL.
const glWindow = (p, mirror) => {
  const vp = p.get('VIEWPORT'), range = p.get('DEPTH_RANGE');
  const halfW = fr(vp[2] / 2), halfH = fr(vp[3] / 2);
  const X0xF = fr(fr(vp[0] + halfW) * SUBPIXEL), Y0xF = fr(fr(vp[1] + halfH) * SUBPIXEL), WxF = fr(halfW * SUBPIXEL), HxF = fr(halfH * SUBPIXEL);
  const zScale = fr((range[1] - range[0]) / 2), zBias = fr((range[1] + range[0]) / 2);
  return (v) => {
    const w = v.pos[3];
    const rhw = w !== 0 ? fr(1 / w) : 1;
    const X = Math.round(fr(X0xF + fr(fr(v.pos[0] * rhw) * WxF))), Yg = Math.round(fr(Y0xF + fr(fr(v.pos[1] * rhw) * HxF)));
    const Y = mirror ? mirror * SUBPIXEL - Yg : Yg;
    return { X, Y, x: X / SUBPIXEL, y: Y / SUBPIXEL, w, rhw, cy: v.pos[1], z: fr(fr(fr(v.pos[2] * rhw) * zScale) + zBias), vary: v.vary };
  };
};
const glTriangle = (s, t, entry, stages, fsGlobals, fsMain, verts, rt, provokingIndex) => {
  const p = entry.params;
  const provoking = verts[provokingIndex];
  const tagged = verts.map((v, i) => ({ pos: v.pos, vary: v.vary, weights: [i === 0 ? 1 : 0, i === 1 ? 1 : 0, i === 2 ? 1 : 0] }));
  const poly = glClip(tagged);
  if (poly.length < 3) return;
  const win = poly.map(glWindow(p, t.upright && t.height));
  for (let i = 1; i + 1 < win.length; i++) glRasterTriangle(s, t, entry, stages, fsGlobals, fsMain, [win[0], win[i], win[i + 1]], provoking, rt);
};

// Um vértice de janela deslocado de (dx, dy) em 1/16 de pixel.
const glShifted = (v, dx, dy) => ({ ...v, X: v.X + dx, Y: v.Y + dy, x: (v.X + dx) / SUBPIXEL, y: (v.Y + dy) / SUBPIXEL });
// Um retângulo de janela (quatro cantos em ordem) como dois triângulos, com os atributos de `prim`.
const glRasterQuad = (s, t, entry, stages, fsGlobals, fsMain, [c0, c1, c2, c3], provoking, rt, prim) => {
  glRasterTriangle(s, t, entry, stages, fsGlobals, fsMain, [c0, c1, c2], provoking, rt, prim);
  glRasterTriangle(s, t, entry, stages, fsGlobals, fsMain, [c0, c2, c3], provoking, rt, prim);
};
// O ponto do SwiftShader: um quadrado de lado gl_PointSize em volta do centro, recortado só por w e z (o centro fora
// da janela ainda desenha a parte que cai nela). Os atributos são os do vértice; gl_PointCoord tem a origem em cima.
const glPoint = (s, t, entry, stages, fsGlobals, fsMain, v, rt) => {
  const p = entry.params;
  if (!(v.pos[3] > 0) || v.pos[2] < -v.pos[3] || v.pos[2] > v.pos[3]) return;
  const range = p.get('ALIASED_POINT_SIZE_RANGE');
  const size = Math.min(Math.max(v.size ?? 1, range[0]), range[1]);
  const c = glWindow(p, t.upright && t.height)(v), half = Math.round(fr(size * SUBPIXEL) / 2);
  // A origem de gl_PointCoord fica em cima no sentido do GL também num framebuffer (linhas espelhadas no alvo).
  const x0 = fr(c.x - fr(size / 2)), y0 = fr(c.y - fr(size / 2)), y1 = fr(c.y + fr(size / 2)), rcp = fr(1 / size);
  // O desempate do ponto é fixo no GL (entram as bordas esquerda e de baixo, no buffer padrão e num framebuffer):
  // nas linhas espelhadas do framebuffer, a borda horizontal aceita é a outra. gl_PointCoord multiplica pelo recíproco
  // do tamanho (1/6 arredonda para cima e 5/6 sai acima de 212,5/255).
  const prim = {
    z: () => c.z, rhw: () => c.rhw, vary: (i) => glslClone(c.vary[i]), flipTies: !!t.upright,
    coord: (x, y) => [fr(fr(fr(x + 0.5) - x0) * rcp), t.upright ? fr(fr(fr(y + 0.5) - y0) * rcp) : fr(fr(y1 - fr(y + 0.5)) * rcp)],
  };
  glRasterQuad(s, t, entry, stages, fsGlobals, fsMain, [glShifted(c, -half, -half), glShifted(c, half, -half), glShifted(c, half, half), glShifted(c, -half, half)], v, rt, prim);
};
// A linha Bresenham do SwiftShader (o modo que o ANGLE pede): um paralelogramo esticado meio pixel para cada lado no
// eixo menor, montado no espaço de recorte, recortado como polígono contra os seis planos e rasterizado com a regra de
// arestas dos triângulos (os vértices recortados, arredondados a 1/16, deslocam os empates). Atributos, z e 1/w vêm
// dos planos do setup de triângulo sobre as pontas originais (P1, P2, P0), com P2 = P0 mais a perpendicular ao segmento
// e os valores de P0: a projeção do centro no segmento da especificação do Vulkan, com o arredondamento do SwiftShader
// (medido nos empates de t = 5/6).
const glLine = (s, t, entry, stages, fsGlobals, fsMain, [P0, P1], provoking, rt) => {
  const p = entry.params;
  if (!(P0.pos[3] > 0) || !(P1.pos[3] > 0)) { glLineBehind(s, t, entry, stages, fsGlobals, fsMain, [P0, P1], provoking, rt); return; }
  const toWindow = glWindow(p, t.upright && t.height);
  const [a, b] = [P0, P1].map(toWindow);
  const dX = b.X - a.X, dY = b.Y - a.Y;
  if (dX === 0 && dY === 0) return;
  const vp = p.get('VIEWPORT'), halfW = fr(vp[2] / 2), halfH = fr(vp[3] / 2);
  const dx = fr(halfW * fr(fr(P1.pos[0] / P1.pos[3]) - fr(P0.pos[0] / P0.pos[3]))), dy = fr(halfH * fr(fr(P1.pos[1] / P1.pos[3]) - fr(P0.pos[1] / P0.pos[3])));
  const horizontal = Math.abs(dx) >= Math.abs(dy);
  // Meio pixel em unidades de recorte do vértice: no eixo menor (Bresenham) ou, com várias amostras, onde o Vulkan não
  // aceita Bresenham, na perpendicular ao segmento (linha retangular de largura 1).
  const len = Math.hypot(dx, dy);
  const [nx, ny] = t.samples.length > 1 ? [fr(fr(-dy / len) * 0.5), fr(fr(dx / len) * 0.5)] : horizontal ? [0, 0.5] : [0.5, 0];
  const offset = (v, sign) => {
    const pos = v.pos.slice();
    pos[0] = fr(pos[0] + sign * fr(fr(nx * pos[3]) / halfW));
    pos[1] = fr(pos[1] + sign * fr(fr(ny * pos[3]) / halfH));
    return { pos, vary: v.vary };
  };
  const poly = glClipPolygon([offset(P0, -1), offset(P1, -1), offset(P1, 1), offset(P0, 1)]);
  if (poly.length < 3) return;
  const win = poly.map(toWindow);
  const prim = { planes: [b, glShifted(a, dY, -dX), a], coord: () => [0, 0] };
  for (let i = 1; i + 1 < win.length; i++) glRasterTriangle(s, t, entry, stages, fsGlobals, fsMain, [win[0], win[i], win[i + 1]], provoking, rt, prim);
};
// Uma ponta atrás do olho: recorta o segmento em w antes (o plano próximo resolve o resto no polígono).
const glLineBehind = (s, t, entry, stages, fsGlobals, fsMain, [a, b], provoking, rt) => {
  const da = fr(a.pos[2] + a.pos[3]), db = fr(b.pos[2] + b.pos[3]);
  if (da < 0 && db < 0) return;
  const D = fr(1 / fr(db - da)), cut = { pos: a.pos.map((x, j) => fr(fr(fr(db * x) - fr(da * b.pos[j])) * D)), vary: a.vary.map((x, j) => glLerp(x, b.vary[j], fr(-da * D))) };
  const seg = da < 0 ? [cut, b] : [a, cut];
  if (seg[0].pos[3] > 0 && seg[1].pos[3] > 0) glLine(s, t, entry, stages, fsGlobals, fsMain, seg, provoking, rt);
};
// O Clipper do SwiftShader: próximo, distante, esquerda, direita, cima e baixo, cada aresta cortada em binário32 por
// Vo = (dj * Vi - di * Vj) / (dj - di).
const GL_CLIP_PLANES = [(v) => v[2] + v[3], (v) => v[3] - v[2], (v) => v[3] + v[0], (v) => v[3] - v[0], (v) => v[3] - v[1], (v) => v[3] + v[1]];
const glClipPolygon = (verts) => {
  let poly = verts;
  for (const plane of GL_CLIP_PLANES) {
    const d = poly.map((v) => fr(plane(v.pos)));
    if (d.every((x) => x >= 0)) continue;
    const next = [];
    const edge = (vi, vj, di, dj) => { const D = fr(1 / fr(dj - di)); return { pos: vi.pos.map((x, k) => fr(fr(fr(dj * x) - fr(di * vj.pos[k])) * D)), vary: vi.vary }; };
    for (let i = 0; i < poly.length; i++) {
      const j = (i + 1) % poly.length;
      if (d[i] >= 0) { next.push(poly[i]); if (d[j] < 0) next.push(edge(poly[i], poly[j], d[i], d[j])); }
      else if (d[j] > 0) next.push(edge(poly[j], poly[i], d[j], d[i]));
    }
    poly = next;
    if (poly.length < 3) return [];
  }
  return poly;
};

const BLEND_FACTORS = {
  [GL.ZERO]: () => 0, [GL.ONE]: () => 1,
  [GL.SRC_COLOR]: (sc, dc, c) => sc[c], [GL.ONE_MINUS_SRC_COLOR]: (sc, dc, c) => 1 - sc[c],
  [GL.DST_COLOR]: (sc, dc, c) => dc[c], [GL.ONE_MINUS_DST_COLOR]: (sc, dc, c) => 1 - dc[c],
  [GL.SRC_ALPHA]: (sc) => sc[3], [GL.ONE_MINUS_SRC_ALPHA]: (sc) => 1 - sc[3],
  [GL.DST_ALPHA]: (sc, dc) => dc[3], [GL.ONE_MINUS_DST_ALPHA]: (sc, dc) => 1 - dc[3],
  [GL.CONSTANT_COLOR]: (sc, dc, c, k) => k[c], [GL.ONE_MINUS_CONSTANT_COLOR]: (sc, dc, c, k) => 1 - k[c],
  [GL.CONSTANT_ALPHA]: (sc, dc, c, k) => k[3], [GL.ONE_MINUS_CONSTANT_ALPHA]: (sc, dc, c, k) => 1 - k[3],
  [GL.SRC_ALPHA_SATURATE]: (sc, dc, c) => (c === 3 ? 1 : Math.min(sc[3], 1 - dc[3])),
};
const BLEND_EQUATIONS = { [GL.FUNC_ADD]: (a, b) => a + b, [GL.FUNC_SUBTRACT]: (a, b) => a - b, [GL.FUNC_REVERSE_SUBTRACT]: (a, b) => b - a, 32775: (a, b, x, y) => Math.min(x, y), 32776: (a, b, x, y) => Math.max(x, y) };
const DEPTH_FUNCS = { [GL.NEVER]: () => false, [GL.LESS]: (a, b) => a < b, [GL.EQUAL]: (a, b) => a === b, [GL.LEQUAL]: (a, b) => a <= b, [GL.GREATER]: (a, b) => a > b, [GL.NOTEQUAL]: (a, b) => a !== b, [GL.GEQUAL]: (a, b) => a >= b, [GL.ALWAYS]: () => true };
// As operações do estêncil de 8 bits (o valor guardado, a referência já limitada a 0..255).
const STENCIL_OPS = {
  [GL.KEEP]: (v) => v, [GL.ZERO]: () => 0, [GL.REPLACE]: (v, ref) => ref, [GL.INVERT]: (v) => ~v & 255,
  [GL.INCR]: (v) => Math.min(v + 1, 255), [GL.DECR]: (v) => Math.max(v - 1, 0), [GL.INCR_WRAP]: (v) => (v + 1) & 255, [GL.DECR_WRAP]: (v) => (v - 1) & 255,
};
// O estado do estêncil de uma face, ou `null` sem teste: a função compara (ref & máscara) com (guardado & máscara).
const glStencilState = (t, p, front) => {
  if (!t.stencil || !p.get('STENCIL_TEST')) return null;
  const side = front ? '_' : '_BACK_';
  return {
    test: DEPTH_FUNCS[p.get(`STENCIL${side}FUNC`)], ref: Math.min(Math.max(p.get(`STENCIL${side}REF`), 0), 255), mask: p.get(`STENCIL${side}VALUE_MASK`) & 255, write: p.get(`STENCIL${side}WRITEMASK`) & 255,
    fail: STENCIL_OPS[p.get(`STENCIL${side}FAIL`)], zfail: STENCIL_OPS[p.get(`STENCIL${side}PASS_DEPTH_FAIL`)], zpass: STENCIL_OPS[p.get(`STENCIL${side}PASS_DEPTH_PASS`)],
  };
};
const glStencilApply = (t, st, si, op) => { const v = t.stencil[si]; t.stencil[si] = (v & ~st.write) | (op(v, st.ref) & st.write); };

// `prim`, nos pontos e nas linhas, dá z, 1/w, atributos e gl_PointCoord por posição de janela: sem descarte de face,
// sempre de frente e sem polygon offset (só vale para preenchimento).
const glRasterTriangle = (s, t, entry, stages, fsGlobals, fsMain, [a, b, c], provoking, rt, prim) => {
  const p = entry.params;
  const area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
  if (area === 0) return;
  const ccw = area > 0;
  const front = prim ? true : (p.get('FRONT_FACE') === GL.CCW) === ccw;
  // O avaliador direto do ponto; a linha usa os planos.
  const direct = prim && prim.vary ? prim : null;
  if (!prim && p.get('CULL_FACE')) { const mode = p.get('CULL_FACE_MODE'); if (mode === GL.FRONT_AND_BACK || (mode === GL.FRONT) === front) return; }
  // As arestas no espaço do Vulkan, que o ANGLE usa com a mesma origem da janela do GL (a inversão fica na leitura),
  // orientadas com o interior do lado positivo. A regra do canto superior esquerdo (y crescendo para baixo, como no
  // Vulkan) aceita a amostra exatamente sobre uma aresta esquerda (o interior à direita) ou de cima (horizontal, com o
  // interior abaixo): medido com faixas cujas bordas caem sobre as amostras.
  const vk = [a, b, c];
  const vkArea = (vk[1].x - vk[0].x) * (vk[2].y - vk[0].y) - (vk[1].y - vk[0].y) * (vk[2].x - vk[0].x);
  const order = vkArea > 0 ? [0, 1, 2] : [0, 2, 1];
  const edges = [0, 1, 2].map((i) => {
    const p0 = vk[order[i]], p1 = vk[order[(i + 1) % 3]];
    const dx = p1.x - p0.x, dy = p1.y - p0.y;
    return { dx, dy, x0: p0.x, y0: p0.y, topLeft: dy < 0 || (dy === 0 && (prim && prim.flipTies ? dx < 0 : dx > 0)) };
  });
  let bx0 = Math.floor(Math.min(a.x, b.x, c.x)), bx1 = Math.ceil(Math.max(a.x, b.x, c.x));
  let by0 = Math.floor(Math.min(a.y, b.y, c.y)), by1 = Math.ceil(Math.max(a.y, b.y, c.y));
  const vp = p.get('VIEWPORT'), [vy0, vy1] = glRows(t, vp[1], vp[1] + vp[3]);
  bx0 = Math.max(bx0, 0, vp[0]); by0 = Math.max(by0, 0, vy0); bx1 = Math.min(bx1, t.width, vp[0] + vp[2]); by1 = Math.min(by1, t.height, vy1);
  if (p.get('SCISSOR_TEST')) { const sb = p.get('SCISSOR_BOX'); bx0 = Math.max(bx0, sb[0]); by0 = Math.max(by0, sb[1]); bx1 = Math.min(bx1, sb[0] + sb[2]); by1 = Math.min(by1, sb[1] + sb[3]); }
  const n = t.samples.length;
  // (x, y) em coordenadas do Vulkan.
  const inside = (x, y) => edges.every((e) => { const v = e.dx * (y - e.y0) - e.dy * (x - e.x0); return v > 0 || (v === 0 && e.topLeft); });
  // As equações de plano do SetupRoutine do SwiftShader, em binário32 e relativas ao vértice 0: cada interpolante é
  // P.x * dx + P.y * dy + P.z (o atributo sobre w), e o plano W dá 1/w; o fragmento multiplica pelo inverso dele.
  // Os vértices dos planos na ordem do SetupRoutine. Com o provocador no fim (VK_EXT_provoking_vertex no modo LAST), o
  // triângulo (a, b, c) chega como (c, a, b); depois gira para o de menor y de recorte vir primeiro e, em seguida, o de
  // maior w (com valores iguais as duas rotações de cada critério se desfazem). O ANGLE inverte o y para o Vulkan, então
  // os planos vivem no espaço invertido, com o Y de janela contado de cima (`YV`).
  const flipped = (v) => ({ ...v, YV: t.height * SUBPIXEL - v.Y });
  let [pa, pb, pc] = [flipped(c), flipped(a), flipped(b)];
  const rotate = (cond1, cond2) => { if (cond1) [pa, pb, pc] = [pb, pc, pa]; if (cond2) [pa, pb, pc] = [pc, pa, pb]; };
  if (prim && prim.planes) [pa, pb, pc] = prim.planes.map(flipped);
  else {
    { const yMin = Math.min(pa.cy, pb.cy, pc.cy); rotate(yMin === pb.cy, yMin === pc.cy); }
    { const [w1, w2] = [pb.w, pc.w], wMax = Math.max(pa.w, w1, w2); rotate(wMax === w1, wMax === w2); }
  }
  const x1 = fr(fr(pb.w * (1 / SUBPIXEL)) * (pb.X - pa.X)), y1 = fr(fr(pb.w * (1 / SUBPIXEL)) * (pb.YV - pa.YV));
  const x2 = fr(fr(pc.w * (1 / SUBPIXEL)) * (pc.X - pa.X)), y2 = fr(fr(pc.w * (1 / SUBPIXEL)) * (pc.YV - pa.YV));
  const det = fr(fr(x1 * y2) - fr(x2 * y1));
  if (det === 0) return;
  const A = fr(1 / det), D = fr(A * pa.rhw);
  const M = [[fr(fr(fr(y1 * pc.w) - fr(y2 * pb.w)) * D), fr(fr(fr(x2 * pb.w) - fr(x1 * pc.w)) * D), pa.rhw], [fr(y2 * A), fr(-x2 * A), 0], [fr(-y1 * A), fr(x1 * A), 0]];
  const plane = (i0, i1, i2) => [0, 1, 2].map((k) => fr(fr(fr(i0 * M[0][k]) + fr(i1 * M[1][k])) + fr(i2 * M[2][k])));
  const W = plane(1, 1, 1);
  // A origem dos planos é o vértice 0 deslocado de meio pixel (o centro do pixel fica nos inteiros), e cada linha
  // soma D = C + y * B antes do termo em x, como o QuadRasterizer.
  const ax = fr((pa.X - SUBPIXEL / 2) / SUBPIXEL), ay = fr((pa.YV - SUBPIXEL / 2) / SUBPIXEL);
  // A linha de um pixel e a de uma amostra no espaço invertido.
  const rowV = (y) => t.height - 1 - y, sampleRowV = (y, sy) => fr(t.height - y - sy - 0.5);
  const planes = stages.varyings.map((v, i) => (v.flat ? null : glslMap3(pa.vary[i], pb.vary[i], pc.vary[i], plane)));
  const zPlane = (() => {
    const z1 = fr(pb.z - pa.z), z2 = fr(pc.z - pa.z);
    const u1 = fr((pb.X - pa.X) / SUBPIXEL), v1 = fr((pb.YV - pa.YV) / SUBPIXEL), u2 = fr((pc.X - pa.X) / SUBPIXEL), v2 = fr((pc.YV - pa.YV) / SUBPIXEL);
    const Dz = fr(1 / fr(fr(u1 * v2) - fr(u2 * v1)));
    const A = fr(fr(fr(v2 * z1) - fr(v1 * z2)) * Dz), B = fr(fr(fr(u1 * z2) - fr(u2 * z1)) * Dz);
    // O polygon offset soma ao termo constante o declive vezes o fator e as unidades vezes 2^-24: o ANGLE emula o D24
    // do GL e a resolução mínima fica a do D24 em qualquer profundidade (medido em 0,1, 0,5 e 0,95).
    if (prim || !p.get('POLYGON_OFFSET_FILL')) return [A, B, pa.z];
    const slope = fr(Math.max(Math.abs(A), Math.abs(B)) * p.get('POLYGON_OFFSET_FACTOR'));
    return [A, B, fr(fr(pa.z + slope) + fr(p.get('POLYGON_OFFSET_UNITS') * 2 ** -24))];
  })();
  const evalPlane = (P, dx, dy) => fr(fr(dx * P[0]) + fr(P[2] + fr(dy * P[1])));
  const varyings = stages.varyings;
  const fragSlot = stages.fragOut.slot;
  const coordSlot = stages.fs.globals.get('gl_FragCoord').slot, facingSlot = stages.fs.globals.get('gl_FrontFacing').slot, pointCoordSlot = stages.fs.globals.get('gl_PointCoord').slot;
  const depthTest = t.depth && p.get('DEPTH_TEST'), depthWrite = depthTest && p.get('DEPTH_WRITEMASK'), depthFn = DEPTH_FUNCS[p.get('DEPTH_FUNC')];
  const stencil = glStencilState(t, p, front);
  const mask = p.get('COLOR_WRITEMASK');
  const blend = p.get('BLEND');
  const constant = p.get('BLEND_COLOR');
  const [srcRgb, dstRgb, srcA, dstA] = ['BLEND_SRC_RGB', 'BLEND_DST_RGB', 'BLEND_SRC_ALPHA', 'BLEND_DST_ALPHA'].map((k) => BLEND_FACTORS[p.get(k)]);
  const [eqRgb, eqA] = ['BLEND_EQUATION_RGB', 'BLEND_EQUATION_ALPHA'].map((k) => BLEND_EQUATIONS[p.get(k)]);
  const alphaBits = t.alpha, max = t.max;
  // O sombreador de fragmento roda por quad de 2x2 pixels, como no SwiftShader, alinhado no espaço invertido do Vulkan:
  // as faixas 0 e 1 na linha de cima (em GL), 2 e 3 na de baixo. Quando o sombreador troca valores entre faixas
  // (derivadas, lod implícito), as faixas não cobertas rodam como auxiliares e as passadas se repetem até os valores
  // trocados ficarem estáveis.
  const fragment = (x, y) => {
    const dx = fr(x - ax), dy = fr(rowV(y) - ay);
    const cx = fr(x + 0.5), cyw = fr(y + 0.5);
    const rhwPix = direct ? direct.rhw(cx, cyw) : evalPlane(W, dx, dy);
    const wPix = fr(1 / rhwPix);
    const z = direct ? direct.z(cx, cyw) : evalPlane(zPlane, dx, dy);
    const g = fsGlobals.map(glslClone);
    varyings.forEach((v, i) => {
      g[v.fs] = v.flat ? glslClone(provoking.vary[i]) : direct ? direct.vary(i, cx, cyw) : glslMapPlane(planes[i], (P) => fr(evalPlane(P, dx, dy) * wPix));
    });
    g[coordSlot] = [cx, t.upright ? fr(t.height - cyw) : cyw, z, rhwPix];
    g[facingSlot] = front;
    if (prim) g[pointCoordSlot] = prim.coord(x, y);
    rt.discarded = false;
    fsMain.body({ g, l: new Array(fsMain.count), rt });
    if (rt.discarded) return null;
    const out = stages.usesFragData && !g[fragSlot].some((x) => x !== 0) ? g[stages.fs.globals.get('gl_FragData').slot][0] : g[fragSlot];
    return out;
  };
  const runLane = (lane, x, y, prev, cur) => { rt.lane = lane; rt.calls = 0; rt.prev = prev; rt.cur = cur; return fragment(x, y); };
  // Alvo normalizado: a saída e a mistura limitam a [0, 1]; em ponto flutuante, nada limita, a mistura fica em
  // binário32 e o formato quantiza na escrita.
  const clamp = t.float ? fr : (v) => Math.min(1, Math.max(0, v));
  // Em ponto flutuante, o fator (1 - a) também sai em binário32 antes do produto (medido na sonda webgl-fbo-gaps).
  const factor = t.float ? fr : (v) => v;
  const encode = t.float ? (v, ch) => t.float[ch](v) : (v, ch) => unormN(v, max[ch]);
  const decode = t.float ? (ci, ch) => f32FromBits(t.color[ci + ch]) : (ci, ch) => t.color[ci + ch] / max[ch];
  const write = (x, y, covered, out) => {
    const src = out.map(clamp);
    for (let k = 0; k < n; k++) {
      if (!(covered & (1 << k))) continue;
      const si = (y * t.width + x) * n + k;
      // A ordem do Vulkan: estêncil, depois profundidade; cada desfecho escolhe a sua operação.
      if (stencil && !stencil.test(stencil.ref & stencil.mask, t.stencil[si] & stencil.mask)) { glStencilApply(t, stencil, si, stencil.fail); continue; }
      if (depthTest) {
        const [sx, sy] = t.samples[k];
        const zs = t.depthQuant(Math.min(1, Math.max(0, direct ? direct.z() : evalPlane(zPlane, fr(fr(x + sx - 0.5) - ax), fr(sampleRowV(y, sy) - ay)))));
        if (!depthFn(zs, t.depth[si])) { if (stencil) glStencilApply(t, stencil, si, stencil.zfail); continue; }
        if (depthWrite) t.depth[si] = zs;
      }
      if (stencil) glStencilApply(t, stencil, si, stencil.zpass);
      const ci = si * 4;
      let rgba = src;
      if (blend && t.color) {
        const dst = [decode(ci, 0), decode(ci, 1), decode(ci, 2), alphaBits ? decode(ci, 3) : 1];
        rgba = [0, 1, 2, 3].map((ch) => {
          const sf = factor((ch === 3 ? srcA : srcRgb)(src, dst, ch, constant)), df = factor((ch === 3 ? dstA : dstRgb)(src, dst, ch, constant));
          return clamp((ch === 3 ? eqA : eqRgb)(fr(src[ch] * sf), fr(dst[ch] * df), src[ch], dst[ch]));
        });
      }
      if (t.color) for (let ch = 0; ch < 4; ch++) if (mask[ch]) t.color[ci + ch] = ch === 3 && !alphaBits ? max[3] : encode(rgba[ch], ch);
    }
  };
  for (let qv = (t.height - by1) & ~1; qv < t.height - by0; qv += 2) for (let qx = bx0 & ~1; qx < bx1; qx += 2) {
    const top = t.height - 1 - qv;
    const lanes = [[qx, top], [qx + 1, top], [qx, top - 1], [qx + 1, top - 1]];
    const covers = lanes.map(([x, y]) => {
      if (x < bx0 || x >= bx1 || y < by0 || y >= by1) return 0;
      let covered = 0;
      for (let k = 0; k < n; k++) { const [sx, sy] = t.samples[k]; if (inside(x + sx, y + sy)) covered |= 1 << k; }
      return covered;
    });
    if (!covers.some(Boolean)) continue;
    rt.crossLane = false;
    let cur = [];
    let results = lanes.map(([x, y], lane) => (covers[lane] ? runLane(lane, x, y, null, cur) : null));
    if (rt.crossLane) {
      for (let pass = 0; pass < 6; pass++) {
        const prev = cur;
        cur = [];
        results = lanes.map(([x, y], lane) => runLane(lane, x, y, prev, cur));
        if (pass > 0 && glSameRecords(prev, cur)) break;
      }
    }
    lanes.forEach(([x, y], lane) => { if (covers[lane] && results[lane]) write(x, y, covers[lane], results[lane]); });
  }
};
// Os valores trocados entre faixas de duas passadas são iguais.
const glSameRecords = (a, b) => {
  const eq = (x, y) => (Array.isArray(x) ? Array.isArray(y) && x.length === y.length && x.every((v, i) => eq(v, y[i])) : Object.is(x, y));
  return a.length === b.length && a.every((lanes, k) => lanes.length === b[k].length && [0, 1, 2, 3].every((i) => eq(lanes[i], b[k][i])));
};
// Aplica `fn` componente a componente sobre três valores iguais em forma (o plano de cada componente) e de volta.
const glslMap3 = (a, b, c, fn) => (Array.isArray(a) ? a.map((x, i) => glslMap3(x, b[i], c[i], fn)) : typeof a === 'number' ? fn(a, b, c) : { constant: a });
const glslMapPlane = (p, fn) => (Array.isArray(p) && Array.isArray(p[0]) ? p.map((q) => glslMapPlane(q, fn)) : Array.isArray(p) && typeof p[0] === 'number' ? fn(p) : Array.isArray(p) ? p.map((q) => glslMapPlane(q, fn)) : p && p.constant !== undefined ? p.constant : p);

// O canvas WebGL visto de fora (`toDataURL`, `drawImage`, `createImageBitmap`): uma superfície da ponte com os pixels
// resolvidos, de cima para baixo e sem pré-multiplicação (o arredondamento do Skia, (c * 255 + a / 2) / a).
const glCanvasSurface = (s) => {
  const pixels = glFlush(s);
  const { width, height } = s.target;
  if (s.surface === undefined) s.surface = op('canvasCreate', width, height);
  else op('canvasResize', s.surface, width, height);
  const out = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const i = ((height - 1 - y) * width + x) * 4, o = (y * width + x) * 4;
    const alpha = s.attrs.alpha ? pixels[i + 3] : 255;
    for (let c = 0; c < 3; c++) out[o + c] = !s.attrs.premultipliedAlpha || alpha === 255 ? pixels[i + c] : alpha === 0 ? 0 : Math.min(255, Math.floor((pixels[i + c] * 255 + (alpha >> 1)) / alpha));
    out[o + 3] = alpha;
  }
  op('canvasWrite', s.surface, 0, 0, width, height, latin1Of(out));
  return s.surface;
};

// O que os sombreadores pedem ao ambiente: amostragem pelas texturas fixadas no desenho (`49-webgl-texture.js`) e as
// trocas entre faixas do quad (derivadas grossas como o OpDPdx do SwiftShader: a faixa 1 menos a 0 em x; em y, a 2
// menos a 0 no espaço invertido, que o ANGLE nega de volta). Sem os valores das quatro faixas (a primeira passada), a
// troca vale zero.
const glRuntime = (entry, stage) => {
  const rt = { discarded: false, lane: 0, calls: 0, prev: null, cur: null, crossLane: false };
  const viewOf = (unit, type) => { const u = entry.textures[unit]; return (u && u[SAMPLER_TARGET(type)]) || texIncomplete(type); };
  const share = (value) => {
    rt.crossLane = true;
    const k = rt.calls++;
    if (rt.cur) (rt.cur[k] ??= [])[rt.lane] = value;
    const all = rt.prev && rt.prev[k];
    return all && [0, 1, 2, 3].every((i) => all[i] !== undefined) ? all : null;
  };
  const integerResult = (type, c) => (/^[iu]/.test(type) ? c.map((x) => x | 0) : c);
  rt.sample = (unit, type, coord, lod, bias, proj) => {
    const view = viewOf(unit, type);
    const c = Array.isArray(coord) ? coord : [coord];
    const dims = /Cube|3D|Array/.test(type) ? 3 : 2;
    const shadow = /Shadow$/.test(type);
    let own = c.slice(0, dims);
    let dref = shadow ? c[dims] : 0;
    if (proj) {
      const q = c[c.length - 1];
      own = own.map((x) => fr(x / q));
      if (shadow) dref = fr(dref / q);
    }
    let method = lod !== null ? 'Lod' : bias ? 'Bias' : 'Implicit';
    let lodOrBias = lod !== null ? lod : bias;
    if (stage === 'vertex' && method !== 'Lod') { method = 'Lod'; lodOrBias = 0; }
    const quad = method === 'Implicit' || method === 'Bias' ? share(own) : null;
    const out = texSample(view, method, own, quad, lodOrBias, dref);
    return shadow ? out[0] : integerResult(type, out);
  };
  rt.texelFetch = (unit, type, p, l) => {
    const view = viewOf(unit, type);
    const pos = Array.isArray(p) ? p : [p];
    return integerResult(type, texSample(view, 'Fetch', [pos[0], pos[1] ?? 0, pos[2] ?? 0], null, l, 0));
  };
  rt.textureSize = (unit, type, l) => {
    const view = viewOf(unit, type);
    const img = view.levels[l]?.[0];
    if (!img) return /3D|Array/.test(type) ? [0, 0, 0] : [0, 0];
    return /3D|Array/.test(type) ? [img.w, img.h, img.d] : [img.w, img.h];
  };
  rt.derivative = (f, e, axis) => {
    const v = f(e);
    const lanes = share(v);
    const diff = (a, b) => (Array.isArray(a) ? a.map((x, i) => diff(x, b[i])) : fr(a - b));
    if (!lanes) return Array.isArray(v) ? v.map(() => 0) : 0;
    // Em y, o ANGLE nega a derivada do buffer padrão (invertido); num framebuffer a faixa 2 já é a linha de cima.
    return axis === 'x' ? diff(lanes[1], lanes[0]) : rt.upright ? diff(lanes[2], lanes[0]) : diff(lanes[0], lanes[2]);
  };
  return rt;
};
