// ---- Imposição do Trusted Types pelo CSP (`require-trusted-types-for 'script'` e `trusted-types`), como o Chromium:
// os sinks do DOM e do motor recusam texto puro com o `TypeError` ou `EvalError` medido no oráculo, a política padrão
// converte o que a página passa, e cada violação (ou relato, no modo só relatório) vira um
// `SecurityPolicyViolationEvent` no documento. Só estas duas diretivas do CSP são interpretadas.

class SecurityPolicyViolationEvent extends Event {
  constructor(type, init = {}) {
    super(type, init);
    const text = (key) => (init[key] === undefined ? '' : String(init[key]));
    hidden(this, '__violation', {
      documentURI: text('documentURI'), referrer: text('referrer'), blockedURI: text('blockedURI'), effectiveDirective: text('effectiveDirective'),
      violatedDirective: text('violatedDirective'), originalPolicy: text('originalPolicy'), sourceFile: text('sourceFile'), sample: text('sample'),
      disposition: init.disposition === undefined ? 'enforce' : String(init.disposition), statusCode: (init.statusCode >>> 0) & 0xFFFF,
      lineNumber: init.lineNumber >>> 0, columnNumber: init.columnNumber >>> 0,
    });
  }
  get documentURI() { return this[P.__violation].documentURI; }
  get referrer() { return this[P.__violation].referrer; }
  get blockedURI() { return this[P.__violation].blockedURI; }
  get effectiveDirective() { return this[P.__violation].effectiveDirective; }
  get violatedDirective() { return this[P.__violation].violatedDirective; }
  get originalPolicy() { return this[P.__violation].originalPolicy; }
  get sourceFile() { return this[P.__violation].sourceFile; }
  get sample() { return this[P.__violation].sample; }
  get disposition() { return this[P.__violation].disposition; }
  get statusCode() { return this[P.__violation].statusCode; }
  get lineNumber() { return this[P.__violation].lineNumber; }
  get columnNumber() { return this[P.__violation].columnNumber; }
}
webInterface(SecurityPolicyViolationEvent, 'SecurityPolicyViolationEvent');
Object.assign(globalThis, { SecurityPolicyViolationEvent });
Object.defineProperty(globalThis, 'SecurityPolicyViolationEvent', { enumerable: false });

const parsePolicy = (text, reportOnly) => {
  const directives = new Map();
  for (const piece of text.split(';')) {
    const tokens = piece.trim().split(/\s+/).filter((token) => token !== '');
    if (tokens.length === 0) continue;
    const name = tokens.shift().toLowerCase();
    if (!directives.has(name)) directives.set(name, tokens);
  }
  return { text, reportOnly, directives };
};
const headerPolicies = DOCUMENT_CSP.map(([text, reportOnly]) => parsePolicy(text, reportOnly));
// O CSP de `<meta http-equiv>` vale a partir da inserção do elemento no `<head>`, e depois de inserido não se desfaz
// (remover o `<meta>` não tira a política); fora do `<head>`, ou de um `<meta>` solto, não vale (medido no oráculo).
// Os `<meta>` do documento entram quando o script que os segue na ordem do documento roda, os da página quando
// inseridos.
let metaPolicies = [];
const registeredMeta = new WeakMap();
let staticMetaComplete = false;
let guardInstalled = false;
let scriptRequirements = [];
const requiresTrustedScript = () => scriptRequirements;
const policiesFor = (directive) => [...headerPolicies, ...metaPolicies].filter((policy) => policy.directives.has(directive));
const recomputePolicies = () => {
  scriptRequirements = policiesFor('require-trusted-types-for').filter((policy) => policy.directives.get('require-trusted-types-for').includes("'script'"));
  if (!guardInstalled && scriptRequirements.length > 0) {
    guardInstalled = true;
    setCodeGuard(codeGuard);
  }
};
// Registra o CSP de um `<meta>` já no `<head>`; devolve se mudou alguma política.
const registerMeta = (meta) => {
  if (!document.head.contains(meta) || lower(meta.getAttribute('http-equiv') || '') !== 'content-security-policy') return false;
  const content = meta.getAttribute('content') || '';
  if (registeredMeta.get(meta) === content) return false;
  registeredMeta.set(meta, content);
  metaPolicies.push(parsePolicy(content, false));
  return true;
};
const noteInserted = (node) => {
  if (node !== null && typeof node === 'object' && node.localName === 'meta' && node.isConnected && registerMeta(node)) recomputePolicies();
};
// Antes de cada script: os `<meta>` do documento que o precedem (todos, depois do parse).
const refreshPolicies = () => {
  const current = document[P.__currentScript];
  let changed = false;
  for (const meta of document.head ? document.head.querySelectorAll('meta[http-equiv]') : []) {
    const before = staticMetaComplete || !current || !!(meta.compareDocumentPosition(current) & Node.DOCUMENT_POSITION_FOLLOWING);
    if (before && registerMeta(meta)) changed = true;
  }
  if (changed || !guardInstalled) recomputePolicies();
};

