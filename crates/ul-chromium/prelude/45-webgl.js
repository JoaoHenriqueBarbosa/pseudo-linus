// ---- WebGL e WebGL2: o contexto do Chromium com --disable-gpu (ANGLE sobre SwiftShader), em estado. Constantes,
// parâmetros iniciais, limites, extensões, atributos de contexto e precisões vêm da tabela medida no oráculo
// (`data/webgl-swiftshader.json`, gerada por `testbench/oracle/measured/webgl-swiftshader.html`); o resto é a máquina de
// estado da especificação, com os erros que o Chromium gera. Desenho não rasteriza: os pixels lidos saem do buffer de
// desenho, que só o `clear` toca.

class WebGLObject {
  constructor() { throw new TypeError('Illegal constructor'); }
}
tag(WebGLObject, 'WebGLObject');
const glClass = (name, base = WebGLObject) => {
  const cls = class extends base { constructor() { throw new TypeError('Illegal constructor'); } };
  Object.defineProperty(cls, 'name', { value: name });
  tag(cls, name);
  return cls;
};
const WebGLBuffer = glClass('WebGLBuffer');
const WebGLFramebuffer = glClass('WebGLFramebuffer');
const WebGLProgram = glClass('WebGLProgram');
const WebGLQuery = glClass('WebGLQuery');
const WebGLRenderbuffer = glClass('WebGLRenderbuffer');
const WebGLSampler = glClass('WebGLSampler');
const WebGLShader = glClass('WebGLShader');
const WebGLSync = glClass('WebGLSync');
const WebGLTexture = glClass('WebGLTexture');
const WebGLTransformFeedback = glClass('WebGLTransformFeedback');
const WebGLVertexArrayObject = glClass('WebGLVertexArrayObject');
const WebGLVertexArrayObjectOES = glClass('WebGLVertexArrayObjectOES');
const WebGLUniformLocation = glClass('WebGLUniformLocation', Object);
class WebGLActiveInfo {
  constructor() { throw new TypeError('Illegal constructor'); }
  get size() { return this[P.__info].size; }
  get type() { return this[P.__info].type; }
  get name() { return this[P.__info].name; }
}
tag(WebGLActiveInfo, 'WebGLActiveInfo');
class WebGLShaderPrecisionFormat {
  constructor() { throw new TypeError('Illegal constructor'); }
  get rangeMin() { return this[P.__info][0]; }
  get rangeMax() { return this[P.__info][1]; }
  get precision() { return this[P.__info][2]; }
}
tag(WebGLShaderPrecisionFormat, 'WebGLShaderPrecisionFormat');
class WebGLContextEvent extends Event {
  constructor(type, init = {}) { super(type, init); hidden(this, '__statusMessage', init.statusMessage === undefined ? '' : String(init.statusMessage)); }
  get statusMessage() { return this[P.__statusMessage]; }
}
tag(WebGLContextEvent, 'WebGLContextEvent');
const withInfo = (cls, info) => { const o = Object.create(cls.prototype); hidden(o, '__info', info); return o; };

// Os números de enum que a máquina de estado usa, pelo nome da tabela.
const GL = WEBGL_TABLE.webgl2.consts;
const GL_ERRORS = { INVALID_ENUM: GL.INVALID_ENUM, INVALID_VALUE: GL.INVALID_VALUE, INVALID_OPERATION: GL.INVALID_OPERATION, INVALID_FRAMEBUFFER_OPERATION: GL.INVALID_FRAMEBUFFER_OPERATION, OUT_OF_MEMORY: GL.OUT_OF_MEMORY };
const CAPABILITIES = ['BLEND', 'CULL_FACE', 'DEPTH_TEST', 'DITHER', 'POLYGON_OFFSET_FILL', 'SAMPLE_ALPHA_TO_COVERAGE', 'SAMPLE_COVERAGE', 'SCISSOR_TEST', 'STENCIL_TEST'];
const CAPABILITIES2 = [...CAPABILITIES, 'RASTERIZER_DISCARD'];
// Tipos de GLSL para os enums de getActiveUniform/getActiveAttrib.
const GLSL_TYPES = {
  float: 'FLOAT', vec2: 'FLOAT_VEC2', vec3: 'FLOAT_VEC3', vec4: 'FLOAT_VEC4', int: 'INT', ivec2: 'INT_VEC2', ivec3: 'INT_VEC3', ivec4: 'INT_VEC4',
  uint: 'UNSIGNED_INT', uvec2: 'UNSIGNED_INT_VEC2', uvec3: 'UNSIGNED_INT_VEC3', uvec4: 'UNSIGNED_INT_VEC4', bool: 'BOOL', bvec2: 'BOOL_VEC2', bvec3: 'BOOL_VEC3', bvec4: 'BOOL_VEC4',
  mat2: 'FLOAT_MAT2', mat3: 'FLOAT_MAT3', mat4: 'FLOAT_MAT4', mat2x3: 'FLOAT_MAT2x3', mat2x4: 'FLOAT_MAT2x4', mat3x2: 'FLOAT_MAT3x2', mat3x4: 'FLOAT_MAT3x4', mat4x2: 'FLOAT_MAT4x2', mat4x3: 'FLOAT_MAT4x3',
  mat2x2: 'FLOAT_MAT2', mat3x3: 'FLOAT_MAT3', mat4x4: 'FLOAT_MAT4',
  sampler2D: 'SAMPLER_2D', samplerCube: 'SAMPLER_CUBE', sampler3D: 'SAMPLER_3D', sampler2DArray: 'SAMPLER_2D_ARRAY', sampler2DShadow: 'SAMPLER_2D_SHADOW', samplerCubeShadow: 'SAMPLER_CUBE_SHADOW', sampler2DArrayShadow: 'SAMPLER_2D_ARRAY_SHADOW',
  isampler2D: 'INT_SAMPLER_2D', isampler3D: 'INT_SAMPLER_3D', isamplerCube: 'INT_SAMPLER_CUBE', isampler2DArray: 'INT_SAMPLER_2D_ARRAY',
  usampler2D: 'UNSIGNED_INT_SAMPLER_2D', usampler3D: 'UNSIGNED_INT_SAMPLER_3D', usamplerCube: 'UNSIGNED_INT_SAMPLER_CUBE', usampler2DArray: 'UNSIGNED_INT_SAMPLER_2D_ARRAY',
};
// Quantos componentes cada tipo guarda (o tamanho do valor de getUniform).
const typeComponents = (t) => {
  const m = /^(?:FLOAT|INT|UNSIGNED_INT|BOOL)(?:_VEC(\d))?$/.exec(t);
  if (m) return Number(m[1] || 1);
  const mat = /^FLOAT_MAT(\d)(?:x(\d))?$/.exec(t);
  if (mat) return Number(mat[1]) * Number(mat[2] || mat[1]);
  return 1;
};

// ---- GLSL: o suficiente do pré-processador e das declarações para o link responder como o ANGLE (variáveis ativas,
// locais, blocos). Comentários saem, as diretivas `#if` não são avaliadas (os shaders de verdade raramente escondem
// declarações atrás delas).
const stripComments = (src) => src.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' ')).replace(/\/\/[^\n]*/g, '');
const glslVersion = (src) => (/^\s*#version\s+300\s+es\b/.test(stripComments(src)) ? 300 : 100);
const parseShader = (source, kind) => {
  const src = stripComments(source);
  const version = glslVersion(source);
  const structs = new Map();
  for (const m of src.matchAll(/\bstruct\s+(\w+)\s*\{([^}]*)\}/g)) structs.set(m[1], parseMembers(m[2]));
  const body = src.replace(/^\s*#.*$/gm, '');
  const decls = [];
  const blocks = [];
  // Blocos de uniform (WebGL2): `layout(std140) uniform Nome { ... } instância;`
  const withoutBlocks = body.replace(/(?:layout\s*\(([^)]*)\)\s*)?\buniform\s+(\w+)\s*\{([^}]*)\}\s*(\w+)?\s*(?:\[\s*(\d+)\s*\])?\s*;/g, (all, layout, name, members, instance) => {
    blocks.push({ name, members: parseMembers(members), instance: instance || null, binding: layoutValue(layout, 'binding') });
    return ' ';
  });
  const re = /(?:layout\s*\(([^)]*)\)\s*)?(?:\b(?:flat|smooth|centroid|invariant)\s+)*\b(uniform|attribute|in|out|varying)\s+(?:(?:lowp|mediump|highp)\s+)?(\w+)\s+([^;{()]+);/g;
  for (const m of withoutBlocks.matchAll(re)) {
    const [, layout, qualifier, type] = m;
    for (const piece of m[4].split(',')) {
      const v = /^\s*(\w+)\s*(?:\[\s*(\w+)\s*\])?\s*$/.exec(piece);
      if (!v) continue;
      decls.push({ qualifier, type, name: v[1], array: v[2] === undefined ? 0 : Number(v[2]) || 1, location: layoutValue(layout, 'location') });
    }
  }
  return { version, kind, decls, blocks, structs, body: withoutBlocks };
};
const parseMembers = (text) => {
  const out = [];
  for (const m of text.matchAll(/(?:(?:lowp|mediump|highp)\s+)?(\w+)\s+([^;]+);/g)) {
    for (const piece of m[2].split(',')) {
      const v = /^\s*(\w+)\s*(?:\[\s*(\w+)\s*\])?\s*$/.exec(piece);
      if (v) out.push({ type: m[1], name: v[1], array: v[2] === undefined ? 0 : Number(v[2]) || 1 });
    }
  }
  return out;
};
const layoutValue = (layout, key) => {
  if (!layout) return -1;
  const m = new RegExp('\\b' + key + '\\s*=\\s*(\\d+)').exec(layout);
  return m ? Number(m[1]) : -1;
};
// Variável ativa: o nome aparece fora da própria declaração em algum shader do programa. O ANGLE corta as inativas.
const usedIn = (name, shaders) => shaders.some((s) => (s.body.match(new RegExp('\\b' + name + '\\b', 'g')) || []).length > (s.declared.has(name) ? 1 : 0));
// Expande structs e arrays no formato dos nomes ativos: `luz.cor`, `vetor[0]`.
const flattenUniform = (decl, structs, prefix = '') => {
  const name = prefix + decl.name;
  const struct = structs.get(decl.type);
  if (struct) {
    const out = [];
    const count = decl.array || 1;
    for (let i = 0; i < count; i++) for (const member of struct) out.push(...flattenUniform(member, structs, decl.array ? `${name}[${i}].` : name + '.'));
    return out;
  }
  const type = GLSL_TYPES[decl.type];
  if (!type) return [];
  return [{ name: decl.array ? name + '[0]' : name, base: name, type: GL[type], typeName: type, size: decl.array || 1 }];
};

// ---- O contexto.
const glState = (ctx) => ctx[P.__gl];
const glError = (ctx, code) => { const s = glState(ctx); if (!s.errors.includes(code)) s.errors.push(code); };
const mkObject = (ctx, cls, fields = {}) => { const o = Object.create(cls.prototype); hidden(o, '__obj', { ctx, deleted: false, ...fields }); return o; };
const objOf = (o) => (o && typeof o === 'object' ? o[P.__obj] : undefined);
// Objeto de outro contexto: INVALID_OPERATION, como o Chromium. Nulo vale como desvincular.
const validObject = (ctx, o, cls, method) => {
  if (o === null) return true;
  if (!(o instanceof cls)) throw new TypeError(`Failed to execute '${method}' on '${ctx[P.__glName]}': parameter 2 is not of type '${cls.name}'.`);
  const s = objOf(o);
  if (s.ctx !== ctx) { glError(ctx, GL.INVALID_OPERATION); return false; }
  if (s.deleted) { glError(ctx, GL.INVALID_OPERATION); return false; }
  return true;
};
const copyValue = (v) => (ArrayBuffer.isView(v) ? v.slice() : Array.isArray(v) ? v.slice() : v);
const tableValue = (v) => {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) return v;
  const type = v.type.slice(8, -1);
  if (type === 'Array') return String(v.value).split(',').map((x) => x === 'true');
  return new globalThis[type](v.value);
};

const newGlState = (ctx, version, canvas, attrs) => {
  const table = WEBGL_TABLE[version === 2 ? 'webgl2' : 'webgl'];
  const params = new Map();
  for (const [name, value] of Object.entries(table.params)) if (!name.startsWith('prec ')) params.set(name, tableValue(value));
  const width = canvas.width, height = canvas.height;
  params.set('VIEWPORT', new Int32Array([0, 0, width, height]));
  params.set('SCISSOR_BOX', new Int32Array([0, 0, width, height]));
  if (attrs.stencil) params.set('STENCIL_BITS', 8);
  if (!attrs.depth) params.set('DEPTH_BITS', 0);
  if (!attrs.alpha) params.set('ALPHA_BITS', 0);
  if (!attrs.antialias) { params.set('SAMPLE_BUFFERS', 0); params.set('SAMPLES', 0); }
  const units = [];
  for (let i = 0; i < table.params.MAX_COMBINED_TEXTURE_IMAGE_UNITS; i++) units.push({ TEXTURE_2D: null, TEXTURE_CUBE_MAP: null, TEXTURE_3D: null, TEXTURE_2D_ARRAY: null, sampler: null });
  const attribs = [];
  for (let i = 0; i < table.params.MAX_VERTEX_ATTRIBS; i++) attribs.push(newAttrib());
  const defaultVao = { attribs, elementBuffer: null };
  return {
    version, table, canvas, attrs, params, units, vao: defaultVao, defaultVao, current: attribs.map(() => new Float32Array([0, 0, 0, 1])),
    errors: [], lost: false, extensions: new Map(), indexed: new Map(), queries: new Map(),
    pixels: null, width, height,
  };
};
const newAttrib = () => ({ enabled: false, size: 4, type: GL.FLOAT, normalized: false, stride: 0, offset: 0, buffer: null, divisor: 0, integer: false });

