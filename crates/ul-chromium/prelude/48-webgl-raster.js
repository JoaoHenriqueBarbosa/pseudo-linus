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
    t = s.target = { width: w, height: h, samples, color: new Uint8Array(w * h * 4 * n), depth: s.attrs.depth ? new Float32Array(w * h * n).fill(1) : null, stencil: s.attrs.stencil ? new Uint8Array(w * h * n) : null, resolved: null };
    s.queue = [];
  }
  return t;
};

// O valor que um parâmetro tinha quando o comando entrou na fila.
const glSnapshot = (s) => new Map(s.params);

// Copia na escrita: um buffer guardado por um desenho pendente é trocado antes de mudar.
const pinBuffer = (b) => { if (b) b.pinned = true; };
const unpinForWrite = (b) => { if (b.pinned) { b.data = b.data.slice(); b.pinned = false; } };

const glEnqueue = (s, entry) => {
  glTarget(s);
  if (entry.kind === 'clear' && glClearsAll(s, entry)) s.queue = [];
  s.queue.push(entry);
  s.target.resolved = null;
};
const glClearsAll = (s, entry) => {
  const p = entry.params;
  if (p.get('SCISSOR_TEST')) return false;
  if (!(entry.mask & GL.COLOR_BUFFER_BIT) || !p.get('COLOR_WRITEMASK').every(Boolean)) return false;
  if (s.attrs.depth && (!(entry.mask & GL.DEPTH_BUFFER_BIT) || !p.get('DEPTH_WRITEMASK'))) return false;
  if (s.attrs.stencil && !(entry.mask & GL.STENCIL_BUFFER_BIT)) return false;
  return true;
};

const glQueueClear = (s, mask) => glEnqueue(s, { kind: 'clear', mask, params: glSnapshot(s) });

const glQueueDraw = (ctx, mode, first, count, instances, elements) => {
  const s = glState(ctx);
  const program = objOf(s.params.get('CURRENT_PROGRAM'));
  const attribs = s.vao.attribs.map((a) => { const b = a.buffer && objOf(a.buffer); pinBuffer(b); return { ...a, data: b ? b.data : null }; });
  if (elements) pinBuffer(elements.buffer);  glEnqueue(s, {
    kind: 'draw', mode, first, count, instances, params: glSnapshot(s),
    program: { sources: program.sources, attribs: program.attribs, uniforms: program.uniforms, values: new Map(program.uniformValues), cache: program },
    attribs, current: s.current.slice(),
    elements: elements ? { data: elements.buffer.data, type: elements.type, offset: elements.offset } : null,
  });
};

// Roda a fila e devolve os pixels resolvidos (RGBA de 8 bits, linha 0 embaixo).
const glFlush = (s) => {
  const t = glTarget(s);
  if (s.queue.length) {
    for (const entry of s.queue) (entry.kind === 'clear' ? glRunClear : glRunDraw)(s, t, entry);
    s.queue = [];
    t.resolved = null;
  }
  if (!t.resolved) t.resolved = glResolve(t);
  return t.resolved;
};

// `float` para 8 bits como o SwiftShader: o produto por 255 em binário32, arredondado ao par mais próximo (0,3 vira
// 76,5 e sai 76; 0,1 vira 25,5 e sai 26).
const unorm8 = (c) => (c >= 1 ? 255 : c > 0 ? roundEven(fr(c * 255)) : 0);