// A posição do código da página numa pilha: a primeira moldura com URL (as do navegador não têm), com ou sem nome de
// função (`at f (url:1:2)` ou `at url:1:2`). `null` sem ela.
const stackPosition = (stack) => {
  for (const line of String(stack).split('\n').slice(1)) {
    const found = /\((.+?):(\d+):(\d+)\)$/.exec(line) || /at (.+?):(\d+):(\d+)$/.exec(line);
    if (found && found[1] !== '<anonymous>' && !found[1].startsWith('eval at ')) return [found[1], Number(found[2]), Number(found[3])];
  }
  return null;
};
// A posição do código da página que chamou o navegador.
const callerPosition = () => stackPosition(new Error().stack) || ['', 0, 0];
const reportViolation = (policy, directive, blockedURI, sample, position) => {
  const [sourceFile, lineNumber, columnNumber] = position;
  const event = new SecurityPolicyViolationEvent('securitypolicyviolation', {
    bubbles: true, composed: true, documentURI: location.href, referrer: document.referrer, blockedURI, effectiveDirective: directive, violatedDirective: directive,
    originalPolicy: policy.text, sourceFile, sample, disposition: policy.reportOnly ? 'report' : 'enforce', statusCode: location.protocol === 'http:' || location.protocol === 'https:' ? 200 : 0,
    lineNumber, columnNumber,
  });
  event.isTrusted = true;
  setTimeout(() => document.dispatchEvent(event), 0);
};

// Valida a atribuição de `value` ao sink `sink` (o nome da amostra, como `Element innerHTML`) para o tipo `type`
// (`TrustedHTML`...): devolve o texto a usar, ou lança `fail()`. O que já é do tipo certo passa; senão a política
// padrão decide; sem ela é violação (relato só no modo só relatório).
const TRUSTED_FACTORIES = { TrustedHTML: [TrustedHTMLType, 'createHTML'], TrustedScript: [TrustedScriptType, 'createScript'], TrustedScriptURL: [TrustedScriptURLType, 'createScriptURL'] };
// O setter de `src` e `text` do `<script>` passa por `setAttribute` e `textContent` por dentro: o valor já foi validado e
// a política padrão não roda de novo. Só os tipos de script entram aqui, porque o set deles não executa código da página.
let sinkDepth = 0;
const withinSink = (type, run) => {
  if (type === 'TrustedHTML') return run();
  sinkDepth++;
  try { return run(); } finally { sinkDepth--; }
};
const enforceSink = (sink, type, value, fail, sampleText, fixedPosition) => {
  const policies = requiresTrustedScript();
  if (policies.length === 0 || sinkDepth > 0) return value;
  const [kind, operation] = TRUSTED_FACTORIES[type];
  if (value !== null && typeof value === 'object' && kind.is(value)) return `${value}`;
  const position = fixedPosition === undefined ? callerPosition() : fixedPosition;
  let text;
  try { text = `${value}`; } catch { text = ''; }
  const fallback = trustedTypes.defaultPolicy;
  if (fallback !== null) {
    const state = policyState.get(fallback);
    const callback = state.options[operation];
    if (callback !== undefined) {
      const result = callback.call(state.options, text, type, sink);
      if (result !== null && result !== undefined) return `${result}`;
    }
  }
  const sample = `${sink}|${(sampleText === undefined ? text : sampleText).slice(0, 40)}`;
  let failure = false;
  for (const policy of policies) {
    reportViolation(policy, 'require-trusted-types-for', 'trusted-types-sink', sample, position);
    if (!policy.reportOnly) failure = true;
  }
  if (failure) throw fail();
  return text;
};
const assignmentError = (kind, member, owner, type) => () => new TypeError(
  (kind === 'set' ? `Failed to set the '${member}' property on '${owner}': ` : kind === 'construct' ? `Failed to construct '${member}': ` : `Failed to execute '${member}' on '${owner}': `) + `This document requires '${type}' assignment.`);