// O valor de getParameter que segue as ligações correntes.
const bindingParam = {
  TEXTURE_BINDING_2D: (s) => s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0].TEXTURE_2D,
  TEXTURE_BINDING_CUBE_MAP: (s) => s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0].TEXTURE_CUBE_MAP,
  TEXTURE_BINDING_3D: (s) => s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0].TEXTURE_3D,
  TEXTURE_BINDING_2D_ARRAY: (s) => s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0].TEXTURE_2D_ARRAY,
  SAMPLER_BINDING: (s) => s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0].sampler,
  ELEMENT_ARRAY_BUFFER_BINDING: (s) => s.vao.elementBuffer,
};
const BUFFER_TARGETS = { ARRAY_BUFFER: 'ARRAY_BUFFER_BINDING', ELEMENT_ARRAY_BUFFER: null, COPY_READ_BUFFER: 'COPY_READ_BUFFER_BINDING', COPY_WRITE_BUFFER: 'COPY_WRITE_BUFFER_BINDING', PIXEL_PACK_BUFFER: 'PIXEL_PACK_BUFFER_BINDING', PIXEL_UNPACK_BUFFER: 'PIXEL_UNPACK_BUFFER_BINDING', TRANSFORM_FEEDBACK_BUFFER: 'TRANSFORM_FEEDBACK_BUFFER_BINDING', UNIFORM_BUFFER: 'UNIFORM_BUFFER_BINDING' };
const WEBGL1_BUFFER_TARGETS = ['ARRAY_BUFFER', 'ELEMENT_ARRAY_BUFFER'];
const enumName = (s, value, allowed) => allowed.find((name) => GL[name] === value);
const bufferTarget = (ctx, target) => {
  const s = glState(ctx);
  const name = enumName(s, target, s.version === 2 ? Object.keys(BUFFER_TARGETS) : WEBGL1_BUFFER_TARGETS);
  if (!name) glError(ctx, GL.INVALID_ENUM);
  return name;
};
const boundBuffer = (s, name) => (name === 'ELEMENT_ARRAY_BUFFER' ? s.vao.elementBuffer : s.params.get(BUFFER_TARGETS[name]));
const TEXTURE_TARGETS = { TEXTURE_2D: 'TEXTURE_2D', TEXTURE_CUBE_MAP: 'TEXTURE_CUBE_MAP', TEXTURE_3D: 'TEXTURE_3D', TEXTURE_2D_ARRAY: 'TEXTURE_2D_ARRAY' };
const textureTarget = (ctx, target) => {
  const s = glState(ctx);
  const name = enumName(s, target, s.version === 2 ? Object.keys(TEXTURE_TARGETS) : ['TEXTURE_2D', 'TEXTURE_CUBE_MAP']);
  if (!name) glError(ctx, GL.INVALID_ENUM);
  return name;
};
const imageTarget = (ctx, target) => {
  if (target === GL.TEXTURE_2D) return 'TEXTURE_2D';
  if (target >= GL.TEXTURE_CUBE_MAP_POSITIVE_X && target <= GL.TEXTURE_CUBE_MAP_NEGATIVE_Z) return 'TEXTURE_CUBE_MAP';
  glError(ctx, GL.INVALID_ENUM);
  return undefined;
};
const boundTexture = (s, name) => s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0][name];
const newTextureParams = () => new Map([
  ['TEXTURE_MIN_FILTER', GL.NEAREST_MIPMAP_LINEAR], ['TEXTURE_MAG_FILTER', GL.LINEAR], ['TEXTURE_WRAP_S', GL.REPEAT], ['TEXTURE_WRAP_T', GL.REPEAT],
  ['TEXTURE_WRAP_R', GL.REPEAT], ['TEXTURE_MIN_LOD', -1000], ['TEXTURE_MAX_LOD', 1000], ['TEXTURE_BASE_LEVEL', 0], ['TEXTURE_MAX_LEVEL', 1000],
  ['TEXTURE_COMPARE_MODE', GL.NONE], ['TEXTURE_COMPARE_FUNC', GL.LEQUAL], ['TEXTURE_IMMUTABLE_FORMAT', false], ['TEXTURE_IMMUTABLE_LEVELS', 0],
]);
const newSamplerParams = () => new Map([
  ['TEXTURE_MIN_FILTER', GL.NEAREST_MIPMAP_LINEAR], ['TEXTURE_MAG_FILTER', GL.LINEAR], ['TEXTURE_WRAP_S', GL.REPEAT], ['TEXTURE_WRAP_T', GL.REPEAT],
  ['TEXTURE_WRAP_R', GL.REPEAT], ['TEXTURE_MIN_LOD', -1000], ['TEXTURE_MAX_LOD', 1000], ['TEXTURE_COMPARE_MODE', GL.NONE], ['TEXTURE_COMPARE_FUNC', GL.LEQUAL],
]);
const paramName = (map, pname) => Array.from(map.keys()).find((k) => GL[k] === pname);
// Dimensões de uma imagem fonte de texImage2D (ImageData, canvas, imagem, vídeo, bitmap).
const sourceSize = (src) => {
  if (src === null || src === undefined) return null;
  if (typeof src.videoWidth === 'number') return [src.videoWidth, src.videoHeight];
  if (typeof src.naturalWidth === 'number') return [src.naturalWidth, src.naturalHeight];
  if (typeof src.width === 'number') return [src.width, src.height];
  return null;
};

// O buffer de desenho padrão: os pixels resolvidos depois de rodar a fila (`48-webgl-raster.js`).
const drawingBuffer = (s) => glFlush(s);

const linkProgram = (ctx, program) => {
  const s = glState(ctx);
  const p = objOf(program);
  const shaders = p.shaders.map(objOf);
  const vertex = shaders.find((sh) => sh.type === GL.VERTEX_SHADER);
  const fragment = shaders.find((sh) => sh.type === GL.FRAGMENT_SHADER);
  p.linked = false;
  p.attribs = []; p.uniforms = []; p.blocks = []; p.uniformValues = new Map();
  if (!vertex || !fragment) { p.log = 'Missing ' + (vertex ? 'fragment' : 'vertex') + ' shader.\n'; return; }
  if (!vertex.compiled || !fragment.compiled) { p.log = 'Attached shader is not compiled.\n'; return; }
  const vs = vertex.parsed, fs = fragment.parsed;
  if (vs.version !== fs.version) { p.log = 'Version mismatch between shaders.\n'; return; }
  for (const sh of [vs, fs]) sh.declared = new Set(sh.decls.map((d) => d.name));
  const inputs = vs.decls.filter((d) => d.qualifier === (vs.version === 300 ? 'in' : 'attribute'));
  // Locais: layout(location), depois bindAttribLocation, depois a menor livre.
  const taken = new Set();
  const placed = [];
  for (const d of inputs) {
    if (!usedIn(d.name, [vs])) continue;
    const type = GLSL_TYPES[d.type];
    if (!type) continue;
    const slots = /MAT(\d)/.test(type) ? Number(/MAT(\d)/.exec(type)[1]) : 1;
    let location = d.location >= 0 ? d.location : p.bound.has(d.name) ? p.bound.get(d.name) : -1;
    placed.push({ d, type, slots, location });
    if (location >= 0) for (let i = 0; i < slots; i++) taken.add(location + i);
  }
  for (const a of placed) {
    if (a.location < 0) {
      let loc = 0;
      while (Array.from({ length: a.slots }, (_, i) => taken.has(loc + i)).some(Boolean)) loc++;
      a.location = loc;
      for (let i = 0; i < a.slots; i++) taken.add(loc + i);
    }
    p.attribs.push({ name: a.d.name, type: GL[a.type], size: a.d.array || 1, location: a.location });
  }
  const seen = new Set();
  for (const sh of [vs, fs]) {
    for (const d of sh.decls) {
      if (d.qualifier !== 'uniform' || seen.has(d.name) || !usedIn(d.name, [vs, fs])) continue;
      seen.add(d.name);
      for (const u of flattenUniform(d, sh.structs)) p.uniforms.push(u);
    }
    for (const b of sh.blocks) {
      if (p.blocks.some((x) => x.name === b.name)) continue;
      const members = b.members.flatMap((m) => flattenUniform(m, sh.structs, b.instance ? b.name + '.' : ''));
      p.blocks.push({ name: b.name, binding: Math.max(b.binding, 0), members, size: std140Size(b.members, sh.structs), vertex: vs.blocks.some((x) => x.name === b.name), fragment: fs.blocks.some((x) => x.name === b.name) });
      for (const m of members) p.uniforms.push({ ...m, block: p.blocks.length - 1 });
    }
  }
  p.log = '';
  p.sources = [vertex.source, fragment.source];
  p.linked = true;
  p.generation++;
};
// Tamanho std140 de um bloco: escalar 4, vec2 8, vec3/vec4 16, matriz por colunas de vec4, array com passo de 16.
const std140Size = (members, structs) => {
  let offset = 0;
  const align = (a) => { offset = Math.ceil(offset / a) * a; };
  for (const m of members) {
    const type = GLSL_TYPES[m.type];
    let size, alignment;
    if (structs.has(m.type)) { alignment = 16; size = Math.ceil(std140Size(structs.get(m.type), structs) / 16) * 16; } else {
      const mat = /^FLOAT_MAT(\d)(?:x(\d))?$/.exec(type || '');
      const vec = /_VEC(\d)$/.exec(type || '');
      if (mat) { alignment = 16; size = 16 * Number(mat[1]); } else if (vec) { const n = Number(vec[1]); alignment = n === 2 ? 8 : 16; size = 4 * n; } else { alignment = 4; size = 4; }
    }
    if (m.array) { alignment = 16; size = Math.ceil(size / 16) * 16 * m.array; }
    align(alignment);
    offset += size;
  }
  align(16);
  return offset;
};
const programOf = (ctx, program, method) => {
  if (!(program instanceof WebGLProgram)) throw new TypeError(`Failed to execute '${method}' on '${ctx[P.__glName]}': parameter 1 is not of type 'WebGLProgram'.`);
  const p = objOf(program);
  if (p.ctx !== ctx || p.deleted) { glError(ctx, GL.INVALID_VALUE); return null; }
  return p;
};
const shaderOf = (ctx, shader, method, index = 1) => {
  if (!(shader instanceof WebGLShader)) throw new TypeError(`Failed to execute '${method}' on '${ctx[P.__glName]}': parameter ${index} is not of type 'WebGLShader'.`);
  const sh = objOf(shader);
  if (sh.ctx !== ctx || sh.deleted) { glError(ctx, GL.INVALID_VALUE); return null; }
  return sh;
};
// Grava um uniform: o local precisa ser do programa em uso e da geração atual do link.
const setUniform = (ctx, location, values, kind) => {
  if (location === null || location === undefined) return;
  const s = glState(ctx);
  const loc = location[P.__info];
  const current = s.params.get('CURRENT_PROGRAM');
  if (!current) { glError(ctx, GL.INVALID_OPERATION); return; }
  const p = objOf(current);
  if (loc.program !== p || loc.generation !== p.generation) { glError(ctx, GL.INVALID_OPERATION); return; }
  const u = p.uniforms[loc.index];
  const n = typeComponents(u.typeName);
  const sampler = /SAMPLER/.test(u.typeName);
  const expected = sampler || /^(INT|BOOL)/.test(u.typeName) ? 'i' : /^UNSIGNED/.test(u.typeName) ? 'ui' : 'f';
  if (kind.count !== n && !(kind.vector && values.length % n === 0 && kind.count === n)) { glError(ctx, GL.INVALID_OPERATION); return; }
  if (kind.type !== expected && !(/^BOOL/.test(u.typeName))) { glError(ctx, GL.INVALID_OPERATION); return; }
  const count = Math.min(values.length / n, u.size - loc.element);
  for (let i = 0; i < count; i++) p.uniformValues.set(loc.index + ':' + (loc.element + i), Array.from(values.slice(i * n, i * n + n)));
};

