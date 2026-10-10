// ---- Trusted Types: a fábrica `trustedTypes`, as políticas e os três tipos (`TrustedHTML`, `TrustedScript`,
// `TrustedScriptURL`). Comportamento, mensagens e o mapa de tipos de atributos e propriedades são os medidos no
// oráculo (`TRUSTED_TYPES_MAPPING` vem de `trustedTypes.getTypeMapping()`).

const argumentsRequired = (iface, operation, needed, given) => {
  if (given < needed) throw new TypeError(`Failed to execute '${operation}' on '${iface}': ${needed} argument${needed === 1 ? '' : 's'} required, but only ${given} present.`);
};
const trustedValue = (value) => (value === null || value === undefined ? '' : value);

// Um tipo confiável: sem construtor público; `toString` e `toJSON` devolvem o texto guardado.
const makeTrustedType = (name) => {
  const constructor = { [name]: function () {
    if (!new.target) throw new TypeError('Illegal constructor');
    throw illegalConstructor(name);
  } }[name];
  const brand = Symbol(name);
  Object.defineProperty(constructor, 'prototype', { writable: false });
  const read = (self) => {
    if (self === null || typeof self !== 'object' || self[brand] === undefined) throw new TypeError('Illegal invocation');
    return self[brand];
  };
  // Na ordem do WebIDL: as operações, depois o `constructor`.
  delete constructor.prototype.constructor;
  Object.defineProperties(constructor.prototype, {
    toJSON: { value: { toJSON() { return read(this); } }.toJSON, writable: true, configurable: true, enumerable: true },
    toString: { value: { toString() { return read(this); } }.toString, writable: true, configurable: true, enumerable: true },
    constructor: { value: constructor, writable: true, configurable: true },
    [Symbol.toStringTag]: { value: name, configurable: true },
  });
  const create = (text) => Object.create(constructor.prototype, { [brand]: { value: text } });
  const is = (value) => value !== null && typeof value === 'object' && value[brand] !== undefined;
  return { constructor, create, is };
};
const TrustedHTMLType = makeTrustedType('TrustedHTML');
const TrustedScriptType = makeTrustedType('TrustedScript');
const TrustedScriptURLType = makeTrustedType('TrustedScriptURL');
const TRUSTED_KINDS = { createHTML: ['TrustedHTML', TrustedHTMLType], createScript: ['TrustedScript', TrustedScriptType], createScriptURL: ['TrustedScriptURL', TrustedScriptURLType] };
const TRUSTED_CALLBACKS = { createHTML: 'CreateHTMLCallback', createScript: 'CreateScriptCallback', createScriptURL: 'CreateURLCallback' };

class TrustedTypePolicy {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('TrustedTypePolicy');
  }
  get name() { return receiverState(this).name; }
}
const policyState = new WeakMap();
const receiverState = (self) => {
  const state = self !== null && typeof self === 'object' ? policyState.get(self) : undefined;
  if (state === undefined) throw new TypeError('Illegal invocation');
  return state;
};
for (const operation of Object.keys(TRUSTED_KINDS)) {
  const [, type] = TRUSTED_KINDS[operation];
  Object.defineProperty(TrustedTypePolicy.prototype, operation, {
    value: { [operation](input, ...rest) {
      const state = receiverState(this);
      argumentsRequired('TrustedTypePolicy', operation, 1, arguments.length);
      const fail = (error) => (error instanceof TypeError && /^Cannot convert a Symbol/.test(error.message) ? new TypeError(`Failed to execute '${operation}' on 'TrustedTypePolicy': ${error.message}`) : error);
      let text;
      try { text = `${input}`; } catch (error) { throw fail(error); }
      const callback = state.options[operation];
      if (callback === undefined) throw new TypeError(`Failed to execute '${operation}' on 'TrustedTypePolicy': Policy ${state.name}'s TrustedTypePolicyOptions did not specify a '${operation}' member.`);
      const result = trustedValue(callback.call(state.options, text, ...rest));
      try { return type.create(`${result}`); } catch (error) {
        if (operation === 'createScriptURL' && error instanceof TypeError) throw new TypeError(`Failed to execute 'invoke' on '${TRUSTED_CALLBACKS[operation]}': ${fail(error).message}`);
        throw fail(error);
      }
    } }[operation],
    writable: true, configurable: true, enumerable: true,
  });
}
webInterface(TrustedTypePolicy, 'TrustedTypePolicy');

// Os tipos por elemento e atributo ou propriedade, como `getTypeMapping()` os lista; o que depende do espaço de nomes
// (o `href` do `script` só vale no SVG, o `src` só no HTML) sai das regras medidas no oráculo.
const HTML_NAMESPACE = 'http://www.w3.org/1999/xhtml';
const SVG_NAMESPACE = 'http://www.w3.org/2000/svg';
const MATHML_NAMESPACE = 'http://www.w3.org/1998/Math/MathML';
const XLINK_NAMESPACE = 'http://www.w3.org/1999/xlink';
const knownNamespace = (namespace) => namespace === null || namespace === undefined || namespace === '' || namespace === HTML_NAMESPACE || namespace === SVG_NAMESPACE || namespace === MATHML_NAMESPACE;
const mappingFor = (tag, namespace, section) => {
  const element = TRUSTED_TYPES_MAPPING[tag];
  if (namespace === SVG_NAMESPACE) return tag === 'script' ? { href: 'TrustedScriptURL' } : {};
  const entries = { ...(element === undefined ? {} : element[section]) };
  if (tag === 'script') delete entries.href;
  return entries;
};