const evalError = () => new EvalError("Evaluating a string as JavaScript violates this document's Trusted Type assignment requirements.");

// O gancho do motor: `eval` de texto e `Function`.
const codeGuard = (kind, value) => {
  if (internalCodeDepth > 0) return undefined;
  if (kind === 'eval') {
    if (typeof value === 'object' && value !== null && TrustedScriptType.is(value)) return `${value}`;
    if (typeof value !== 'string') return undefined;
    const policies = requiresTrustedScript();
    if (policies.length === 0) return undefined;
    return enforceSink('eval', 'TrustedScript', value, evalError);
  }
  if (requiresTrustedScript().length === 0) return undefined;
  // O V8 entrega à política e ao relato o texto entre parênteses (`(function anonymous(a\n) {\n...\n})`); o motor
  // compila o texto sem eles, então a volta tira os parênteses.
  const wrapped = `(${value})`;
  const converted = enforceSink('Function', 'TrustedScript', wrapped, evalError, wrapped.replace('function anonymous(', ''));
  if (converted === wrapped) return undefined;
  return converted.startsWith('(') && converted.endsWith(')') ? converted.slice(1, -1) : converted;
};

// Os sinks do DOM: o setter ou o método original recebe o texto já validado.
const getOwn = Object.getOwnPropertyDescriptor;
const guardSetter = (prototype, name, owner, type, applies = () => true, nullToEmpty = false) => {
  const descriptor = getOwn(prototype, name);
  if (descriptor === undefined || descriptor.set === undefined) return;
  const original = descriptor.set;
  const wrapper = getOwn({ set [name](value) {
    // `innerHTML` e `outerHTML` são `[LegacyNullToEmptyString]`: o `null` chega à política e ao relato como `""`.
    if (nullToEmpty && value === null) value = '';
    if (applies(this)) value = enforceSink(`${owner} ${name}`, type, value, assignmentError('set', name, owner, type));
    withinSink(type, () => original.call(this, value));
  } }, name).set;
  Object.defineProperty(prototype, name, { ...descriptor, set: wrapper });
};
const guardMethod = (prototype, name, owner, type, firstArgument, everyArgument = false) => {
  const descriptor = getOwn(prototype, name);
  if (descriptor === undefined || typeof descriptor.value !== 'function') return;
  const original = descriptor.value;
  const wrapper = { [name](...args) {
    for (let index = firstArgument; index < (everyArgument ? args.length : Math.min(args.length, firstArgument + 1)); index++) {
      args[index] = enforceSink(`${owner} ${name}`, type, args[index], assignmentError('call', name, owner, type));
    }
    return original.apply(this, args);
  } }[name];
  Object.defineProperty(wrapper, 'length', { value: original.length });
  Object.defineProperty(prototype, name, { ...descriptor, value: wrapper });
};
const guardInsertion = (prototype, names, inserted) => {
  for (const name of names) {
    const descriptor = getOwn(prototype, name);
    if (descriptor === undefined || typeof descriptor.value !== 'function') continue;
    const original = descriptor.value;
    const wrapper = { [name](...args) {
      const result = original.apply(this, args);
      for (const node of inserted(args)) noteInserted(node);
      return result;
    } }[name];
    Object.defineProperty(wrapper, 'length', { value: original.length });
    Object.defineProperty(prototype, name, { ...descriptor, value: wrapper });
  }
};