// O comportamento das extensões; interface, constantes, métodos e aridades vêm de `data/webgl-extensions.json`. Os
// métodos sem entrada aqui só existem (o ANGLE os aceita e não muda nada que a página leia).
const EXTENSION_SPECS = {
  ANGLE_instanced_arrays: { methods: (ctx) => ({ drawArraysInstancedANGLE(m, f, c, n) { drawArrays(ctx, m, f, c, n); }, drawElementsInstancedANGLE(m, c, t, o, n) { drawElements(ctx, m, c, t, o, n); }, vertexAttribDivisorANGLE(i, d) { divisor(ctx, i, d); } }) },
  EXT_texture_filter_anisotropic: { params: { MAX_TEXTURE_MAX_ANISOTROPY_EXT: 16 } },
  OES_standard_derivatives: { params: { FRAGMENT_SHADER_DERIVATIVE_HINT_OES: 4352 } },
  OES_vertex_array_object: { methods: (ctx) => ({ createVertexArrayOES() { return createVao(ctx, WebGLVertexArrayObjectOES); }, deleteVertexArrayOES(v) { deleteVao(ctx, v); }, isVertexArrayOES(v) { return isObject(ctx, v, WebGLVertexArrayObjectOES); }, bindVertexArrayOES(v) { bindVao(ctx, v); } }), params: { VERTEX_ARRAY_BINDING_OES: null } },
  WEBGL_compressed_texture_astc: { methods: () => ({ getSupportedProfiles() { return ['ldr']; } }) },
  WEBGL_debug_renderer_info: { params: (t) => ({ UNMASKED_VENDOR_WEBGL: t.renderer[0], UNMASKED_RENDERER_WEBGL: t.renderer[1] }) },
  WEBGL_debug_shaders: { methods: (ctx) => ({ getTranslatedShaderSource(shader) { const sh = shaderOf(ctx, shader, 'getTranslatedShaderSource'); return sh ? sh.source : null; } }) },
  WEBGL_draw_buffers: { methods: (ctx) => ({ drawBuffersWEBGL(list) { glState(ctx).params.set('DRAW_BUFFER0', list[0]); } }) },
  WEBGL_lose_context: { methods: (ctx) => ({ loseContext() { loseContext(ctx); }, restoreContext() { glError(ctx, GL.INVALID_OPERATION); } }) },
};
// Uma classe por interface, compartilhada pelos contextos; o contexto dono fica no objeto.
const extensionClasses = new Map();
const extensionClass = (name) => {
  let cls = extensionClasses.get(name);
  if (cls) return cls;
  const info = WEBGL_EXTENSIONS.extensions[name];
  cls = class { constructor() { throw new TypeError('Illegal constructor'); } };
  Object.defineProperty(cls, 'name', { value: info.interface });
  for (const [member, value] of info.members) {
    if (/^[A-Z0-9_x]+$/.test(member) && !/[a-wyz]/.test(member)) { Object.defineProperty(cls.prototype, member, { value, enumerable: true }); continue; }
    const method = { [member]: function (...args) { const impl = this[P.__ext][member]; return impl ? impl(...args) : undefined; } }[member];
    Object.defineProperty(method, 'length', { value });
    Object.defineProperty(cls.prototype, member, { value: method, writable: true, enumerable: true, configurable: true });
  }
  tag(cls, info.interface);
  extensionClasses.set(name, cls);
  return cls;
};
const extensionObject = (ctx, name) => {
  const spec = EXTENSION_SPECS[name] || {};
  const ext = Object.create(extensionClass(name).prototype);
  hidden(ext, '__ext', spec.methods ? spec.methods(ctx) : {});
  const s = glState(ctx);
  const params = typeof spec.params === 'function' ? spec.params(s.table) : spec.params || {};
  for (const [k, v] of Object.entries(params)) s.params.set(k, v);
  for (const [member, value] of WEBGL_EXTENSIONS.extensions[name].members) if (!(member in GL) && params[member] !== undefined) s.extConsts.set(value, member);
  return ext;
};
const loseContext = (ctx) => {
  const s = glState(ctx);
  if (s.lost) return;
  s.lost = true;
  s.errors = [GL.CONTEXT_LOST_WEBGL];
  setTimeout(() => { s.canvas.dispatchEvent(new WebGLContextEvent('webglcontextlost', { cancelable: true, statusMessage: '' })); }, 0);
};
const isObject = (ctx, o, cls) => o instanceof cls && objOf(o).ctx === ctx && !objOf(o).deleted && objOf(o).used !== false;
const createVao = (ctx, cls) => mkObject(ctx, cls, { attribs: glState(ctx).table.params.MAX_VERTEX_ATTRIBS ? Array.from({ length: glState(ctx).table.params.MAX_VERTEX_ATTRIBS }, newAttrib) : [], elementBuffer: null, used: false });
const bindVao = (ctx, vao) => {
  const s = glState(ctx);
  if (vao !== null && (objOf(vao).ctx !== ctx || objOf(vao).deleted)) { glError(ctx, GL.INVALID_OPERATION); return; }
  s.vao = vao === null ? s.defaultVao : objOf(vao);
  if (vao) s.vao.used = true;
  s.params.set('VERTEX_ARRAY_BINDING', vao);
  if (s.params.has('VERTEX_ARRAY_BINDING_OES')) s.params.set('VERTEX_ARRAY_BINDING_OES', vao);
};
const deleteVao = (ctx, vao) => {
  if (!vao || objOf(vao).deleted) return;
  const s = glState(ctx);
  if (s.vao === objOf(vao)) bindVao(ctx, null);
  objOf(vao).deleted = true;
};
const divisor = (ctx, index, value) => {
  const s = glState(ctx);
  if (index >= s.vao.attribs.length) { glError(ctx, GL.INVALID_VALUE); return; }
  s.vao.attribs[index].divisor = value >>> 0;
};
const deleteObject = (ctx, o, cls, method, unbind) => {
  if (o === null || o === undefined) return;
  if (!validObject(ctx, o, cls, method)) return;
  const st = objOf(o);
  st.deleted = true;
  if (unbind) unbind(glState(ctx), o);
};
const unbindEverywhere = (s, o) => {
  for (const [k, v] of s.params) if (v === o) s.params.set(k, null);
  for (const u of s.units) for (const k of Object.keys(u)) if (u[k] === o) u[k] = null;
  if (s.vao.elementBuffer === o) s.vao.elementBuffer = null;
  for (const a of s.vao.attribs) if (a.buffer === o) a.buffer = null;
};
const DRAW_MODES = ['POINTS', 'LINE_STRIP', 'LINE_LOOP', 'LINES', 'TRIANGLE_STRIP', 'TRIANGLE_FAN', 'TRIANGLES'];
const drawArrays = (ctx, mode, first, count, instances) => {
  const s = glState(ctx);
  if (s.lost) return;
  if (!enumName(s, mode, DRAW_MODES)) { glError(ctx, GL.INVALID_ENUM); return; }
  if (first < 0 || count < 0 || instances < 0) { glError(ctx, GL.INVALID_VALUE); return; }
  if (checkDraw(ctx, count ? first + count : 0, instances) && count && instances && !s.params.get('DRAW_FRAMEBUFFER_BINDING')) glQueueDraw(ctx, mode, first, count, instances, null);
};
// O maior índice lido do buffer de elementos decide quantos vértices cada atributo precisa cobrir.
const drawElements = (ctx, mode, count, type, offset, instances) => {
  const s = glState(ctx);
  if (s.lost) return;
  if (!enumName(s, mode, DRAW_MODES)) { glError(ctx, GL.INVALID_ENUM); return; }
  if (!enumName(s, type, s.version === 2 || s.extensions.has('OES_element_index_uint') ? ['UNSIGNED_BYTE', 'UNSIGNED_SHORT', 'UNSIGNED_INT'] : ['UNSIGNED_BYTE', 'UNSIGNED_SHORT'])) { glError(ctx, GL.INVALID_ENUM); return; }
  if (count < 0 || offset < 0 || instances < 0) { glError(ctx, GL.INVALID_VALUE); return; }
  const size = TYPE_BYTES[type];
  if (offset % size) { glError(ctx, GL.INVALID_OPERATION); return; }
  const element = s.vao.elementBuffer;
  if (!element) { glError(ctx, GL.INVALID_OPERATION); return; }
  const data = objOf(element).data;
  if (!data || offset + count * size > data.length) { glError(ctx, GL.INVALID_OPERATION); return; }
  const view = new DataView(data.buffer, data.byteOffset + offset, count * size);
  let max = -1;
  for (let i = 0; i < count; i++) { const v = size === 1 ? view.getUint8(i) : size === 2 ? view.getUint16(i * 2, true) : view.getUint32(i * 4, true); if (v > max) max = v; }
  if (checkDraw(ctx, max + 1, instances) && count && instances && !s.params.get('DRAW_FRAMEBUFFER_BINDING')) glQueueDraw(ctx, mode, 0, count, instances, { buffer: objOf(element), type, offset });
};
const TYPE_BYTES = { [GL.BYTE]: 1, [GL.UNSIGNED_BYTE]: 1, [GL.SHORT]: 2, [GL.UNSIGNED_SHORT]: 2, [GL.INT]: 4, [GL.UNSIGNED_INT]: 4, [GL.FLOAT]: 4, [GL.HALF_FLOAT]: 2, [GL.INT_2_10_10_10_REV]: 4, [GL.UNSIGNED_INT_2_10_10_10_REV]: 4 };
// Validação do Chromium antes do desenho: programa ligado e cada atributo ativo habilitado com buffer que cobre os
// vértices lidos (`vertices` é o maior índice mais um; os instanciados contam pelas instâncias).
const checkDraw = (ctx, vertices, instances = 1) => {
  const s = glState(ctx);
  const program = s.params.get('CURRENT_PROGRAM');
  if (!program || !objOf(program).linked) { glError(ctx, GL.INVALID_OPERATION); return false; }
  for (const active of objOf(program).attribs) {
    const a = s.vao.attribs[active.location];
    if (!a || !a.enabled) continue;
    if (!a.buffer) { glError(ctx, GL.INVALID_OPERATION); return false; }
    const n = a.divisor ? Math.ceil(instances / a.divisor) : vertices;
    if (n === 0) continue;
    const elem = a.size * TYPE_BYTES[a.type];
    const needed = a.offset + (n - 1) * (a.stride || elem) + elem;
    const data = objOf(a.buffer).data;
    if (!data || needed > data.length) { glError(ctx, GL.INVALID_OPERATION); return false; }
  }
  return true;
};
const framebufferStatus = (s, fb) => {
  if (!fb) return GL.FRAMEBUFFER_COMPLETE;
  const f = objOf(fb);
  const attachments = Array.from(f.attachments.values());
  if (!attachments.length) return GL.FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT;
  const sizes = attachments.map((a) => { const o = objOf(a.object); return o.size ? o.size.join('x') : a.level !== undefined && o.levels && o.levels.get(a.level) ? o.levels.get(a.level).join('x') : null; });
  if (sizes.some((x) => x === null || x === '0x0')) return GL.FRAMEBUFFER_INCOMPLETE_ATTACHMENT;
  if (new Set(sizes).size > 1) return s.version === 2 ? GL.FRAMEBUFFER_INCOMPLETE_ATTACHMENT : GL.FRAMEBUFFER_INCOMPLETE_DIMENSIONS;
  return GL.FRAMEBUFFER_COMPLETE;
};
const framebufferTarget = (ctx, target) => {
  const s = glState(ctx);
  const names = s.version === 2 ? ['FRAMEBUFFER', 'DRAW_FRAMEBUFFER', 'READ_FRAMEBUFFER'] : ['FRAMEBUFFER'];
  const name = enumName(s, target, names);
  if (!name) { glError(ctx, GL.INVALID_ENUM); return undefined; }
  return name === 'READ_FRAMEBUFFER' ? 'READ_FRAMEBUFFER_BINDING' : 'DRAW_FRAMEBUFFER_BINDING';
};