const glRunClear = (s, t, entry) => {
  const p = entry.params;
  let [x0, y0, x1, y1] = [0, 0, t.width, t.height];
  if (p.get('SCISSOR_TEST')) { const b = p.get('SCISSOR_BOX'); x0 = Math.max(0, b[0]); y0 = Math.max(0, b[1]); x1 = Math.min(t.width, b[0] + b[2]); y1 = Math.min(t.height, b[1] + b[3]); }
  const n = t.samples.length;
  if (entry.mask & GL.COLOR_BUFFER_BIT) {
    const color = p.get('COLOR_CLEAR_VALUE');
    const mask = p.get('COLOR_WRITEMASK');
    const bytes = Array.from(color, (c, i) => (i === 3 && !s.attrs.alpha ? 255 : unorm8(c)));
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < n; k++) { const i = ((y * t.width + x) * n + k) * 4; for (let c = 0; c < 4; c++) if (mask[c]) t.color[i + c] = bytes[c]; }
  }
  if (t.depth && entry.mask & GL.DEPTH_BUFFER_BIT && p.get('DEPTH_WRITEMASK')) {
    const d = Math.min(1, Math.max(0, p.get('DEPTH_CLEAR_VALUE')));
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < n; k++) t.depth[(y * t.width + x) * n + k] = d;
  }
  if (t.stencil && entry.mask & GL.STENCIL_BUFFER_BIT) {
    const v = p.get('STENCIL_CLEAR_VALUE') & p.get('STENCIL_WRITEMASK') & 255;
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < n; k++) t.stencil[(y * t.width + x) * n + k] = v;
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
const glRunDraw = (s, t, entry) => {
  let stages;
  try { stages = glProgramStages(entry); } catch (error) { if (error instanceof GlslCompileError) return; throw error; }
  const { vs, fs, varyings } = stages;
  const vsGlobals = glStageGlobals(vs, entry);
  const fsGlobals = glStageGlobals(fs, entry);
  const vsMain = vs.functions.get('main')[0], fsMain = fs.functions.get('main')[0];
  const inputs = entry.program.attribs.map((a) => ({ slot: vs.globals.get(a.name)?.slot, location: a.location, t: vs.globals.get(a.name)?.t })).filter((a) => a.slot !== undefined);
  const posSlot = vs.globals.get('gl_Position').slot, idSlot = vs.globals.get('gl_VertexID').slot, instSlot = vs.globals.get('gl_InstanceID').slot;
  const rt = glRuntime(s);
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
      vsMain.body({ g, l: new Array(vsMain.count), rt });
      v = { pos: g[posSlot].slice(), vary: varyings.map((x) => glslClone(g[x.vs])) };
      cache.set(index, v);
      return v;
    };
    for (const prim of glPrimitives(entry.mode, indices)) {
      if (prim.length === 3) glTriangle(s, t, entry, stages, fsGlobals, fsMain, prim.map(shade), rt);
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

// Triângulos de cada modo, com a ordem de vértices que preserva o sentido (o último vértice provoca).
const glPrimitives = (mode, idx) => {
  const out = [];
  if (mode === GL.TRIANGLES) for (let i = 0; i + 2 < idx.length; i += 3) out.push([idx[i], idx[i + 1], idx[i + 2]]);
  else if (mode === GL.TRIANGLE_STRIP) for (let i = 0; i + 2 < idx.length; i++) out.push(i % 2 ? [idx[i + 1], idx[i], idx[i + 2]] : [idx[i], idx[i + 1], idx[i + 2]]);
  else if (mode === GL.TRIANGLE_FAN) for (let i = 1; i + 1 < idx.length; i++) out.push([idx[i], idx[i + 1], idx[0]]);
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
const glTriangle = (s, t, entry, stages, fsGlobals, fsMain, verts, rt) => {
  const p = entry.params;
  const provoking = verts[2];
  const tagged = verts.map((v, i) => ({ pos: v.pos, vary: v.vary, weights: [i === 0 ? 1 : 0, i === 1 ? 1 : 0, i === 2 ? 1 : 0] }));
  const poly = glClip(tagged);
  if (poly.length < 3) return;
  const vp = p.get('VIEWPORT'), range = p.get('DEPTH_RANGE');
  // A janela como o SetupRoutine do SwiftShader: X = RoundInt(X0xF + x * rhw * WxF), em 1/16 de pixel e binário32.
  const halfW = fr(vp[2] / 2), halfH = fr(vp[3] / 2);
  const X0xF = fr(fr(vp[0] + halfW) * SUBPIXEL), Y0xF = fr(fr(vp[1] + halfH) * SUBPIXEL), WxF = fr(halfW * SUBPIXEL), HxF = fr(halfH * SUBPIXEL);
  const win = poly.map((v) => {
    const w = v.pos[3];
    const rhw = w !== 0 ? fr(1 / w) : 1;
    const X = Math.round(fr(X0xF + fr(fr(v.pos[0] * rhw) * WxF))), Y = Math.round(fr(Y0xF + fr(fr(v.pos[1] * rhw) * HxF)));
    return { X, Y, x: X / SUBPIXEL, y: Y / SUBPIXEL, w, rhw, z: fr(fr(fr(v.pos[2] * rhw) * fr((range[1] - range[0]) / 2)) + fr((range[1] + range[0]) / 2)), vary: v.vary };
  });
  for (let i = 1; i + 1 < win.length; i++) glRasterTriangle(s, t, entry, stages, fsGlobals, fsMain, [win[0], win[i], win[i + 1]], provoking, rt);
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

const glRasterTriangle = (s, t, entry, stages, fsGlobals, fsMain, [a, b, c], provoking, rt) => {
  const p = entry.params;
  const area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
  if (area === 0) return;
  const ccw = area > 0;
  const front = (p.get('FRONT_FACE') === GL.CCW) === ccw;
  if (p.get('CULL_FACE')) { const mode = p.get('CULL_FACE_MODE'); if (mode === GL.FRONT_AND_BACK || (mode === GL.FRONT) === front) return; }
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
    return { dx, dy, x0: p0.x, y0: p0.y, topLeft: dy < 0 || (dy === 0 && dx > 0) };
  });
  let bx0 = Math.floor(Math.min(a.x, b.x, c.x)), bx1 = Math.ceil(Math.max(a.x, b.x, c.x));
  let by0 = Math.floor(Math.min(a.y, b.y, c.y)), by1 = Math.ceil(Math.max(a.y, b.y, c.y));
  const vp = p.get('VIEWPORT');
  bx0 = Math.max(bx0, 0, vp[0]); by0 = Math.max(by0, 0, vp[1]); bx1 = Math.min(bx1, t.width, vp[0] + vp[2]); by1 = Math.min(by1, t.height, vp[1] + vp[3]);
  if (p.get('SCISSOR_TEST')) { const sb = p.get('SCISSOR_BOX'); bx0 = Math.max(bx0, sb[0]); by0 = Math.max(by0, sb[1]); bx1 = Math.min(bx1, sb[0] + sb[2]); by1 = Math.min(by1, sb[1] + sb[3]); }
  const n = t.samples.length;
  // (x, y) em coordenadas do Vulkan.
  const inside = (x, y) => edges.every((e) => { const v = e.dx * (y - e.y0) - e.dy * (x - e.x0); return v > 0 || (v === 0 && e.topLeft); });
  // As equações de plano do SetupRoutine do SwiftShader, em binário32 e relativas ao vértice 0: cada interpolante é
  // P.x * dx + P.y * dy + P.z (o atributo sobre w), e o plano W dá 1/w; o fragmento multiplica pelo inverso dele.
  const x1 = fr(fr(b.w * (1 / SUBPIXEL)) * (b.X - a.X)), y1 = fr(fr(b.w * (1 / SUBPIXEL)) * (b.Y - a.Y));
  const x2 = fr(fr(c.w * (1 / SUBPIXEL)) * (c.X - a.X)), y2 = fr(fr(c.w * (1 / SUBPIXEL)) * (c.Y - a.Y));
  const det = fr(fr(x1 * y2) - fr(x2 * y1));
  if (det === 0) return;
  const A = fr(1 / det), D = fr(A * a.rhw);
  const M = [[fr(fr(fr(y1 * c.w) - fr(y2 * b.w)) * D), fr(fr(fr(x2 * b.w) - fr(x1 * c.w)) * D), a.rhw], [fr(y2 * A), fr(-x2 * A), 0], [fr(-y1 * A), fr(x1 * A), 0]];
  const plane = (i0, i1, i2) => [0, 1, 2].map((k) => fr(fr(fr(i0 * M[0][k]) + fr(i1 * M[1][k])) + fr(i2 * M[2][k])));
  const W = plane(1, 1, 1);
  // A origem dos planos é o vértice 0 deslocado de meio pixel (o centro do pixel fica nos inteiros), e cada linha
  // soma D = C + y * B antes do termo em x, como o QuadRasterizer.
  const ax = fr((a.X - SUBPIXEL / 2) / SUBPIXEL), ay = fr((a.Y - SUBPIXEL / 2) / SUBPIXEL);
  const planes = stages.varyings.map((v, i) => (v.flat ? null : glslMap3(a.vary[i], b.vary[i], c.vary[i], plane)));
  const zPlane = (() => {
    const z1 = fr(b.z - a.z), z2 = fr(c.z - a.z);
    const u1 = fr((b.X - a.X) / SUBPIXEL), v1 = fr((b.Y - a.Y) / SUBPIXEL), u2 = fr((c.X - a.X) / SUBPIXEL), v2 = fr((c.Y - a.Y) / SUBPIXEL);
    const Dz = fr(1 / fr(fr(u1 * v2) - fr(u2 * v1)));
    return [fr(fr(fr(v2 * z1) - fr(v1 * z2)) * Dz), fr(fr(fr(u1 * z2) - fr(u2 * z1)) * Dz), a.z];
  })();
  const evalPlane = (P, dx, dy) => fr(fr(dx * P[0]) + fr(P[2] + fr(dy * P[1])));
  const varyings = stages.varyings;
  const fragSlot = stages.fragOut.slot;
  const coordSlot = stages.fs.globals.get('gl_FragCoord').slot, facingSlot = stages.fs.globals.get('gl_FrontFacing').slot;
  const depthTest = t.depth && p.get('DEPTH_TEST'), depthWrite = depthTest && p.get('DEPTH_WRITEMASK'), depthFn = DEPTH_FUNCS[p.get('DEPTH_FUNC')];
  const mask = p.get('COLOR_WRITEMASK');
  const blend = p.get('BLEND');
  const constant = p.get('BLEND_COLOR');
  const [srcRgb, dstRgb, srcA, dstA] = ['BLEND_SRC_RGB', 'BLEND_DST_RGB', 'BLEND_SRC_ALPHA', 'BLEND_DST_ALPHA'].map((k) => BLEND_FACTORS[p.get(k)]);
  const [eqRgb, eqA] = ['BLEND_EQUATION_RGB', 'BLEND_EQUATION_ALPHA'].map((k) => BLEND_EQUATIONS[p.get(k)]);
  const alphaBits = s.attrs.alpha;
  for (let y = by0; y < by1; y++) for (let x = bx0; x < bx1; x++) {
    let covered = 0;
    for (let k = 0; k < n; k++) { const [sx, sy] = t.samples[k]; if (inside(x + sx, y + sy)) covered |= 1 << k; }
    if (!covered) continue;
    // O fragmento: interpolantes no centro do pixel, com correção de perspectiva pelo plano W.
    const dx = fr(x - ax), dy = fr(y - ay);
    const rhwPix = evalPlane(W, dx, dy);
    const wPix = fr(1 / rhwPix);
    const z = evalPlane(zPlane, dx, dy);
    const g = fsGlobals.map(glslClone);
    varyings.forEach((v, i) => {
      g[v.fs] = v.flat ? glslClone(provoking.vary[i]) : glslMapPlane(planes[i], (P) => fr(evalPlane(P, dx, dy) * wPix));
    });
    g[coordSlot] = [fr(x + 0.5), fr(y + 0.5), z, rhwPix];
    g[facingSlot] = front;
    rt.discarded = false;
    fsMain.body({ g, l: new Array(fsMain.count), rt });
    if (rt.discarded) continue;
    const out = stages.usesFragData && !g[fragSlot].some((x) => x !== 0) ? g[stages.fs.globals.get('gl_FragData').slot][0] : g[fragSlot];
    const src = out.map((v) => Math.min(1, Math.max(0, v)));
    for (let k = 0; k < n; k++) {
      if (!(covered & (1 << k))) continue;
      const si = (y * t.width + x) * n + k;
      if (depthTest) {
        const [sx, sy] = t.samples[k];
        const zs = Math.min(1, Math.max(0, evalPlane(zPlane, fr(fr(x + sx - 0.5) - ax), fr(fr(y + sy - 0.5) - ay))));
        if (!depthFn(zs, t.depth[si])) continue;
        if (depthWrite) t.depth[si] = zs;
      }
      const ci = si * 4;
      let rgba = src;
      if (blend) {
        const dst = [t.color[ci] / 255, t.color[ci + 1] / 255, t.color[ci + 2] / 255, alphaBits ? t.color[ci + 3] / 255 : 1];
        rgba = [0, 1, 2, 3].map((ch) => {
          const sf = (ch === 3 ? srcA : srcRgb)(src, dst, ch, constant), df = (ch === 3 ? dstA : dstRgb)(src, dst, ch, constant);
          return Math.min(1, Math.max(0, (ch === 3 ? eqA : eqRgb)(fr(src[ch] * sf), fr(dst[ch] * df), src[ch], dst[ch])));
        });
      }
      for (let ch = 0; ch < 4; ch++) if (mask[ch]) t.color[ci + ch] = ch === 3 && !alphaBits ? 255 : unorm8(rgba[ch]);
    }
  }
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

// O que os sombreadores pedem ao ambiente: amostragem (texturas ainda sem dados: o valor de textura incompleta) e
// derivadas (zero, sem quadras de pixels).
const glRuntime = () => ({
  discarded: false,
  sample: (unit, type) => (/^[iu]/.test(type) ? [0, 0, 0, 1] : /Shadow$/.test(type) ? 0 : [0, 0, 0, 1]),
  texelFetch: () => [0, 0, 0, 1],
  textureSize: () => [0, 0],
  derivative: (f, e) => { const v = f(e); return Array.isArray(v) ? v.map(() => 0) : 0; },
});