// Os sinks cujas classes só existem no fim do prelúdio (os elementos HTML por etiqueta e o `DOMParser`): instalados
// pela passagem final, `installTrustedTypesSinks`.
const installTrustedTypesSinks = () => {
  const elementPrototype = (tag) => Object.getPrototypeOf(document.createElement(tag));
  const scriptPrototype = elementPrototype('script');
  const isScript = (node) => scriptPrototype.isPrototypeOf(node);
  guardSetter(Element.prototype, 'innerHTML', 'Element', 'TrustedHTML', () => true, true);
  guardSetter(Element.prototype, 'outerHTML', 'Element', 'TrustedHTML', () => true, true);
  guardMethod(Element.prototype, 'insertAdjacentHTML', 'Element', 'TrustedHTML', 1);
  guardMethod(Element.prototype, 'setHTMLUnsafe', 'Element', 'TrustedHTML', 0);
  guardSetter(ShadowRoot.prototype, 'innerHTML', 'ShadowRoot', 'TrustedHTML', () => true, true);
  guardMethod(ShadowRoot.prototype, 'setHTMLUnsafe', 'ShadowRoot', 'TrustedHTML', 0);
  guardMethod(Document.prototype, 'write', 'Document', 'TrustedHTML', 0, true);
  guardMethod(Document.prototype, 'writeln', 'Document', 'TrustedHTML', 0, true);
  guardMethod(Document, 'parseHTMLUnsafe', 'Document', 'TrustedHTML', 0);
  guardMethod(DOMParser.prototype, 'parseFromString', 'DOMParser', 'TrustedHTML', 0);
  guardMethod(Range.prototype, 'createContextualFragment', 'Range', 'TrustedHTML', 0);
  guardSetter(scriptPrototype, 'text', 'HTMLScriptElement', 'TrustedScript');
  guardSetter(scriptPrototype, 'src', 'HTMLScriptElement', 'TrustedScriptURL');
  guardSetter(Node.prototype, 'textContent', 'HTMLScriptElement', 'TrustedScript', isScript);
  guardSetter(Element.prototype, 'innerText', 'HTMLScriptElement', 'TrustedScript', isScript);
  guardSetter(HTMLElement.prototype, 'innerText', 'HTMLScriptElement', 'TrustedScript', isScript);
  guardSetter(elementPrototype('iframe'), 'srcdoc', 'HTMLIFrameElement', 'TrustedHTML');
  guardSetter(elementPrototype('object'), 'data', 'HTMLObjectElement', 'TrustedScriptURL');
  guardSetter(elementPrototype('object'), 'codeBase', 'HTMLObjectElement', 'TrustedScriptURL');
  guardSetter(elementPrototype('embed'), 'src', 'HTMLEmbedElement', 'TrustedScriptURL');
  guardAttribute('setAttribute', 0);
  guardAttribute('setAttributeNS', 1, 0);
  guardWorker();
  // A inserção de um `<meta>` no `<head>` liga o CSP dele na hora.
  guardInsertion(Node.prototype, ['appendChild', 'insertBefore', 'replaceChild'], (args) => [args[0]]);
  guardInsertion(Element.prototype, ['append', 'prepend', 'before', 'after', 'replaceWith', 'replaceChildren'], (args) => args);
  guardInsertion(Element.prototype, ['insertAdjacentElement'], (args) => [args[1]]);
  // Terminado o parse, todos os `<meta>` do documento já valem.
  document.addEventListener('DOMContentLoaded', () => { staticMetaComplete = true; refreshPolicies(); }, { capture: true });
  recomputePolicies();
};