// Os métodos comuns às duas versões. `this` é o contexto; cada um confere o estado perdido antes de tudo.
const glMethods = {
  getContextAttributes() { const s = glState(this); return s.lost ? null : { ...s.attrs }; },
  isContextLost() { return glState(this).lost; },
  getSupportedExtensions() { const s = glState(this); return s.lost ? null : s.table.ext.slice(); },
  getExtension(name) {
    const s = glState(this);
    if (s.lost) return null;
    const found = s.table.ext.find((e) => e.toLowerCase() === String(name).toLowerCase());
    if (!found) return null;
    if (!s.extensions.has(found)) s.extensions.set(found, extensionObject(this, found));
    return s.extensions.get(found);
  },
  getError() {
    const s = glState(this);
    return s.errors.length ? s.errors.shift() : GL.NO_ERROR;
  },
  getParameter(pname) {
    const s = glState(this);
    if (s.lost) return null;
    const name = s.paramByValue.get(pname) || s.extConsts.get(pname);
    if (name && bindingParam[name] && s.params.has(name)) return bindingParam[name](s);
    if (name && s.params.has(name)) return copyValue(s.params.get(name));
    glError(this, GL.INVALID_ENUM);
    return null;
  },
  getShaderPrecisionFormat(shaderType, precisionType) {
    const s = glState(this);
    if (s.lost) return null;
    const sh = enumName(s, shaderType, ['VERTEX_SHADER', 'FRAGMENT_SHADER']);
    const pr = enumName(s, precisionType, ['LOW_FLOAT', 'MEDIUM_FLOAT', 'HIGH_FLOAT', 'LOW_INT', 'MEDIUM_INT', 'HIGH_INT']);
    if (!sh || !pr) { glError(this, GL.INVALID_ENUM); return null; }
    return withInfo(WebGLShaderPrecisionFormat, s.table.params[`prec ${sh} ${pr}`]);
  },
  enable(cap) { setCap(this, cap, true); },
  disable(cap) { setCap(this, cap, false); },
  isEnabled(cap) {
    const s = glState(this);
    if (s.lost) return false;
    const name = enumName(s, cap, s.version === 2 ? CAPABILITIES2 : CAPABILITIES);
    if (!name) { glError(this, GL.INVALID_ENUM); return false; }
    return s.params.get(name);
  },
  activeTexture(unit) {
    const s = glState(this);
    if (s.lost) return;
    if (unit < GL.TEXTURE0 || unit >= GL.TEXTURE0 + s.units.length) { glError(this, GL.INVALID_ENUM); return; }
    s.params.set('ACTIVE_TEXTURE', unit);
  },
  blendColor(r, g, b, a) { setParam(this, 'BLEND_COLOR', new Float32Array([r, g, b, a])); },
  blendEquation(mode) { this.blendEquationSeparate(mode, mode); },
  blendEquationSeparate(rgb, alpha) {
    const s = glState(this);
    if (s.lost) return;
    const modes = ['FUNC_ADD', 'FUNC_SUBTRACT', 'FUNC_REVERSE_SUBTRACT', ...(s.version === 2 || s.extensions.has('EXT_blend_minmax') ? ['MIN', 'MAX'] : [])];
    if (!enumName(s, rgb, modes) || !enumName(s, alpha, modes)) { glError(this, GL.INVALID_ENUM); return; }
    s.params.set('BLEND_EQUATION', rgb); s.params.set('BLEND_EQUATION_RGB', rgb); s.params.set('BLEND_EQUATION_ALPHA', alpha);
  },
  blendFunc(src, dst) { this.blendFuncSeparate(src, dst, src, dst); },
  blendFuncSeparate(srcRgb, dstRgb, srcAlpha, dstAlpha) {
    const s = glState(this);
    if (s.lost) return;
    const factors = ['ZERO', 'ONE', 'SRC_COLOR', 'ONE_MINUS_SRC_COLOR', 'DST_COLOR', 'ONE_MINUS_DST_COLOR', 'SRC_ALPHA', 'ONE_MINUS_SRC_ALPHA', 'DST_ALPHA', 'ONE_MINUS_DST_ALPHA', 'CONSTANT_COLOR', 'ONE_MINUS_CONSTANT_COLOR', 'CONSTANT_ALPHA', 'ONE_MINUS_CONSTANT_ALPHA', 'SRC_ALPHA_SATURATE'];
    if (![srcRgb, dstRgb, srcAlpha, dstAlpha].every((f) => enumName(s, f, factors))) { glError(this, GL.INVALID_ENUM); return; }
    const constant = (f) => f === GL.CONSTANT_COLOR || f === GL.ONE_MINUS_CONSTANT_COLOR;
    const alphaConst = (f) => f === GL.CONSTANT_ALPHA || f === GL.ONE_MINUS_CONSTANT_ALPHA;
    if ((constant(srcRgb) && alphaConst(dstRgb)) || (alphaConst(srcRgb) && constant(dstRgb))) { glError(this, GL.INVALID_OPERATION); return; }
    s.params.set('BLEND_SRC_RGB', srcRgb); s.params.set('BLEND_DST_RGB', dstRgb); s.params.set('BLEND_SRC_ALPHA', srcAlpha); s.params.set('BLEND_DST_ALPHA', dstAlpha);
  },
  clearColor(r, g, b, a) { setParam(this, 'COLOR_CLEAR_VALUE', new Float32Array([r, g, b, a])); },
  clearDepth(d) { setParam(this, 'DEPTH_CLEAR_VALUE', Math.min(1, Math.max(0, Number(d)))); },
  clearStencil(v) { setParam(this, 'STENCIL_CLEAR_VALUE', v | 0); },
  colorMask(r, g, b, a) { setParam(this, 'COLOR_WRITEMASK', [!!r, !!g, !!b, !!a]); },
  cullFace(mode) { setEnum(this, 'CULL_FACE_MODE', mode, ['FRONT', 'BACK', 'FRONT_AND_BACK']); },
  depthFunc(f) { setEnum(this, 'DEPTH_FUNC', f, ['NEVER', 'LESS', 'EQUAL', 'LEQUAL', 'GREATER', 'NOTEQUAL', 'GEQUAL', 'ALWAYS']); },
  depthMask(flag) { setParam(this, 'DEPTH_WRITEMASK', !!flag); },
  depthRange(near, far) {
    if (near > far) { glError(this, GL.INVALID_OPERATION); return; }
    setParam(this, 'DEPTH_RANGE', new Float32Array([Math.min(1, Math.max(0, near)), Math.min(1, Math.max(0, far))]));
  },
  frontFace(mode) { setEnum(this, 'FRONT_FACE', mode, ['CW', 'CCW']); },
  hint(target, mode) {
    const s = glState(this);
    if (s.lost) return;
    const targets = ['GENERATE_MIPMAP_HINT', ...(s.version === 2 ? ['FRAGMENT_SHADER_DERIVATIVE_HINT'] : []), ...(s.extensions.has('OES_standard_derivatives') ? ['FRAGMENT_SHADER_DERIVATIVE_HINT_OES'] : [])];
    const name = targets.find((t) => (GL[t] ?? 35723) === target);
    if (!name || !enumName(s, mode, ['FASTEST', 'NICEST', 'DONT_CARE'])) { glError(this, GL.INVALID_ENUM); return; }
    s.params.set(name, mode);
  },
  lineWidth(w) { if (!(w > 0)) { glError(this, GL.INVALID_VALUE); return; } setParam(this, 'LINE_WIDTH', Math.fround(w)); },
  pixelStorei(pname, param) {
    const s = glState(this);
    if (s.lost) return;
    const names = ['PACK_ALIGNMENT', 'UNPACK_ALIGNMENT', 'UNPACK_FLIP_Y_WEBGL', 'UNPACK_PREMULTIPLY_ALPHA_WEBGL', 'UNPACK_COLORSPACE_CONVERSION_WEBGL', ...(s.version === 2 ? ['PACK_ROW_LENGTH', 'PACK_SKIP_PIXELS', 'PACK_SKIP_ROWS', 'UNPACK_ROW_LENGTH', 'UNPACK_IMAGE_HEIGHT', 'UNPACK_SKIP_PIXELS', 'UNPACK_SKIP_ROWS', 'UNPACK_SKIP_IMAGES'] : [])];
    const name = enumName(s, pname, names);
    if (!name) { glError(this, GL.INVALID_ENUM); return; }
    if (/ALIGNMENT/.test(name) && ![1, 2, 4, 8].includes(param)) { glError(this, GL.INVALID_VALUE); return; }
    if (name === 'UNPACK_COLORSPACE_CONVERSION_WEBGL' && param !== GL.NONE && param !== GL.BROWSER_DEFAULT_WEBGL) { glError(this, GL.INVALID_ENUM); return; }
    if (/WEBGL$/.test(name) && name !== 'UNPACK_COLORSPACE_CONVERSION_WEBGL') s.params.set(name, !!param);
    else { if (param < 0) { glError(this, GL.INVALID_VALUE); return; } s.params.set(name, param | 0); }
  },
  polygonOffset(factor, units) { const s = glState(this); if (s.lost) return; s.params.set('POLYGON_OFFSET_FACTOR', Math.fround(factor)); s.params.set('POLYGON_OFFSET_UNITS', Math.fround(units)); },
  sampleCoverage(value, invert) { const s = glState(this); if (s.lost) return; s.params.set('SAMPLE_COVERAGE_VALUE', Math.fround(Math.min(1, Math.max(0, value)))); s.params.set('SAMPLE_COVERAGE_INVERT', !!invert); },
  scissor(x, y, w, h) { if (w < 0 || h < 0) { glError(this, GL.INVALID_VALUE); return; } setParam(this, 'SCISSOR_BOX', new Int32Array([x, y, w, h])); },
  viewport(x, y, w, h) {
    if (w < 0 || h < 0) { glError(this, GL.INVALID_VALUE); return; }
    const max = glState(this).params.get('MAX_VIEWPORT_DIMS');
    setParam(this, 'VIEWPORT', new Int32Array([x, y, Math.min(w, max[0]), Math.min(h, max[1])]));
  },
  stencilFunc(func, ref, mask) { this.stencilFuncSeparate(GL.FRONT_AND_BACK, func, ref, mask); },
  stencilFuncSeparate(face, func, ref, mask) {
    const s = glState(this);
    if (s.lost) return;
    if (!enumName(s, face, ['FRONT', 'BACK', 'FRONT_AND_BACK']) || !enumName(s, func, ['NEVER', 'LESS', 'EQUAL', 'LEQUAL', 'GREATER', 'NOTEQUAL', 'GEQUAL', 'ALWAYS'])) { glError(this, GL.INVALID_ENUM); return; }
    for (const side of faces(face)) { s.params.set(`STENCIL${side}FUNC`, func); s.params.set(`STENCIL${side}REF`, ref | 0); s.params.set(`STENCIL${side}VALUE_MASK`, mask >>> 0); }
  },
  stencilMask(mask) { this.stencilMaskSeparate(GL.FRONT_AND_BACK, mask); },
  stencilMaskSeparate(face, mask) {
    const s = glState(this);
    if (s.lost) return;
    if (!enumName(s, face, ['FRONT', 'BACK', 'FRONT_AND_BACK'])) { glError(this, GL.INVALID_ENUM); return; }
    for (const side of faces(face)) s.params.set(`STENCIL${side}WRITEMASK`, mask >>> 0);
  },
  stencilOp(fail, zfail, zpass) { this.stencilOpSeparate(GL.FRONT_AND_BACK, fail, zfail, zpass); },
  stencilOpSeparate(face, fail, zfail, zpass) {
    const s = glState(this);
    if (s.lost) return;
    const ops = ['KEEP', 'ZERO', 'REPLACE', 'INCR', 'INCR_WRAP', 'DECR', 'DECR_WRAP', 'INVERT'];
    if (!enumName(s, face, ['FRONT', 'BACK', 'FRONT_AND_BACK']) || ![fail, zfail, zpass].every((o) => enumName(s, o, ops))) { glError(this, GL.INVALID_ENUM); return; }
    for (const side of faces(face)) { s.params.set(`STENCIL${side}FAIL`, fail); s.params.set(`STENCIL${side}PASS_DEPTH_FAIL`, zfail); s.params.set(`STENCIL${side}PASS_DEPTH_PASS`, zpass); }
  },

  createBuffer() { return glState(this).lost ? null : mkObject(this, WebGLBuffer, { data: null, usage: GL.STATIC_DRAW, used: false }); },
  deleteBuffer(b) { deleteObject(this, b, WebGLBuffer, 'deleteBuffer', unbindEverywhere); },
  isBuffer(b) { return !glState(this).lost && isObject(this, b, WebGLBuffer); },
  bindBuffer(target, buffer) {
    const s = glState(this);
    if (s.lost) return;
    const name = bufferTarget(this, target);
    if (!name || !validObject(this, buffer, WebGLBuffer, 'bindBuffer')) return;
    if (buffer) {
      const b = objOf(buffer);
      const elementKind = name === 'ELEMENT_ARRAY_BUFFER';
      if (b.kind !== undefined && b.kind !== elementKind) { glError(this, GL.INVALID_OPERATION); return; }
      b.kind = elementKind;
      b.used = true;
    }
    if (name === 'ELEMENT_ARRAY_BUFFER') s.vao.elementBuffer = buffer; else s.params.set(BUFFER_TARGETS[name], buffer);
    if (name === 'UNIFORM_BUFFER' || name === 'TRANSFORM_FEEDBACK_BUFFER') s.params.set(name + '_BINDING', buffer);
  },
  bufferData(target, data, usage, srcOffset = 0, length = 0) {
    const s = glState(this);
    if (s.lost) return;
    const name = bufferTarget(this, target);
    if (!name) return;
    if (!enumName(s, usage, s.version === 2 ? ['STREAM_DRAW', 'STATIC_DRAW', 'DYNAMIC_DRAW', 'STREAM_READ', 'STATIC_READ', 'DYNAMIC_READ', 'STREAM_COPY', 'STATIC_COPY', 'DYNAMIC_COPY'] : ['STREAM_DRAW', 'STATIC_DRAW', 'DYNAMIC_DRAW'])) { glError(this, GL.INVALID_ENUM); return; }
    const buffer = boundBuffer(s, name);
    if (!buffer) { glError(this, GL.INVALID_OPERATION); return; }
    let bytes;
    if (typeof data === 'number') { if (data < 0) { glError(this, GL.INVALID_VALUE); return; } bytes = new Uint8Array(data); }
    else if (data === null) { glError(this, GL.INVALID_VALUE); return; }
    else bytes = viewBytes(data, srcOffset, length);
    const b = objOf(buffer);
    b.data = bytes; b.usage = usage;
  },
  bufferSubData(target, offset, data, srcOffset = 0, length = 0) {
    const s = glState(this);
    if (s.lost) return;
    const name = bufferTarget(this, target);
    if (!name) return;
    const buffer = boundBuffer(s, name);
    if (!buffer) { glError(this, GL.INVALID_OPERATION); return; }
    const b = objOf(buffer);
    const bytes = viewBytes(data, srcOffset, length);
    if (offset < 0 || !b.data || offset + bytes.length > b.data.length) { glError(this, GL.INVALID_VALUE); return; }
    glBeforeWrite(s, b);
    b.data.set(bytes, offset);
  },
  getBufferParameter(target, pname) {
    const s = glState(this);
    if (s.lost) return null;
    const name = bufferTarget(this, target);
    if (!name) return null;
    const buffer = boundBuffer(s, name);
    if (!buffer) { glError(this, GL.INVALID_OPERATION); return null; }
    const b = objOf(buffer);
    if (pname === GL.BUFFER_SIZE) return b.data ? b.data.length : 0;
    if (pname === GL.BUFFER_USAGE) return b.usage;
    glError(this, GL.INVALID_ENUM);
    return null;
  },

  createShader(type) {
    const s = glState(this);
    if (s.lost) return null;
    if (type !== GL.VERTEX_SHADER && type !== GL.FRAGMENT_SHADER) { glError(this, GL.INVALID_ENUM); return null; }
    return mkObject(this, WebGLShader, { type, source: '', compiled: false, log: '', parsed: null, attached: 0 });
  },
  deleteShader(shader) { deleteObject(this, shader, WebGLShader, 'deleteShader'); },
  isShader(shader) { return !glState(this).lost && isObject(this, shader, WebGLShader); },
  shaderSource(shader, source) { const sh = !glState(this).lost && shaderOf(this, shader, 'shaderSource'); if (sh) sh.source = String(source); },
  getShaderSource(shader) { const sh = !glState(this).lost && shaderOf(this, shader, 'getShaderSource'); return sh ? sh.source : null; },
  compileShader(shader) {
    const sh = !glState(this).lost && shaderOf(this, shader, 'compileShader');
    if (!sh) return;
    const version = glslVersion(sh.source);
    const s = glState(this);
    if (version === 300 && s.version !== 2) { sh.compiled = false; sh.log = "ERROR: 0:1: '300' : version number not supported\n"; return; }
    const problem = glslProblem(sh.source, sh.type);
    sh.compiled = !problem;
    sh.log = problem || '';
    sh.parsed = sh.compiled ? parseShader(sh.source, sh.type) : null;
  },
  getShaderParameter(shader, pname) {
    const sh = !glState(this).lost && shaderOf(this, shader, 'getShaderParameter');
    if (!sh) return null;
    if (pname === GL.SHADER_TYPE) return sh.type;
    if (pname === GL.DELETE_STATUS) return sh.deleted;
    if (pname === GL.COMPILE_STATUS) return sh.compiled;
    glError(this, GL.INVALID_ENUM);
    return null;
  },
  getShaderInfoLog(shader) { const sh = !glState(this).lost && shaderOf(this, shader, 'getShaderInfoLog'); return sh ? sh.log : null; },

  createProgram() { return glState(this).lost ? null : mkObject(this, WebGLProgram, { shaders: [], bound: new Map(), linked: false, log: '', attribs: [], uniforms: [], blocks: [], uniformValues: new Map(), generation: 0, validated: false }); },
  deleteProgram(program) {
    deleteObject(this, program, WebGLProgram, 'deleteProgram');
  },
  isProgram(program) { return !glState(this).lost && isObject(this, program, WebGLProgram); },
  attachShader(program, shader) {
    if (glState(this).lost) return;
    const p = programOf(this, program, 'attachShader');
    const sh = shaderOf(this, shader, 'attachShader', 2);
    if (!p || !sh) return;
    if (p.shaders.some((x) => objOf(x).type === sh.type)) { glError(this, GL.INVALID_OPERATION); return; }
    p.shaders.push(shader);
  },
  detachShader(program, shader) {
    if (glState(this).lost) return;
    const p = programOf(this, program, 'detachShader');
    const sh = shaderOf(this, shader, 'detachShader', 2);
    if (!p || !sh) return;
    const i = p.shaders.indexOf(shader);
    if (i < 0) { glError(this, GL.INVALID_OPERATION); return; }
    p.shaders.splice(i, 1);
  },
  getAttachedShaders(program) { const p = !glState(this).lost && programOf(this, program, 'getAttachedShaders'); return p ? p.shaders.slice() : null; },
  bindAttribLocation(program, index, name) {
    const p = !glState(this).lost && programOf(this, program, 'bindAttribLocation');
    if (!p) return;
    if (index >= glState(this).table.params.MAX_VERTEX_ATTRIBS) { glError(this, GL.INVALID_VALUE); return; }
    if (/^(gl_|webgl_|_webgl_)/.test(name)) { glError(this, GL.INVALID_OPERATION); return; }
    p.bound.set(String(name), index);
  },
  linkProgram(program) { const p = !glState(this).lost && programOf(this, program, 'linkProgram'); if (p) linkProgram(this, program); },
  validateProgram(program) { const p = !glState(this).lost && programOf(this, program, 'validateProgram'); if (p) p.validated = p.linked; },
  useProgram(program) {
    const s = glState(this);
    if (s.lost || !validObject(this, program, WebGLProgram, 'useProgram')) return;
    if (program && !objOf(program).linked) { glError(this, GL.INVALID_OPERATION); return; }
    s.params.set('CURRENT_PROGRAM', program);
  },
  getProgramParameter(program, pname) {
    const s = glState(this);
    const p = !s.lost && programOf(this, program, 'getProgramParameter');
    if (!p) return null;
    switch (pname) {
      case GL.DELETE_STATUS: return p.deleted;
      case GL.LINK_STATUS: return p.linked;
      case GL.VALIDATE_STATUS: return p.validated;
      case GL.ATTACHED_SHADERS: return p.shaders.length;
      case GL.ACTIVE_ATTRIBUTES: return p.attribs.length;
      case GL.ACTIVE_UNIFORMS: return p.uniforms.length;
    }
    if (s.version === 2) {
      if (pname === GL.ACTIVE_UNIFORM_BLOCKS) return p.blocks.length;
      if (pname === GL.TRANSFORM_FEEDBACK_BUFFER_MODE) return p.feedbackMode ?? GL.INTERLEAVED_ATTRIBS;
      if (pname === GL.TRANSFORM_FEEDBACK_VARYINGS) return p.linked && p.feedbackVaryings ? p.feedbackVaryings.length : 0;
    }
    glError(this, GL.INVALID_ENUM);
    return null;
  },
  getProgramInfoLog(program) { const p = !glState(this).lost && programOf(this, program, 'getProgramInfoLog'); return p ? p.log : null; },
  getActiveAttrib(program, index) {
    const p = !glState(this).lost && programOf(this, program, 'getActiveAttrib');
    if (!p) return null;
    const a = p.attribs[index];
    if (!a) { glError(this, GL.INVALID_VALUE); return null; }
    return withInfo(WebGLActiveInfo, { size: a.size, type: a.type, name: a.name });
  },
  getActiveUniform(program, index) {
    const p = !glState(this).lost && programOf(this, program, 'getActiveUniform');
    if (!p) return null;
    const u = p.uniforms[index];
    if (!u) { glError(this, GL.INVALID_VALUE); return null; }
    return withInfo(WebGLActiveInfo, { size: u.size, type: u.type, name: u.name });
  },
  getAttribLocation(program, name) {
    const p = !glState(this).lost && programOf(this, program, 'getAttribLocation');
    if (!p) return -1;
    if (!p.linked) { glError(this, GL.INVALID_OPERATION); return -1; }
    const a = p.attribs.find((x) => x.name === String(name));
    return a ? a.location : -1;
  },
  getUniformLocation(program, name) {
    const p = !glState(this).lost && programOf(this, program, 'getUniformLocation');
    if (!p) return null;
    if (!p.linked) { glError(this, GL.INVALID_OPERATION); return null; }
    const m = /^(.*?)(?:\[(\d+)\])?$/.exec(String(name));
    const index = p.uniforms.findIndex((u) => u.block === undefined && (u.name === m[0] || u.base === m[0] || (u.base === m[1] && (m[2] === undefined || Number(m[2]) < u.size))));
    if (index < 0) return null;
    const u = p.uniforms[index];
    const element = u.base === m[1] && m[2] !== undefined ? Number(m[2]) : 0;
    return withInfo(WebGLUniformLocation, { program: p, generation: p.generation, index, element });
  },
  getUniform(program, location) {
    const p = !glState(this).lost && programOf(this, program, 'getUniform');
    if (!p) return null;
    const loc = location[P.__info];
    if (loc.program !== p || loc.generation !== p.generation) { glError(this, GL.INVALID_OPERATION); return null; }
    const u = p.uniforms[loc.index];
    const n = typeComponents(u.typeName);
    const value = p.uniformValues.get(loc.index + ':' + loc.element) || new Array(n).fill(0);
    if (/^BOOL/.test(u.typeName)) return n === 1 ? !!value[0] : value.map(Boolean);
    if (/SAMPLER/.test(u.typeName) || u.typeName === 'INT') return value[0] | 0;
    if (u.typeName === 'UNSIGNED_INT') return value[0] >>> 0;
    if (u.typeName === 'FLOAT') return Math.fround(value[0]);
    if (/^INT_VEC/.test(u.typeName)) return new Int32Array(value);
    if (/^UNSIGNED_INT_VEC/.test(u.typeName)) return new Uint32Array(value);
    return new Float32Array(value);
  },

  enableVertexAttribArray(index) { setAttrib(this, index, (a) => { a.enabled = true; }); },
  disableVertexAttribArray(index) { setAttrib(this, index, (a) => { a.enabled = false; }); },
  vertexAttribPointer(index, size, type, normalized, stride, offset) {
    const s = glState(this);
    if (s.lost) return;
    if (index >= s.vao.attribs.length || size < 1 || size > 4 || stride < 0 || stride > 255 || offset < 0) { glError(this, GL.INVALID_VALUE); return; }
    const types = ['BYTE', 'UNSIGNED_BYTE', 'SHORT', 'UNSIGNED_SHORT', 'FLOAT', ...(s.version === 2 ? ['INT', 'UNSIGNED_INT', 'HALF_FLOAT', 'INT_2_10_10_10_REV', 'UNSIGNED_INT_2_10_10_10_REV'] : [])];
    if (!enumName(s, type, types)) { glError(this, GL.INVALID_ENUM); return; }
    const buffer = s.params.get('ARRAY_BUFFER_BINDING');
    if (!buffer && offset !== 0) { glError(this, GL.INVALID_OPERATION); return; }
    Object.assign(s.vao.attribs[index], { size, type, normalized: !!normalized, stride, offset, buffer, integer: false });
  },
  getVertexAttrib(index, pname) {
    const s = glState(this);
    if (s.lost) return null;
    if (index >= s.vao.attribs.length) { glError(this, GL.INVALID_VALUE); return null; }
    const a = s.vao.attribs[index];
    switch (pname) {
      case GL.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING: return a.buffer;
      case GL.VERTEX_ATTRIB_ARRAY_ENABLED: return a.enabled;
      case GL.VERTEX_ATTRIB_ARRAY_SIZE: return a.size;
      case GL.VERTEX_ATTRIB_ARRAY_STRIDE: return a.stride;
      case GL.VERTEX_ATTRIB_ARRAY_TYPE: return a.type;
      case GL.VERTEX_ATTRIB_ARRAY_NORMALIZED: return a.normalized;
      case GL.CURRENT_VERTEX_ATTRIB: return s.current[index].slice();
    }
    if (pname === 35070 && (s.version === 2 || s.extensions.has('ANGLE_instanced_arrays'))) return a.divisor;
    if (s.version === 2 && pname === GL.VERTEX_ATTRIB_ARRAY_INTEGER) return a.integer;
    glError(this, GL.INVALID_ENUM);
    return null;
  },
  getVertexAttribOffset(index, pname) {
    const s = glState(this);
    if (index >= s.vao.attribs.length) { glError(this, GL.INVALID_VALUE); return 0; }
    if (pname !== GL.VERTEX_ATTRIB_ARRAY_POINTER) { glError(this, GL.INVALID_ENUM); return 0; }
    return s.vao.attribs[index].offset;
  },

  createTexture() { return glState(this).lost ? null : mkObject(this, WebGLTexture, { target: null, params: newTextureParams(), levels: new Map(), used: false }); },
  deleteTexture(t) { deleteObject(this, t, WebGLTexture, 'deleteTexture', unbindEverywhere); },
  isTexture(t) { return !glState(this).lost && isObject(this, t, WebGLTexture); },
  bindTexture(target, texture) {
    const s = glState(this);
    if (s.lost) return;
    const name = textureTarget(this, target);
    if (!name || !validObject(this, texture, WebGLTexture, 'bindTexture')) return;
    if (texture) {
      const t = objOf(texture);
      if (t.target !== null && t.target !== name) { glError(this, GL.INVALID_OPERATION); return; }
      t.target = name; t.used = true;
    }
    s.units[s.params.get('ACTIVE_TEXTURE') - GL.TEXTURE0][name] = texture;
  },
  texParameteri(target, pname, param) { texParameter(this, target, pname, param | 0); },
  texParameterf(target, pname, param) { texParameter(this, target, pname, Number(param)); },
  getTexParameter(target, pname) {
    const s = glState(this);
    if (s.lost) return null;
    const name = textureTarget(this, target);
    if (!name) return null;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return null; }
    const t = objOf(texture);
    const key = paramName(t.params, pname);
    if (!key || (s.version !== 2 && !['TEXTURE_MIN_FILTER', 'TEXTURE_MAG_FILTER', 'TEXTURE_WRAP_S', 'TEXTURE_WRAP_T'].includes(key))) {
      if (pname === 34046 && s.extensions.has('EXT_texture_filter_anisotropic')) return t.anisotropy ?? 1;
      glError(this, GL.INVALID_ENUM);
      return null;
    }
    return t.params.get(key);
  },
  texImage2D(target, level, internalformat, ...rest) {
    const s = glState(this);
    if (s.lost) return;
    const name = imageTarget(this, target);
    if (!name) return;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    const explicit = rest.length >= 6;
    const [width, height, border, format, type, source, srcOffset] = explicit ? rest : [undefined, undefined, 0, rest[0], rest[1], rest[2]];
    texImage(this, s, objOf(texture), target, level, internalformat, explicit ? [width, height, 1] : null, border, format, type, source, srcOffset);
  },
  texSubImage2D(target, level, xoffset, yoffset, ...rest) {
    const s = glState(this);
    if (s.lost) return;
    const name = imageTarget(this, target);
    if (!name) return;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    const explicit = rest.length >= 5;
    const [width, height, format, type, source, srcOffset] = explicit ? rest : [undefined, undefined, rest[0], rest[1], rest[2]];
    texSubImage(this, s, objOf(texture), target, level, [xoffset, yoffset, 0], explicit ? [width, height, 1] : null, format, type, source, srcOffset);
  },
  compressedTexImage2D(target, level, internalformat, width, height) {
    const s = glState(this);
    const name = imageTarget(this, target);
    if (!name) return;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    objOf(texture).levels.set(level, [width, height]);
  },
  compressedTexSubImage2D() {},
  copyTexImage2D(target, level, internalformat, x, y, width, height, border) {
    const s = glState(this);
    if (s.lost) return;
    const name = imageTarget(this, target);
    if (!name) return;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    const t = objOf(texture);
    if (t.params.get('TEXTURE_IMMUTABLE_FORMAT')) { glError(this, GL.INVALID_OPERATION); return; }
    if (level < 0 || width < 0 || height < 0 || border !== 0) { glError(this, GL.INVALID_VALUE); return; }
    const fmtName = { [GL.RGBA]: 'RGBA8', [GL.RGB]: 'RGB8', [GL.LUMINANCE]: 'LUMINANCE', [GL.ALPHA]: 'ALPHA', [GL.LUMINANCE_ALPHA]: 'LUMINANCE_ALPHA' }[internalformat] || Object.keys(TEX_SIZED).find((k) => GL[k] === internalformat && s.version === 2);
    if (!fmtName) { glError(this, GL.INVALID_ENUM); return; }
    const fmt = TEX_FORMATS[fmtName];
    const face = texFace(target);
    const img = texNewImage(fmt, width, height, 1);
    texSetImage(t, face, level, img);
    t.levels.set(level, [width, height]);
    texCopyFromFramebuffer(s, img, 0, 0, 0, x, y, width, height);
  },
  copyTexSubImage2D(target, level, xoffset, yoffset, x, y, width, height) {
    const s = glState(this);
    if (s.lost) return;
    const name = imageTarget(this, target);
    if (!name) return;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    const t = objOf(texture), face = texFace(target);
    const img = t.images && t.images.get(texImageKey(face, level));
    if (!img) { glError(this, GL.INVALID_OPERATION); return; }
    if (xoffset < 0 || yoffset < 0 || width < 0 || height < 0 || xoffset + width > img.w || yoffset + height > img.h) { glError(this, GL.INVALID_VALUE); return; }
    texCopyFromFramebuffer(s, texWritable(s, t, face, level), xoffset, yoffset, 0, x, y, width, height);
  },
  generateMipmap(target) {
    const s = glState(this);
    if (s.lost) return;
    const name = textureTarget(this, target);
    if (!name) return;
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    const t = objOf(texture);
    const base = s.version === 2 ? t.params.get('TEXTURE_BASE_LEVEL') : 0;
    const faces = name === 'TEXTURE_CUBE_MAP' ? 6 : 1;
    const img0 = t.images && t.images.get(texImageKey(0, base));
    if (!img0) { glError(this, GL.INVALID_OPERATION); return; }
    if (s.version !== 2 && (!texIsPot(img0.w) || !texIsPot(img0.h))) { glError(this, GL.INVALID_OPERATION); return; }
    if (img0.fmt.kind === 'uint' || img0.fmt.kind === 'int' || img0.fmt.depth || !texFilterable(s, img0.fmt)) { glError(this, GL.INVALID_OPERATION); return; }
    for (let f = 0; f < faces; f++) { const o = t.images.get(texImageKey(f, base)); if (!o || o.w !== img0.w || o.h !== img0.h || o.fmt !== img0.fmt) { glError(this, GL.INVALID_OPERATION); return; } }
    const third = name === 'TEXTURE_3D';
    const maxLevel = s.version === 2 ? t.params.get('TEXTURE_MAX_LEVEL') : 1000;
    for (let f = 0; f < faces; f++) {
      let src = t.images.get(texImageKey(f, base));
      for (let level = base + 1; level <= maxLevel && (src.w > 1 || src.h > 1 || (third && src.d > 1)); level++) {
        const next = texBlitLevel(src, Math.max(1, src.w >> 1), Math.max(1, src.h >> 1), third ? Math.max(1, src.d >> 1) : src.d);
        texSetImage(t, f, level, next);
        t.levels.set(level, [next.w, next.h]);
        src = next;
      }
    }
  },

  createFramebuffer() { return glState(this).lost ? null : mkObject(this, WebGLFramebuffer, { attachments: new Map(), used: false }); },
  deleteFramebuffer(f) { deleteObject(this, f, WebGLFramebuffer, 'deleteFramebuffer', unbindEverywhere); },
  isFramebuffer(f) { return !glState(this).lost && isObject(this, f, WebGLFramebuffer); },
  bindFramebuffer(target, framebuffer) {
    const s = glState(this);
    if (s.lost) return;
    const slot = framebufferTarget(this, target);
    if (!slot || !validObject(this, framebuffer, WebGLFramebuffer, 'bindFramebuffer')) return;
    if (framebuffer) objOf(framebuffer).used = true;
    if (target === GL.FRAMEBUFFER) { s.params.set('DRAW_FRAMEBUFFER_BINDING', framebuffer); s.params.set('READ_FRAMEBUFFER_BINDING', framebuffer); }
    else s.params.set(slot, framebuffer);
    s.params.set('FRAMEBUFFER_BINDING', s.params.get('DRAW_FRAMEBUFFER_BINDING'));
  },
  checkFramebufferStatus(target) {
    const s = glState(this);
    if (s.lost) return 0;
    const slot = framebufferTarget(this, target);
    if (!slot) return 0;
    return framebufferStatus(s, s.params.get(slot));
  },
  framebufferTexture2D(target, attachment, textarget, texture, level) {
    const s = glState(this);
    if (s.lost) return;
    const slot = framebufferTarget(this, target);
    if (!slot || !validObject(this, texture, WebGLTexture, 'framebufferTexture2D')) return;
    const fb = s.params.get(slot);
    if (!fb) { glError(this, GL.INVALID_OPERATION); return; }
    if (texture) objOf(fb).attachments.set(attachment, { object: texture, level, textarget }); else objOf(fb).attachments.delete(attachment);
  },
  framebufferRenderbuffer(target, attachment, renderbuffertarget, renderbuffer) {
    const s = glState(this);
    if (s.lost) return;
    const slot = framebufferTarget(this, target);
    if (!slot || !validObject(this, renderbuffer, WebGLRenderbuffer, 'framebufferRenderbuffer')) return;
    const fb = s.params.get(slot);
    if (!fb) { glError(this, GL.INVALID_OPERATION); return; }
    if (renderbuffer) objOf(fb).attachments.set(attachment, { object: renderbuffer }); else objOf(fb).attachments.delete(attachment);
  },
  getFramebufferAttachmentParameter(target, attachment, pname) {
    const s = glState(this);
    if (s.lost) return null;
    const slot = framebufferTarget(this, target);
    if (!slot) return null;
    const fb = s.params.get(slot);
    if (!fb) {
      if (s.version !== 2) { glError(this, GL.INVALID_OPERATION); return null; }
      if (pname === GL.FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE) return GL.FRAMEBUFFER_DEFAULT;
      glError(this, GL.INVALID_ENUM);
      return null;
    }
    const a = objOf(fb).attachments.get(attachment);
    if (pname === GL.FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE) return a ? (a.object instanceof WebGLTexture ? GL.TEXTURE : GL.RENDERBUFFER) : GL.NONE;
    if (!a) { glError(this, GL.INVALID_OPERATION); return null; }
    if (pname === GL.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME) return a.object;
    if (pname === GL.FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL && a.level !== undefined) return a.level;
    if (pname === GL.FRAMEBUFFER_ATTACHMENT_TEXTURE_CUBE_MAP_FACE && a.level !== undefined) return a.textarget === GL.TEXTURE_2D ? 0 : a.textarget;
    glError(this, GL.INVALID_ENUM);
    return null;
  },

  createRenderbuffer() { return glState(this).lost ? null : mkObject(this, WebGLRenderbuffer, { size: null, format: GL.RGBA4, samples: 0, used: false }); },
  deleteRenderbuffer(r) { deleteObject(this, r, WebGLRenderbuffer, 'deleteRenderbuffer', unbindEverywhere); },
  isRenderbuffer(r) { return !glState(this).lost && isObject(this, r, WebGLRenderbuffer); },
  bindRenderbuffer(target, renderbuffer) {
    const s = glState(this);
    if (s.lost) return;
    if (target !== GL.RENDERBUFFER) { glError(this, GL.INVALID_ENUM); return; }
    if (!validObject(this, renderbuffer, WebGLRenderbuffer, 'bindRenderbuffer')) return;
    if (renderbuffer) objOf(renderbuffer).used = true;
    s.params.set('RENDERBUFFER_BINDING', renderbuffer);
  },
  renderbufferStorage(target, internalformat, width, height) { renderbufferStorage(this, target, 0, internalformat, width, height); },
  getRenderbufferParameter(target, pname) {
    const s = glState(this);
    if (s.lost) return null;
    if (target !== GL.RENDERBUFFER) { glError(this, GL.INVALID_ENUM); return null; }
    const rb = s.params.get('RENDERBUFFER_BINDING');
    if (!rb) { glError(this, GL.INVALID_OPERATION); return null; }
    const r = objOf(rb);
    switch (pname) {
      case GL.RENDERBUFFER_WIDTH: return r.size ? r.size[0] : 0;
      case GL.RENDERBUFFER_HEIGHT: return r.size ? r.size[1] : 0;
      case GL.RENDERBUFFER_INTERNAL_FORMAT: return r.format;
      case GL.RENDERBUFFER_RED_SIZE: case GL.RENDERBUFFER_GREEN_SIZE: case GL.RENDERBUFFER_BLUE_SIZE: case GL.RENDERBUFFER_ALPHA_SIZE: return r.size && /RGBA|RGB/.test(r.formatName || '') ? 8 : 0;
      case GL.RENDERBUFFER_DEPTH_SIZE: return r.size && /DEPTH/.test(r.formatName || '') ? 24 : 0;
      case GL.RENDERBUFFER_STENCIL_SIZE: return r.size && /STENCIL/.test(r.formatName || '') ? 8 : 0;
    }
    if (s.version === 2 && pname === GL.RENDERBUFFER_SAMPLES) return r.samples;
    glError(this, GL.INVALID_ENUM);
    return null;
  },

  clear(mask) {
    const s = glState(this);
    if (s.lost) return;
    if (mask & ~(GL.COLOR_BUFFER_BIT | GL.DEPTH_BUFFER_BIT | GL.STENCIL_BUFFER_BIT)) { glError(this, GL.INVALID_VALUE); return; }
    const fb = s.params.get('DRAW_FRAMEBUFFER_BINDING');
    if (fb && framebufferStatus(s, fb) !== GL.FRAMEBUFFER_COMPLETE) { glError(this, GL.INVALID_FRAMEBUFFER_OPERATION); return; }
    if (fb || !mask) return;
    glQueueClear(s, mask);
  },
  drawArrays(mode, first, count) { drawArrays(this, mode, first, count, 1); },
  drawElements(mode, count, type, offset) { drawElements(this, mode, count, type, offset, 1); },
  finish() {},
  flush() {},
  readPixels(x, y, width, height, format, type, dst, dstOffset = 0) {
    const s = glState(this);
    if (s.lost) return;
    if (width < 0 || height < 0) { glError(this, GL.INVALID_VALUE); return; }
    if (format !== GL.RGBA || type !== GL.UNSIGNED_BYTE) {
      if (!(format === s.params.get('IMPLEMENTATION_COLOR_READ_FORMAT') && type === s.params.get('IMPLEMENTATION_COLOR_READ_TYPE'))) { glError(this, GL.INVALID_OPERATION); return; }
    }
    if (!ArrayBuffer.isView(dst)) { if (s.version === 2 && typeof dst === 'number' && s.params.get('PIXEL_PACK_BUFFER_BINDING')) return; glError(this, GL.INVALID_VALUE); return; }
    if (!(dst instanceof Uint8Array || dst instanceof Uint8ClampedArray)) { glError(this, GL.INVALID_OPERATION); return; }
    const out = new Uint8Array(dst.buffer, dst.byteOffset + dstOffset, dst.byteLength - dstOffset);
    if (out.length < width * height * 4) { glError(this, GL.INVALID_OPERATION); return; }
    const fb = s.params.get('READ_FRAMEBUFFER_BINDING');
    if (fb) { if (framebufferStatus(s, fb) !== GL.FRAMEBUFFER_COMPLETE) glError(this, GL.INVALID_FRAMEBUFFER_OPERATION); return; }
    const pixels = glFlush(s);
    for (let row = 0; row < height; row++) for (let col = 0; col < width; col++) {
      const sx = x + col, sy = y + row, o = (row * width + col) * 4;
      if (sx < 0 || sy < 0 || sx >= s.width || sy >= s.height) continue;
      const i = (sy * s.width + sx) * 4;
      for (let c = 0; c < 4; c++) out[o + c] = pixels[i + c];
    }
  },
  vertexAttrib1f(i, x) { setCurrent(this, i, [x, 0, 0, 1]); },
  vertexAttrib2f(i, x, y) { setCurrent(this, i, [x, y, 0, 1]); },
  vertexAttrib3f(i, x, y, z) { setCurrent(this, i, [x, y, z, 1]); },
  vertexAttrib4f(i, x, y, z, w) { setCurrent(this, i, [x, y, z, w]); },
  vertexAttrib1fv(i, v) { setCurrent(this, i, [v[0], 0, 0, 1]); },
  vertexAttrib2fv(i, v) { setCurrent(this, i, [v[0], v[1], 0, 1]); },
  vertexAttrib3fv(i, v) { setCurrent(this, i, [v[0], v[1], v[2], 1]); },
  vertexAttrib4fv(i, v) { setCurrent(this, i, [v[0], v[1], v[2], v[3]]); },
};
// Os uniform*: o sufixo diz quantos componentes e de que tipo; `v` aceita sequência.
for (const n of [1, 2, 3, 4]) {
  for (const t of ['f', 'i']) {
    glMethods[`uniform${n}${t}`] = function (location, ...values) { if (!glState(this).lost) setUniform(this, location, values.slice(0, n), { count: n, type: t }); };
    glMethods[`uniform${n}${t}v`] = function (location, values, srcOffset = 0, srcLength = 0) { if (!glState(this).lost) setUniform(this, location, sliceValues(values, srcOffset, srcLength), { count: n, type: t, vector: true }); };
  }
}
for (const n of [2, 3, 4]) glMethods[`uniformMatrix${n}fv`] = function (location, transpose, values, srcOffset = 0, srcLength = 0) {
  if (glState(this).lost) return;
  if (transpose && glState(this).version !== 2) { glError(this, GL.INVALID_VALUE); return; }
  setUniform(this, location, sliceValues(values, srcOffset, srcLength), { count: n * n, type: 'f', vector: true });
};