class TrustedTypePolicyFactory {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('TrustedTypePolicyFactory');
    hidden(this, '__trustedFactory', { defaultPolicy: null, names: new Set(), emptyHTML: TrustedHTMLType.create(''), emptyScript: TrustedScriptType.create('') });
  }
  get defaultPolicy() { return factoryState(this).defaultPolicy; }
  get emptyHTML() { return factoryState(this).emptyHTML; }
  get emptyScript() { return factoryState(this).emptyScript; }
  createPolicy(name, options) {
    const state = factoryState(this);
    argumentsRequired('TrustedTypePolicyFactory', 'createPolicy', 1, arguments.length);
    const prefix = "Failed to execute 'createPolicy' on 'TrustedTypePolicyFactory': ";
    const text = String(name);
    if (options !== undefined && options !== null && typeof options !== 'object' && typeof options !== 'function') throw new TypeError(`${prefix}The provided value is not of type 'TrustedTypePolicyOptions'.`);
    const members = {};
    for (const member of Object.keys(TRUSTED_KINDS)) {
      const callback = options === undefined || options === null ? undefined : options[member];
      if (callback === undefined) continue;
      if (typeof callback !== 'function') throw new TypeError(`${prefix}Failed to read the '${member}' property from 'TrustedTypePolicyOptions': The given value is not a function.`);
      members[member] = callback;
    }
    // A diretiva `trusted-types` do CSP (nomes permitidos, repetição) vem antes do nome `default` repetido.
    const refusal = cspCreatePolicyCheck(text, state.names.has(text));
    if (refusal !== null) throw new TypeError(prefix + refusal);
    state.names.add(text);
    if (text === 'default' && state.defaultPolicy !== null) throw new TypeError(`${prefix}Policy with name "default" already exists.`);
    const policy = Object.create(TrustedTypePolicy.prototype);
    policyState.set(policy, { name: text, options: members });
    if (text === 'default') state.defaultPolicy = policy;
    return policy;
  }
  getAttributeType(tagName, attribute, elementNamespace, attributeNamespace) {
    factoryState(this);
    argumentsRequired('TrustedTypePolicyFactory', 'getAttributeType', 2, arguments.length);
    const tag = String(tagName).toLowerCase();
    const name = String(attribute).toLowerCase();
    if (!knownNamespace(elementNamespace)) return null;
    const hasAttributeNamespace = attributeNamespace !== null && attributeNamespace !== undefined && attributeNamespace !== '';
    if (hasAttributeNamespace && !(attributeNamespace === XLINK_NAMESPACE && elementNamespace === SVG_NAMESPACE && tag === 'script' && name === 'href')) return null;
    if (!hasAttributeNamespace && TRUSTED_TYPES_MAPPING['*'].attributes[name] !== undefined) return TRUSTED_TYPES_MAPPING['*'].attributes[name];
    return mappingFor(tag, elementNamespace === '' ? null : elementNamespace, 'attributes')[name] ?? null;
  }
  getPropertyType(tagName, property, elementNamespace) {
    factoryState(this);
    argumentsRequired('TrustedTypePolicyFactory', 'getPropertyType', 2, arguments.length);
    const tag = String(tagName).toLowerCase();
    const name = String(property);
    if (TRUSTED_TYPES_MAPPING['*'].properties[name] !== undefined) return TRUSTED_TYPES_MAPPING['*'].properties[name];
    return mappingFor(tag, elementNamespace, 'properties')[name] ?? null;
  }
  // Só o espaço de nomes do HTML tem mapa (a lista é nova a cada chamada); com argumento o resultado é sempre nulo.
  getTypeMapping(namespace) {
    factoryState(this);
    if (arguments.length > 0 && namespace !== undefined) return null;
    return JSON.parse(JSON.stringify(TRUSTED_TYPES_MAPPING));
  }
  isHTML(value) { factoryState(this); argumentsRequired('TrustedTypePolicyFactory', 'isHTML', 1, arguments.length); return TrustedHTMLType.is(value); }
  isScript(value) { factoryState(this); argumentsRequired('TrustedTypePolicyFactory', 'isScript', 1, arguments.length); return TrustedScriptType.is(value); }
  isScriptURL(value) { factoryState(this); argumentsRequired('TrustedTypePolicyFactory', 'isScriptURL', 1, arguments.length); return TrustedScriptURLType.is(value); }
}
const factoryState = (self) => {
  const state = self !== null && typeof self === 'object' ? self[P.__trustedFactory] : undefined;
  if (state === undefined) throw new TypeError('Illegal invocation');
  return state;
};
webInterface(TrustedTypePolicyFactory, 'TrustedTypePolicyFactory');
const trustedTypes = new TrustedTypePolicyFactory(ILLEGAL);
readonly('trustedTypes', () => trustedTypes);

const trustedApis = { TrustedHTML: TrustedHTMLType.constructor, TrustedScript: TrustedScriptType.constructor, TrustedScriptURL: TrustedScriptURLType.constructor, TrustedTypePolicy, TrustedTypePolicyFactory };
Object.assign(globalThis, trustedApis);
for (const key of Object.keys(trustedApis)) Object.defineProperty(globalThis, key, { enumerable: false });