// `new Worker(url)`: o endereço do script é `TrustedScriptURL`. A classe do motor segue a mesma, com o mesmo protótipo.
const guardWorker = () => {
  const descriptor = getOwn(globalThis, 'Worker');
  if (descriptor === undefined || typeof descriptor.value !== 'function') return;
  const Native = descriptor.value;
  const Guarded = function Worker(url, options) {
    if (new.target === undefined) throw new TypeError("Failed to construct 'Worker': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'Worker': 1 argument required, but only 0 present.");
    const address = enforceSink('Worker constructor', 'TrustedScriptURL', url, assignmentError('construct', 'Worker', 'Worker', 'TrustedScriptURL'));
    return Reflect.construct(Native, arguments.length > 1 ? [address, options] : [address], new.target === Guarded ? Native : new.target);
  };
  Object.defineProperty(Guarded, 'length', { value: Native.length });
  Object.defineProperty(Guarded, 'prototype', { value: Native.prototype, writable: false });
  Object.defineProperty(Native.prototype, 'constructor', { value: Guarded, writable: true, configurable: true });
  Object.defineProperty(globalThis, 'Worker', { ...descriptor, value: Guarded });
};

// `setAttribute` e `setAttributeNS`: o tipo do atributo vem do mapa do `trustedTypes`.
const attributeSink = (element, name, namespace, attributeNamespace) => {
  const type = trustedTypes.getAttributeType(element.localName, name, element.namespaceURI, attributeNamespace);
  if (type === null) return null;
  const generic = TRUSTED_TYPES_MAPPING['*'].attributes[lower(name)] !== undefined;
  return [`${generic ? 'Element' : element.constructor.name} ${lower(name)}`, type];
};
const guardAttribute = (name, nameIndex, namespaceIndex) => {
  const descriptor = getOwn(Element.prototype, name);
  const original = descriptor.value;
  const wrapper = { [name](...args) {
    if (requiresTrustedScript().length > 0 && args.length > nameIndex + 1) {
      const attribute = String(args[nameIndex]);
      const local = namespaceIndex === undefined ? attribute : attribute.slice(attribute.indexOf(':') + 1);
      const sink = attributeSink(this, local, namespaceIndex === undefined ? null : args[namespaceIndex]);
      if (sink !== null) args[nameIndex + 1] = enforceSink(sink[0], sink[1], args[nameIndex + 1], assignmentError('call', name, 'Element', sink[1]));
    }
    const result = withinSink('TrustedScript', () => original.apply(this, args));
    // Atributo de um `<meta>` que já está no `<head>`: o CSP é processado de novo.
    if (this.localName === 'meta') noteInserted(this);
    return result;
  } }[name];
  Object.defineProperty(wrapper, 'length', { value: original.length });
  Object.defineProperty(Element.prototype, name, { ...descriptor, value: wrapper });
};

// `createPolicy`: a diretiva `trusted-types` limita os nomes e as repetições.
const validPolicyName = (name) => /^[A-Za-z0-9\-#=_/@.%]+$/.test(name);
const cspCreatePolicyCheck = (name, duplicate) => {
  const policies = policiesFor('trusted-types');
  const position = policies.length > 0 ? callerPosition() : null;
  let failure = null;
  for (const policy of policies) {
    const tokens = policy.directives.get('trusted-types');
    const allowed = tokens.includes("'none'") ? false : tokens.includes('*') ? validPolicyName(name) : tokens.includes(name);
    let message = null;
    if (!allowed) message = `Policy "${name}" disallowed.`;
    else if (duplicate && !tokens.includes("'allow-duplicates'")) message = `Policy with name "${name}" already exists.`;
    if (message === null) continue;
    reportViolation(policy, 'trusted-types', 'trusted-types-policy', name, position);
    if (!policy.reportOnly && failure === null) failure = message;
  }
  return failure;
};