// Só do WebGL2.
const gl2Methods = {
  bufferData: glMethods.bufferData,
  getBufferSubData(target, srcByteOffset, dst, dstOffset = 0, length = 0) {
    const s = glState(this);
    const name = bufferTarget(this, target);
    if (!name) return;
    const buffer = boundBuffer(s, name);
    if (!buffer) { glError(this, GL.INVALID_OPERATION); return; }
    const b = objOf(buffer);
    const elem = dst.BYTES_PER_ELEMENT || 1;
    const out = new Uint8Array(dst.buffer, dst.byteOffset + dstOffset * elem, (length || dst.length - dstOffset) * elem);
    if (!b.data || srcByteOffset + out.length > b.data.length) { glError(this, GL.INVALID_VALUE); return; }
    out.set(b.data.subarray(srcByteOffset, srcByteOffset + out.length));
  },
  copyBufferSubData(readTarget, writeTarget, readOffset, writeOffset, size) {
    const s = glState(this);
    const r = bufferTarget(this, readTarget), w = bufferTarget(this, writeTarget);
    if (!r || !w) return;
    const rb = boundBuffer(s, r), wb = boundBuffer(s, w);
    if (!rb || !wb) { glError(this, GL.INVALID_OPERATION); return; }
    const src = objOf(rb).data, dst = objOf(wb).data;
    if (!src || !dst || readOffset + size > src.length || writeOffset + size > dst.length) { glError(this, GL.INVALID_VALUE); return; }
    glBeforeWrite(s, objOf(wb));
    dst.set(src.slice(readOffset, readOffset + size), writeOffset);
  },
  bindBufferBase(target, index, buffer) { this.bindBufferRange(target, index, buffer, 0, 0); },
  bindBufferRange(target, index, buffer) {
    const s = glState(this);
    if (s.lost) return;
    const name = enumName(s, target, ['UNIFORM_BUFFER', 'TRANSFORM_FEEDBACK_BUFFER']);
    if (!name) { glError(this, GL.INVALID_ENUM); return; }
    const max = name === 'UNIFORM_BUFFER' ? s.table.params.MAX_UNIFORM_BUFFER_BINDINGS : s.table.params.MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS;
    if (index >= max) { glError(this, GL.INVALID_VALUE); return; }
    if (!validObject(this, buffer, WebGLBuffer, 'bindBufferRange')) return;
    s.indexed.set(name + ':' + index, buffer);
    s.params.set(name + '_BINDING', buffer);
  },
  getIndexedParameter(target, index) {
    const s = glState(this);
    const name = enumName(s, target, ['UNIFORM_BUFFER_BINDING', 'TRANSFORM_FEEDBACK_BUFFER_BINDING']);
    if (!name) { glError(this, GL.INVALID_ENUM); return null; }
    return s.indexed.get(name.replace('_BINDING', '') + ':' + index) ?? null;
  },
  createVertexArray() { return glState(this).lost ? null : createVao(this, WebGLVertexArrayObject); },
  deleteVertexArray(v) { deleteVao(this, v); },
  isVertexArray(v) { return !glState(this).lost && isObject(this, v, WebGLVertexArrayObject); },
  bindVertexArray(v) { if (!glState(this).lost) bindVao(this, v); },
  vertexAttribDivisor(index, value) { divisor(this, index, value); },
  vertexAttribIPointer(index, size, type, stride, offset) {
    const s = glState(this);
    if (index >= s.vao.attribs.length || size < 1 || size > 4 || stride < 0 || stride > 255 || offset < 0) { glError(this, GL.INVALID_VALUE); return; }
    if (!enumName(s, type, ['BYTE', 'UNSIGNED_BYTE', 'SHORT', 'UNSIGNED_SHORT', 'INT', 'UNSIGNED_INT'])) { glError(this, GL.INVALID_ENUM); return; }
    Object.assign(s.vao.attribs[index], { size, type, normalized: false, stride, offset, buffer: s.params.get('ARRAY_BUFFER_BINDING'), integer: true });
  },
  vertexAttribI4i(i, x, y, z, w) { setCurrent(this, i, [x, y, z, w], Int32Array); },
  vertexAttribI4ui(i, x, y, z, w) { setCurrent(this, i, [x, y, z, w], Uint32Array); },
  vertexAttribI4iv(i, v) { setCurrent(this, i, Array.from(v).slice(0, 4), Int32Array); },
  vertexAttribI4uiv(i, v) { setCurrent(this, i, Array.from(v).slice(0, 4), Uint32Array); },
  drawArraysInstanced(mode, first, count, instances) { drawArrays(this, mode, first, count, instances); },
  drawElementsInstanced(mode, count, type, offset, instances) { drawElements(this, mode, count, type, offset, instances); },
  drawRangeElements(mode, start, end, count, type, offset) { if (end < start) { glError(this, GL.INVALID_VALUE); return; } drawElements(this, mode, count, type, offset, 1); },
  drawBuffers(buffers) {
    const s = glState(this);
    const list = Array.from(buffers);
    if (list.length > s.table.params.MAX_DRAW_BUFFERS) { glError(this, GL.INVALID_VALUE); return; }
    const fb = s.params.get('DRAW_FRAMEBUFFER_BINDING');
    if (!fb && (list.length !== 1 || (list[0] !== GL.BACK && list[0] !== GL.NONE))) { glError(this, GL.INVALID_OPERATION); return; }
    for (let i = 0; i < s.table.params.MAX_DRAW_BUFFERS; i++) s.params.set('DRAW_BUFFER' + i, i < list.length ? list[i] : GL.NONE);
  },
  readBuffer(src) {
    const s = glState(this);
    const fb = s.params.get('READ_FRAMEBUFFER_BINDING');
    if (!fb && src !== GL.BACK && src !== GL.NONE) { glError(this, GL.INVALID_OPERATION); return; }
    s.params.set('READ_BUFFER', src);
  },
  clearBufferfv() {}, clearBufferiv() {}, clearBufferuiv() {}, clearBufferfi() {},
  blitFramebuffer() {},
  invalidateFramebuffer() {}, invalidateSubFramebuffer() {},
  framebufferTextureLayer(target, attachment, texture, level, layer) {
    const s = glState(this);
    const slot = framebufferTarget(this, target);
    if (!slot || !validObject(this, texture, WebGLTexture, 'framebufferTextureLayer')) return;
    const fb = s.params.get(slot);
    if (!fb) { glError(this, GL.INVALID_OPERATION); return; }
    if (texture) objOf(fb).attachments.set(attachment, { object: texture, level, layer }); else objOf(fb).attachments.delete(attachment);
  },
  renderbufferStorageMultisample(target, samples, internalformat, width, height) { renderbufferStorage(this, target, samples, internalformat, width, height); },
  getInternalformatParameter(target, internalformat, pname) {
    if (target !== GL.RENDERBUFFER) { glError(this, GL.INVALID_ENUM); return null; }
    if (pname !== GL.SAMPLES) { glError(this, GL.INVALID_ENUM); return null; }
    return new Int32Array([4]);
  },
  texStorage2D(target, levels, internalformat, width, height) { texStorage(this, target, levels, internalformat, [width, height, 1], false); },
  texStorage3D(target, levels, internalformat, width, height, depth) { texStorage(this, target, levels, internalformat, [width, height, depth], true); },
  texImage3D(target, level, internalformat, width, height, depth, border, format, type, source, srcOffset) {
    const s = glState(this);
    if (s.lost) return;
    const name = enumName(s, target, ['TEXTURE_3D', 'TEXTURE_2D_ARRAY']);
    if (!name) { glError(this, GL.INVALID_ENUM); return; }
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    texImage(this, s, objOf(texture), target, level, internalformat, [width, height, depth], border, format, type, source, srcOffset);
  },
  texSubImage3D(target, level, xoffset, yoffset, zoffset, width, height, depth, format, type, source, srcOffset) {
    const s = glState(this);
    if (s.lost) return;
    const name = enumName(s, target, ['TEXTURE_3D', 'TEXTURE_2D_ARRAY']);
    if (!name) { glError(this, GL.INVALID_ENUM); return; }
    const texture = boundTexture(s, name);
    if (!texture) { glError(this, GL.INVALID_OPERATION); return; }
    texSubImage(this, s, objOf(texture), target, level, [xoffset, yoffset, zoffset], [width, height, depth], format, type, source, srcOffset);
  },
  copyTexSubImage3D() {}, compressedTexImage3D() {}, compressedTexSubImage3D() {},
  getFragDataLocation(program, name) {
    const p = programOf(this, program, 'getFragDataLocation');
    if (!p) return -1;
    if (!p.linked) { glError(this, GL.INVALID_OPERATION); return -1; }
    const fs = p.shaders.map(objOf).find((sh) => sh.type === GL.FRAGMENT_SHADER).parsed;
    const outs = fs.decls.filter((d) => d.qualifier === 'out');
    const i = outs.findIndex((d) => d.name === String(name));
    if (i < 0) return -1;
    return outs[i].location >= 0 ? outs[i].location : outs.length === 1 ? 0 : -1;
  },
  getUniformBlockIndex(program, name) {
    const p = programOf(this, program, 'getUniformBlockIndex');
    if (!p) return GL.INVALID_INDEX;
    const i = p.blocks.findIndex((b) => b.name === String(name));
    return i < 0 ? GL.INVALID_INDEX : i;
  },
  uniformBlockBinding(program, index, binding) {
    const p = programOf(this, program, 'uniformBlockBinding');
    if (!p) return;
    if (!p.blocks[index] || binding >= glState(this).table.params.MAX_UNIFORM_BUFFER_BINDINGS) { glError(this, GL.INVALID_VALUE); return; }
    p.blocks[index].binding = binding;
  },
  getActiveUniformBlockName(program, index) {
    const p = programOf(this, program, 'getActiveUniformBlockName');
    if (!p) return null;
    if (!p.blocks[index]) { glError(this, GL.INVALID_VALUE); return null; }
    return p.blocks[index].name;
  },
  getActiveUniformBlockParameter(program, index, pname) {
    const p = programOf(this, program, 'getActiveUniformBlockParameter');
    if (!p) return null;
    const b = p.blocks[index];
    if (!b) { glError(this, GL.INVALID_VALUE); return null; }
    switch (pname) {
      case GL.UNIFORM_BLOCK_BINDING: return b.binding;
      case GL.UNIFORM_BLOCK_DATA_SIZE: return b.size;
      case GL.UNIFORM_BLOCK_ACTIVE_UNIFORMS: return b.members.length;
      case GL.UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES: return new Uint32Array(p.uniforms.map((u, i) => (u.block === index ? i : -1)).filter((i) => i >= 0));
      case GL.UNIFORM_BLOCK_REFERENCED_BY_VERTEX_SHADER: return b.vertex;
      case GL.UNIFORM_BLOCK_REFERENCED_BY_FRAGMENT_SHADER: return b.fragment;
    }
    glError(this, GL.INVALID_ENUM);
    return null;
  },
  getUniformIndices(program, names) {
    const p = programOf(this, program, 'getUniformIndices');
    if (!p) return null;
    return Array.from(names, (n) => { const i = p.uniforms.findIndex((u) => u.name === n || u.base === n); return i < 0 ? GL.INVALID_INDEX : i; });
  },
  getActiveUniforms(program, indices, pname) {
    const p = programOf(this, program, 'getActiveUniforms');
    if (!p) return null;
    return Array.from(indices, (i) => {
      const u = p.uniforms[i];
      if (!u) return null;
      switch (pname) {
        case GL.UNIFORM_TYPE: return u.type;
        case GL.UNIFORM_SIZE: return u.size;
        case GL.UNIFORM_BLOCK_INDEX: return u.block ?? -1;
        case GL.UNIFORM_IS_ROW_MAJOR: return false;
        default: return u.block === undefined ? -1 : 0;
      }
    });
  },
  uniform1ui(l, x) { setUniform(this, l, [x], { count: 1, type: 'ui' }); },
  uniform2ui(l, x, y) { setUniform(this, l, [x, y], { count: 2, type: 'ui' }); },
  uniform3ui(l, x, y, z) { setUniform(this, l, [x, y, z], { count: 3, type: 'ui' }); },
  uniform4ui(l, x, y, z, w) { setUniform(this, l, [x, y, z, w], { count: 4, type: 'ui' }); },
  transformFeedbackVaryings(program, varyings, mode) {
    const p = programOf(this, program, 'transformFeedbackVaryings');
    if (!p) return;
    p.feedbackVaryings = Array.from(varyings, String);
    p.feedbackMode = mode;
  },
  getTransformFeedbackVarying(program, index) {
    const p = programOf(this, program, 'getTransformFeedbackVarying');
    if (!p) return null;
    const name = p.feedbackVaryings && p.feedbackVaryings[index];
    if (!p.linked || name === undefined) { glError(this, GL.INVALID_VALUE); return null; }
    const vs = p.shaders.map(objOf).find((sh) => sh.type === GL.VERTEX_SHADER).parsed;
    const d = vs.decls.find((x) => x.qualifier === 'out' && x.name === name);
    return withInfo(WebGLActiveInfo, { size: d ? d.array || 1 : 1, type: d ? GL[GLSL_TYPES[d.type]] : GL.FLOAT, name });
  },
  createTransformFeedback() { return mkObject(this, WebGLTransformFeedback, { used: false }); },
  deleteTransformFeedback(t) { deleteObject(this, t, WebGLTransformFeedback, 'deleteTransformFeedback', unbindEverywhere); },
  isTransformFeedback(t) { return isObject(this, t, WebGLTransformFeedback); },
  bindTransformFeedback(target, t) {
    if (target !== GL.TRANSFORM_FEEDBACK) { glError(this, GL.INVALID_ENUM); return; }
    if (!validObject(this, t, WebGLTransformFeedback, 'bindTransformFeedback')) return;
    if (t) objOf(t).used = true;
    glState(this).params.set('TRANSFORM_FEEDBACK_BINDING', t);
  },
  beginTransformFeedback() { glState(this).params.set('TRANSFORM_FEEDBACK_ACTIVE', true); },
  endTransformFeedback() { const s = glState(this); s.params.set('TRANSFORM_FEEDBACK_ACTIVE', false); s.params.set('TRANSFORM_FEEDBACK_PAUSED', false); },
  pauseTransformFeedback() { glState(this).params.set('TRANSFORM_FEEDBACK_PAUSED', true); },
  resumeTransformFeedback() { glState(this).params.set('TRANSFORM_FEEDBACK_PAUSED', false); },
  createQuery() { return mkObject(this, WebGLQuery, { target: null, used: false }); },
  deleteQuery(q) { deleteObject(this, q, WebGLQuery, 'deleteQuery'); },
  isQuery(q) { return isObject(this, q, WebGLQuery); },
  beginQuery(target, query) {
    const s = glState(this);
    if (!enumName(s, target, ['ANY_SAMPLES_PASSED', 'ANY_SAMPLES_PASSED_CONSERVATIVE', 'TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN'])) { glError(this, GL.INVALID_ENUM); return; }
    if (!query || s.queries.get(target)) { glError(this, GL.INVALID_OPERATION); return; }
    const q = objOf(query);
    q.target = target; q.used = true; q.available = false;
    s.queries.set(target, query);
  },
  endQuery(target) {
    const s = glState(this);
    const query = s.queries.get(target);
    if (!query) { glError(this, GL.INVALID_OPERATION); return; }
    s.queries.delete(target);
    // O resultado fica disponível na próxima tarefa, como no Chromium (nunca no mesmo quadro de script).
    setTimeout(() => { objOf(query).available = true; }, 0);
  },
  getQuery(target, pname) {
    if (pname !== GL.CURRENT_QUERY) { glError(this, GL.INVALID_ENUM); return null; }
    return glState(this).queries.get(target) ?? null;
  },
  getQueryParameter(query, pname) {
    const q = objOf(query);
    if (pname === GL.QUERY_RESULT_AVAILABLE) return !!q.available;
    if (pname === GL.QUERY_RESULT) return q.available ? 0 : 0;
    glError(this, GL.INVALID_ENUM);
    return null;
  },
  createSampler() { return mkObject(this, WebGLSampler, { params: newSamplerParams() }); },
  deleteSampler(sm) { deleteObject(this, sm, WebGLSampler, 'deleteSampler', unbindEverywhere); },
  isSampler(sm) { return isObject(this, sm, WebGLSampler); },
  bindSampler(unit, sampler) {
    const s = glState(this);
    if (unit >= s.units.length) { glError(this, GL.INVALID_VALUE); return; }
    if (!validObject(this, sampler, WebGLSampler, 'bindSampler')) return;
    s.units[unit].sampler = sampler;
  },
  samplerParameteri(sampler, pname, param) { samplerParameter(this, sampler, pname, param | 0); },
  samplerParameterf(sampler, pname, param) { samplerParameter(this, sampler, pname, Number(param)); },
  getSamplerParameter(sampler, pname) {
    const sm = objOf(sampler);
    const key = paramName(sm.params, pname);
    if (!key) { glError(this, GL.INVALID_ENUM); return null; }
    return sm.params.get(key);
  },
  fenceSync(condition, flags) {
    if (condition !== GL.SYNC_GPU_COMMANDS_COMPLETE) { glError(this, GL.INVALID_ENUM); return null; }
    if (flags !== 0) { glError(this, GL.INVALID_VALUE); return null; }
    const sync = mkObject(this, WebGLSync, { signaled: false });
    setTimeout(() => { objOf(sync).signaled = true; }, 0);
    return sync;
  },
  deleteSync(sync) { deleteObject(this, sync, WebGLSync, 'deleteSync'); },
  isSync(sync) { return isObject(this, sync, WebGLSync); },
  clientWaitSync(sync) { return objOf(sync).signaled ? GL.ALREADY_SIGNALED : GL.TIMEOUT_EXPIRED; },
  waitSync() {},
  getSyncParameter(sync, pname) {
    const st = objOf(sync);
    switch (pname) {
      case GL.OBJECT_TYPE: return GL.SYNC_FENCE;
      case GL.SYNC_STATUS: return st.signaled ? GL.SIGNALED : GL.UNSIGNALED;
      case GL.SYNC_CONDITION: return GL.SYNC_GPU_COMMANDS_COMPLETE;
      case GL.SYNC_FLAGS: return 0;
    }
    glError(this, GL.INVALID_ENUM);
    return null;
  },
};
for (const n of [1, 2, 3, 4]) gl2Methods[`uniform${n}uiv`] = function (location, values, srcOffset = 0, srcLength = 0) { setUniform(this, location, sliceValues(values, srcOffset, srcLength), { count: n, type: 'ui', vector: true }); };
for (const [a, b] of [[2, 3], [2, 4], [3, 2], [3, 4], [4, 2], [4, 3]]) gl2Methods[`uniformMatrix${a}x${b}fv`] = function (location, transpose, values, srcOffset = 0, srcLength = 0) { setUniform(this, location, sliceValues(values, srcOffset, srcLength), { count: a * b, type: 'f', vector: true }); };

const sliceValues = (values, srcOffset, srcLength) => { const all = Array.from(values); return all.slice(srcOffset, srcLength ? srcOffset + srcLength : undefined); };
const viewBytes = (data, srcOffset, length) => {
  if (data instanceof ArrayBuffer) return new Uint8Array(data.slice(0));
  if (ArrayBuffer.isView(data)) {
    const elem = data.BYTES_PER_ELEMENT || 1;
    const start = srcOffset * elem;
    const end = length ? start + length * elem : data.byteLength;
    return new Uint8Array(data.buffer.slice(data.byteOffset + start, data.byteOffset + end));
  }
  throw new TypeError("Failed to execute 'bufferData' on 'WebGL2RenderingContext': The provided value is not of type '(ArrayBuffer or ArrayBufferView)'.");
};
const faces = (face) => (face === GL.FRONT ? ['_'] : face === GL.BACK ? ['_BACK_'] : ['_', '_BACK_']);
const setParam = (ctx, name, value) => { const s = glState(ctx); if (!s.lost) s.params.set(name, value); };
const setEnum = (ctx, name, value, allowed) => {
  const s = glState(ctx);
  if (s.lost) return;
  if (!enumName(s, value, allowed)) { glError(ctx, GL.INVALID_ENUM); return; }
  s.params.set(name, value);
};
const setCap = (ctx, cap, on) => {
  const s = glState(ctx);
  if (s.lost) return;
  const name = enumName(s, cap, s.version === 2 ? CAPABILITIES2 : CAPABILITIES);
  if (!name) { glError(ctx, GL.INVALID_ENUM); return; }
  s.params.set(name, on);
};
const setAttrib = (ctx, index, fn) => {
  const s = glState(ctx);
  if (s.lost) return;
  if (index >= s.vao.attribs.length) { glError(ctx, GL.INVALID_VALUE); return; }
  fn(s.vao.attribs[index]);
};
const setCurrent = (ctx, index, values, Type = Float32Array) => {
  const s = glState(ctx);
  if (s.lost) return;
  if (index >= s.current.length) { glError(ctx, GL.INVALID_VALUE); return; }
  s.current[index] = new Type(values);
};
const texParameter = (ctx, target, pname, value) => {
  const s = glState(ctx);
  if (s.lost) return;
  const name = textureTarget(ctx, target);
  if (!name) return;
  const texture = boundTexture(s, name);
  if (!texture) { glError(ctx, GL.INVALID_OPERATION); return; }
  const t = objOf(texture);
  if (pname === 34046 && s.extensions.has('EXT_texture_filter_anisotropic')) { if (value < 1) { glError(ctx, GL.INVALID_VALUE); return; } t.anisotropy = value; return; }
  const key = paramName(t.params, pname);
  if (!key || key === 'TEXTURE_IMMUTABLE_FORMAT' || key === 'TEXTURE_IMMUTABLE_LEVELS' || (s.version !== 2 && !['TEXTURE_MIN_FILTER', 'TEXTURE_MAG_FILTER', 'TEXTURE_WRAP_S', 'TEXTURE_WRAP_T'].includes(key))) { glError(ctx, GL.INVALID_ENUM); return; }
  t.params.set(key, value);
};
const samplerParameter = (ctx, sampler, pname, value) => {
  const sm = objOf(sampler);
  const key = paramName(sm.params, pname);
  if (!key) { glError(ctx, GL.INVALID_ENUM); return; }
  sm.params.set(key, value);
};
const renderbufferStorage = (ctx, target, samples, internalformat, width, height) => {
  const s = glState(ctx);
  if (s.lost) return;
  if (target !== GL.RENDERBUFFER) { glError(ctx, GL.INVALID_ENUM); return; }
  const rb = s.params.get('RENDERBUFFER_BINDING');
  if (!rb) { glError(ctx, GL.INVALID_OPERATION); return; }
  if (width < 0 || height < 0 || width > s.table.params.MAX_RENDERBUFFER_SIZE || height > s.table.params.MAX_RENDERBUFFER_SIZE) { glError(ctx, GL.INVALID_VALUE); return; }
  const formatName = Object.keys(GL).find((k) => GL[k] === internalformat && /^(RGB|RGBA|DEPTH|STENCIL|R8|RG8|R16|RG16|R32|RG32|SRGB)/.test(k));
  if (!formatName) { glError(ctx, GL.INVALID_ENUM); return; }
  Object.assign(objOf(rb), { size: [width, height], format: internalformat, formatName, samples });
};
// O que o compilador do ANGLE rejeita antes de qualquer análise: o fonte vazio e chaves desbalanceadas. A mensagem
// segue o formato do ANGLE (`ERROR: 0:linha: 'símbolo' : descrição`).
const glslProblem = (source, type) => {
  const src = stripComments(source);
  if (!/\bvoid\s+main\s*\(/.test(src)) return "ERROR: 0:1: '' : Missing main()\n";
  let depth = 0, line = 1;
  for (const ch of src) {
    if (ch === '\n') line++;
    if (ch === '{') depth++;
    if (ch === '}' && --depth < 0) return `ERROR: 0:${line}: '}' : syntax error\n`;
  }
  if (depth !== 0) return `ERROR: 0:${line}: '' : syntax error\n`;
  return '';
};

// As classes de contexto: métodos e constantes na ordem do protótipo medido.
const makeContextClass = (name, version) => {
  const table = WEBGL_TABLE[version === 2 ? 'webgl2' : 'webgl'];
  const cls = class { constructor() { throw new TypeError('Illegal constructor'); } };
  Object.defineProperty(cls, 'name', { value: name });
  const all = version === 2 ? { ...glMethods, ...gl2Methods } : glMethods;
  for (const key of table.methods) {
    if (key === 'constructor') continue;
    if (key in table.consts) { Object.defineProperty(cls.prototype, key, { value: table.consts[key], enumerable: true }); continue; }
    if (key === 'canvas') { Object.defineProperty(cls.prototype, key, { get() { return glState(this).canvas; }, enumerable: true, configurable: true }); continue; }
    if (key === 'drawingBufferWidth' || key === 'drawingBufferHeight') {
      Object.defineProperty(cls.prototype, key, { get() { const s = glState(this); return s.lost ? 0 : key === 'drawingBufferWidth' ? s.canvas.width : s.canvas.height; }, enumerable: true, configurable: true });
      continue;
    }
    if (key === 'drawingBufferColorSpace' || key === 'unpackColorSpace') {
      Object.defineProperty(cls.prototype, key, { get() { return this[P['__' + key]] ?? 'srgb'; }, set(v) { if (v === 'srgb' || v === 'display-p3') hidden(this, '__' + key, v); }, enumerable: true, configurable: true });
      continue;
    }
    if (key === 'drawingBufferFormat') { Object.defineProperty(cls.prototype, key, { get() { return GL.RGBA8; }, enumerable: true, configurable: true }); continue; }
    const fn = all[key] || (key === 'drawingBufferStorage' ? function () {} : key === 'makeXRCompatible' ? function () { return Promise.resolve(); } : null);
    if (!fn) continue;
    const method = { [key]: function (...args) { return fn.apply(this, args); } }[key];
    Object.defineProperty(method, 'length', { value: WEBGL_EXTENSIONS.lengths[version === 2 ? 'webgl2' : 'webgl'][key] ?? fn.length });
    Object.defineProperty(cls.prototype, key, { value: method, writable: true, enumerable: true, configurable: true });
  }
  for (const [k, v] of Object.entries(table.consts)) {
    if (!Object.prototype.hasOwnProperty.call(cls.prototype, k)) Object.defineProperty(cls.prototype, k, { value: v, enumerable: true });
    Object.defineProperty(cls, k, { value: v, enumerable: true });
  }
  tag(cls, name);
  return cls;
};
const WebGLRenderingContext = makeContextClass('WebGLRenderingContext', 1);
const WebGL2RenderingContext = makeContextClass('WebGL2RenderingContext', 2);
// O valor de enum de cada parâmetro com estado, para o getParameter.
const paramIndex = (version) => {
  const table = WEBGL_TABLE[version === 2 ? 'webgl2' : 'webgl'];
  const map = new Map();
  for (const name of Object.keys(table.params)) if (name in table.consts && !map.has(table.consts[name])) map.set(table.consts[name], name);
  return map;
};
const PARAM_INDEX = { 1: paramIndex(1), 2: paramIndex(2) };

// getContext: o mesmo contexto a cada chamada do mesmo tipo; outro tipo no mesmo canvas devolve null.
const CONTEXT_ATTRIBUTE_DEFAULTS = { alpha: true, antialias: true, depth: true, desynchronized: false, failIfMajorPerformanceCaveat: false, powerPreference: 'default', premultipliedAlpha: true, preserveDrawingBuffer: false, stencil: false, xrCompatible: false };
const createWebGLContext = (canvas, version, options) => {
  const attrs = {};
  const given = options && typeof options === 'object' ? options : {};
  for (const [k, def] of Object.entries(CONTEXT_ATTRIBUTE_DEFAULTS)) {
    const v = given[k];
    attrs[k] = v === undefined ? def : k === 'powerPreference' ? (['default', 'low-power', 'high-performance'].includes(String(v)) ? String(v) : def) : Boolean(v);
  }
  const ctx = Object.create((version === 2 ? WebGL2RenderingContext : WebGLRenderingContext).prototype);
  hidden(ctx, '__glName', version === 2 ? 'WebGL2RenderingContext' : 'WebGLRenderingContext');
  const s = newGlState(ctx, version, canvas, attrs);
  s.paramByValue = PARAM_INDEX[version];
  s.extConsts = new Map();
  hidden(ctx, '__gl', s);
  return ctx;
};
