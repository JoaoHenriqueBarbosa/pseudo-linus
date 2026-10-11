// ---- IndexedDB (`indexedDB`): bancos por origem que vivem enquanto a página vive, com as chaves, a ordem, os caminhos
// de chave, os geradores, os índices (únicos e de várias entradas), as transações (escopo, modos, ativação até o fim
// da tarefa, auto-commit, `commit`, `abort` com reversão), as requisições e os cursores, a abertura com
// `upgradeneeded`, `versionchange` e `blocked`, e as mensagens de erro do Blink, medidos no oráculo. O resultado de
// cada requisição chega numa tarefa própria, como a resposta do processo de armazenamento do Chromium.

// ---- O clone estruturado com a mensagem do Blink: o V8 nomeia o primeiro valor que não clona, na ordem em que o
// serializador o encontra (as propriedades próprias enumeráveis, em profundidade).
const uncloneableText = (value, seen = new Set()) => {
  if (typeof value === 'function') {
    try { return Function.prototype.toString.call(value); } catch { return 'function'; }
  }
  if (typeof value === 'symbol') return String(value);
  if (value === null || typeof value !== 'object' || seen.has(value)) return null;
  seen.add(value);
  if (value instanceof WeakMap || value instanceof WeakSet || value instanceof WeakRef || value instanceof Promise) return `#<${value.constructor.name}>`;
  const children = value instanceof Map ? [...value.keys(), ...value.values()] : value instanceof Set ? [...value] : Object.keys(value).map((key) => value[key]);
  for (const child of children) {
    const found = uncloneableText(child, seen);
    if (found !== null) return found;
  }
  return null;
};
const engineStructuredClone = structuredClone;
// `prefix` é o começo da mensagem do binding (`Failed to execute 'add' on 'IDBObjectStore': `).
const cloneFor = (value, prefix) => {
  try {
    return engineStructuredClone(value);
  } catch (error) {
    if (!(error instanceof DOMException) || error.name !== 'DataCloneError') throw error;
    const text = uncloneableText(value);
    throw domException(`${prefix}${text === null ? 'An object could not be cloned.' : `${text} could not be cloned.`}`, 'DataCloneError');
  }
};
const windowStructuredClone = { structuredClone(value, options) {
  if (arguments.length < 1) throw new TypeError("Failed to execute 'structuredClone' on 'Window': 1 argument required, but only 0 present.");
  if (options !== undefined && options !== null && options.transfer !== undefined) return engineStructuredClone(value, options);
  return cloneFor(value, "Failed to execute 'structuredClone' on 'Window': ");
} }.structuredClone;
Object.defineProperty(globalThis, 'structuredClone', { value: windowStructuredClone, writable: true, configurable: true, enumerable: true });

// ---- DOMStringList: os nomes dos stores e dos índices, em ordem, com acesso por índice.
class DOMStringList {
  constructor(key = undefined, items = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('DOMStringList');
    hidden(this, '__strings', items);
    items.forEach((item, index) => Object.defineProperty(this, index, { value: item, enumerable: true }));
  }
  get length() { receiver(this, P.__strings); return this[P.__strings].length; }
  contains(string) {
    receiver(this, P.__strings);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'contains' on 'DOMStringList': 1 argument required, but only 0 present.");
    return this[P.__strings].includes(String(string));
  }
  item(index) {
    receiver(this, P.__strings);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'item' on 'DOMStringList': 1 argument required, but only 0 present.");
    const value = this[P.__strings][index >>> 0];
    return value === undefined ? null : value;
  }
}
blinkInterface(DOMStringList, 'DOMStringList', ['constructor']);
Object.defineProperty(DOMStringList.prototype, Symbol.iterator, { value: Array.prototype.values, writable: true, configurable: true });
const stringList = (names) => new DOMStringList(ILLEGAL, [...names].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)));

// ---- Chaves: número < data < texto < binário < lista (a ordem do IndexedDB).
const KEY_NUMBER = 1;
const KEY_DATE = 2;
const KEY_STRING = 3;
const KEY_BINARY = 4;
const KEY_ARRAY = 5;
// O valor convertido em chave, ou `null` se não for chave válida.
const toKey = (value, seen = new Set()) => {
  if (typeof value === 'number') return Number.isNaN(value) ? null : { type: KEY_NUMBER, value };
  if (typeof value === 'string') return { type: KEY_STRING, value };
  if (value instanceof Date) {
    const time = value.getTime();
    return Number.isNaN(time) ? null : { type: KEY_DATE, value: time };
  }
  if (value instanceof ArrayBuffer) return { type: KEY_BINARY, value: new Uint8Array(value.slice(0)) };
  if (ArrayBuffer.isView(value)) return { type: KEY_BINARY, value: new Uint8Array(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength)) };
  if (Array.isArray(value)) {
    if (seen.has(value)) return null;
    seen.add(value);
    const items = [];
    for (let index = 0; index < value.length; index++) {
      if (!Object.prototype.hasOwnProperty.call(value, index)) return null;
      const item = toKey(value[index], seen);
      if (item === null) return null;
      items.push(item);
    }
    seen.delete(value);
    return { type: KEY_ARRAY, value: items };
  }
  return null;
};
const compareKeys = (a, b) => {
  if (a.type !== b.type) return a.type < b.type ? -1 : 1;
  if (a.type === KEY_BINARY || a.type === KEY_ARRAY) {
    const length = Math.min(a.value.length, b.value.length);
    for (let index = 0; index < length; index++) {
      const order = a.type === KEY_BINARY ? Math.sign(a.value[index] - b.value[index]) : compareKeys(a.value[index], b.value[index]);
      if (order !== 0) return order;
    }
    return Math.sign(a.value.length - b.value.length);
  }
  return a.value < b.value ? -1 : a.value > b.value ? 1 : 0;
};
const keyValue = (key) => {
  if (key.type === KEY_DATE) return new Date(key.value);
  if (key.type === KEY_BINARY) return key.value.slice().buffer;
  if (key.type === KEY_ARRAY) return key.value.map(keyValue);
  return key.value;
};
const failed = (operation, owner) => `Failed to execute '${operation}' on '${owner}': `;
const notValidKey = (operation, owner) => domException(`${failed(operation, owner)}The parameter is not a valid key.`, 'DataError');
const requireKey = (value, operation, owner) => {
  const key = toKey(value);
  if (key === null) throw notValidKey(operation, owner);
  return key;
};
const requireCount = (count, needed, operation, owner) => {
  if (count < needed) throw new TypeError(`${failed(operation, owner)}${needed} argument${needed === 1 ? '' : 's'} required, but only ${count} present.`);
};

// ---- Faixas de chaves.
const rangeIncludes = (range, key) => {
  if (range.lower !== null) {
    const order = compareKeys(range.lower, key);
    if (order > 0 || (order === 0 && range.lowerOpen)) return false;
  }
  if (range.upper !== null) {
    const order = compareKeys(range.upper, key);
    if (order < 0 || (order === 0 && range.upperOpen)) return false;
  }
  return true;
};
const UNBOUNDED = { lower: null, upper: null, lowerOpen: false, upperOpen: false };
class IDBKeyRange {
  constructor(key = undefined, range = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBKeyRange');
    hidden(this, '__keyRange', range);
  }
  get lower() { receiver(this, P.__keyRange); const key = this[P.__keyRange].lower; return key === null ? undefined : keyValue(key); }
  get upper() { receiver(this, P.__keyRange); const key = this[P.__keyRange].upper; return key === null ? undefined : keyValue(key); }
  get lowerOpen() { receiver(this, P.__keyRange); return this[P.__keyRange].lowerOpen; }
  get upperOpen() { receiver(this, P.__keyRange); return this[P.__keyRange].upperOpen; }
  includes(key) {
    receiver(this, P.__keyRange);
    requireCount(arguments.length, 1, 'includes', 'IDBKeyRange');
    return rangeIncludes(this[P.__keyRange], requireKey(key, 'includes', 'IDBKeyRange'));
  }
  static bound(lower, upper, lowerOpen = false, upperOpen = false) {
    requireCount(arguments.length, 2, 'bound', 'IDBKeyRange');
    const low = toKey(lower);
    const high = toKey(upper);
    if (low === null || high === null) throw notValidKey('bound', 'IDBKeyRange');
    const order = compareKeys(low, high);
    if (order > 0) throw domException(`${failed('bound', 'IDBKeyRange')}The lower key is greater than the upper key.`, 'DataError');
    if (order === 0 && (lowerOpen || upperOpen)) throw domException(`${failed('bound', 'IDBKeyRange')}The lower key and upper key are equal and one of the bounds is open.`, 'DataError');
    return new IDBKeyRange(ILLEGAL, { lower: low, upper: high, lowerOpen: !!lowerOpen, upperOpen: !!upperOpen });
  }
  static lowerBound(lower, open = false) {
    requireCount(arguments.length, 1, 'lowerBound', 'IDBKeyRange');
    return new IDBKeyRange(ILLEGAL, { lower: requireKey(lower, 'lowerBound', 'IDBKeyRange'), upper: null, lowerOpen: !!open, upperOpen: true });
  }
  static only(value) {
    requireCount(arguments.length, 1, 'only', 'IDBKeyRange');
    const key = requireKey(value, 'only', 'IDBKeyRange');
    return new IDBKeyRange(ILLEGAL, { lower: key, upper: key, lowerOpen: false, upperOpen: false });
  }
  static upperBound(upper, open = false) {
    requireCount(arguments.length, 1, 'upperBound', 'IDBKeyRange');
    return new IDBKeyRange(ILLEGAL, { lower: null, upper: requireKey(upper, 'upperBound', 'IDBKeyRange'), lowerOpen: true, upperOpen: !!open });
  }
}
blinkInterface(IDBKeyRange, 'IDBKeyRange', ['constructor']);
for (const name of ['bound', 'lowerBound', 'only', 'upperBound']) Object.defineProperty(IDBKeyRange, name, { enumerable: true });
// A consulta de uma operação: faixa, chave (vira faixa de um só) ou nada (tudo, quando permitido).
const queryRange = (query, operation, owner, required) => {
  if (query instanceof IDBKeyRange) return query[P.__keyRange];
  if (query === undefined || query === null) {
    if (required) throw domException(`${failed(operation, owner)}No key or key range specified.`, 'DataError');
    return UNBOUNDED;
  }
  const key = requireKey(query, operation, owner);
  return { lower: key, upper: key, lowerOpen: false, upperOpen: false };
};

// ---- Caminhos de chave.
const IDENTIFIER = /^[$_\p{ID_Start}][$‌‍\p{ID_Continue}]*$/u;
const validKeyPath = (path) => {
  if (Array.isArray(path)) return path.length > 0 && path.every((item) => typeof item === 'string' && validKeyPath(item));
  return typeof path === 'string' && (path === '' || path.split('.').every((part) => IDENTIFIER.test(part)));
};
const normalizeKeyPath = (path) => (Array.isArray(path) ? path.map(String) : path === null || path === undefined ? null : String(path));
const keyPathValue = (path) => (Array.isArray(path) ? path.slice() : path);
// Avalia o caminho no valor: `{ found, value }` (`found` falso quando um passo não existe).
const walkKeyPath = (value, path) => {
  if (path === '') return { found: true, value };
  let current = value;
  for (const part of path.split('.')) {
    if (typeof current === 'string' && part === 'length') { current = current.length; continue; }
    if (current === null || typeof current !== 'object' || !Object.prototype.hasOwnProperty.call(current, part)) return { found: false };
    current = current[part];
  }
  return { found: true, value: current };
};
// A chave que o caminho dá no valor: `undefined` sem valor, `null` com valor que não é chave.
const keyFromPath = (value, path) => {
  if (Array.isArray(path)) {
    const items = [];
    for (const part of path) {
      const key = keyFromPath(value, part);
      if (key === undefined || key === null) return key === undefined ? null : key;
      items.push(key);
    }
    return { type: KEY_ARRAY, value: items };
  }
  const walked = walkKeyPath(value, path);
  if (!walked.found || walked.value === undefined) return undefined;
  return toKey(walked.value);
};
// As chaves de um índice de várias entradas: as válidas da lista, sem repetição.
const multiEntryKeys = (value, path) => {
  const walked = walkKeyPath(value, path);
  if (!walked.found) return [];
  if (!Array.isArray(walked.value)) {
    const key = toKey(walked.value);
    return key === null ? [] : [key];
  }
  const keys = [];
  for (const item of walked.value) {
    const key = toKey(item);
    if (key !== null && !keys.some((other) => compareKeys(other, key) === 0)) keys.push(key);
  }
  return keys;
};
// Escreve a chave gerada no valor, criando os objetos do caminho.
const injectKey = (value, path, key) => {
  const parts = path.split('.');
  let current = value;
  for (const part of parts.slice(0, -1)) {
    if (!Object.prototype.hasOwnProperty.call(current, part)) current[part] = {};
    current = current[part];
  }
  current[parts[parts.length - 1]] = keyValue(key);
};
const canInjectKey = (value, path) => {
  let current = value;
  for (const part of path.split('.').slice(0, -1)) {
    if (current === null || typeof current !== 'object') return false;
    if (!Object.prototype.hasOwnProperty.call(current, part)) return true;
    current = current[part];
  }
  return current !== null && typeof current === 'object';
};

// ---- Registros ordenados: listas de `{ key, ... }` em ordem (no índice, por chave e depois chave primária).
const searchSorted = (list, compare) => {
  let low = 0;
  let high = list.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (compare(list[middle]) < 0) low = middle + 1;
    else high = middle;
  }
  return low;
};
const compareIndexEntries = (a, b) => compareKeys(a.key, b.key) || compareKeys(a.primaryKey, b.primaryKey);

// ---- O armazenamento: bancos da origem (`databases`), cada um com stores (`{ name, keyPath, autoIncrement,
// generator, records, indexes }`) e índices (`{ name, keyPath, unique, multiEntry, entries }`).
const databases = new Map();
// Os bancos vivem no perfil, fora do documento: o primeiro uso do IndexedDB no documento traz os da origem (cada um
// serializado em texto) e cada transação que escreve devolve o banco inteiro ao perfil.
let databasesLoaded = false;
const loadDatabases = () => {
  if (databasesLoaded) return;
  databasesLoaded = true;
  for (const [name, text] of op('idb', 'load')) {
    if (databases.has(name)) continue;
    const saved = deserializeValue(text);
    // O disco guarda as entradas dos índices com as remoções atrasadas do Chromium; a página vê as que os registros dão.
    for (const store of saved.stores.values()) {
      for (const index of store.indexes.values()) {
        index.entries = [];
        for (const record of store.records) addIndexEntries(index, record.key, record.value);
      }
    }
    databases.set(name, { name, version: saved.version, stores: saved.stores, connections: new Set(), transactions: [], waiting: null, creating: false });
  }
};
// Cada transação que escreve devolve ao perfil o diário das mutações, na ordem em que as requisições rodaram: é o
// perfil que sabe os lotes que o Chromium grava para cada uma (o banco em memória continua sendo a fonte da página).
const commitJournal = (state) => op('idb', 'commit', state.database.name, serializeValue({
  mode: state.mode,
  version: state.database.version,
  scope: [...state.scope],
  journal: state.journal,
}));
const MAX_GENERATED_KEY = 2 ** 53;
const findRecord = (store, key) => {
  const position = searchSorted(store.records, (record) => compareKeys(record.key, key));
  return position < store.records.length && compareKeys(store.records[position].key, key) === 0 ? position : -1;
};
const indexKeysOf = (index, value) => {
  if (index.multiEntry) return multiEntryKeys(value, index.keyPath);
  const key = keyFromPath(value, index.keyPath);
  return key === undefined || key === null ? [] : [key];
};
const addIndexEntries = (index, primaryKey, value) => {
  for (const key of indexKeysOf(index, value)) {
    const entry = { key, primaryKey };
    index.entries.splice(searchSorted(index.entries, (other) => compareIndexEntries(other, entry)), 0, entry);
  }
};
const removeRecordAt = (store, position) => {
  const [record] = store.records.splice(position, 1);
  for (const index of store.indexes.values()) index.entries = index.entries.filter((entry) => compareKeys(entry.primaryKey, record.key) !== 0);
};
const constraintError = (message) => domException(message, 'ConstraintError');
// "Store a record into an object store": a chave gerada, a unicidade dos índices e a troca do registro. Devolve a
// chave, ou lança o erro que vira o `error` da requisição.
const storeRecord = (store, value, key, noOverwrite, journal) => {
  const generated = store.autoIncrement && key === undefined;
  if (generated) {
    if (store.generator > MAX_GENERATED_KEY) throw constraintError('Key generator overflow.');
    key = { type: KEY_NUMBER, value: store.generator++ };
    if (store.keyPath !== null) injectKey(value, store.keyPath, key);
  } else if (store.autoIncrement && key.type === KEY_NUMBER && key.value >= store.generator) {
    store.generator = Math.min(Math.floor(key.value) + 1, MAX_GENERATED_KEY + 1);
  }
  const existing = findRecord(store, key);
  if (existing !== -1 && noOverwrite) throw constraintError('Key already exists in the object store.');
  for (const index of store.indexes.values()) {
    if (!index.unique) continue;
    for (const indexKey of indexKeysOf(index, value)) {
      const clash = index.entries.some((entry) => compareKeys(entry.key, indexKey) === 0 && compareKeys(entry.primaryKey, key) !== 0);
      if (clash) throw constraintError(`Unable to add key to index '${index.name}': at least one key does not satisfy the uniqueness requirements.`);
    }
  }
  if (existing !== -1) removeRecordAt(store, existing);
  store.records.splice(searchSorted(store.records, (record) => compareKeys(record.key, key)), 0, { key, value });
  for (const index of store.indexes.values()) addIndexEntries(index, key, value);
  journal.push(['put', store.name, key, value, [...store.indexes.values()].map((index) => [index.name, indexKeysOf(index, value)]), store.generator, generated]);
  return key;
};
const deleteRange = (store, range, journal) => {
  const removed = [];
  for (let position = store.records.length - 1; position >= 0; position--) {
    if (!rangeIncludes(range, store.records[position].key)) continue;
    removed.unshift(store.records[position].key);
    removeRecordAt(store, position);
  }
  for (const key of removed) journal.push(['delete', store.name, key]);
};
// Cópia para a reversão do `abort`: os registros, o gerador e as entradas dos índices de um store.
const snapshotStore = (store) => ({ store, records: store.records.slice(), generator: store.generator, entries: new Map([...store.indexes].map(([name, index]) => [name, index.entries.slice()])) });
const restoreStore = (snapshot) => {
  snapshot.store.records = snapshot.records;
  snapshot.store.generator = snapshot.generator;
  for (const [name, entries] of snapshot.entries) {
    const index = snapshot.store.indexes.get(name);
    if (index !== undefined) index.entries = entries;
  }
};
// O esquema inteiro, para reverter o `versionchange`.
const snapshotSchema = (database) => ({
  version: database.version,
  stores: new Map([...database.stores].map(([name, store]) => [name, { store, name: store.name, indexes: new Map(store.indexes), snapshot: snapshotStore(store) }])),
});
const restoreSchema = (database, schema) => {
  database.version = schema.version;
  database.stores = new Map([...schema.stores].map(([name, saved]) => {
    saved.store.name = saved.name;
    saved.store.indexes = saved.indexes;
    restoreStore(saved.snapshot);
    return [name, saved.store];
  }));
};

// ---- Transações e requisições.
const TRANSACTION_FINISHED = 'The transaction has finished.';
const TRANSACTION_INACTIVE = 'The transaction is not active.';
const ABORTED_REQUEST = 'The transaction was aborted, so the request cannot be fulfilled.';
const stateOf = (transaction) => transaction[P.__idbTransaction];
// A validação comum das operações de store, índice e cursor, na ordem do Blink.
const checkUsable = (transaction, operation, owner, write) => {
  const state = stateOf(transaction);
  if (state.phase === 'finished' || state.phase === 'committing') throw domException(failed(operation, owner) + TRANSACTION_FINISHED, 'TransactionInactiveError');
  if (state.phase !== 'active') throw domException(failed(operation, owner) + TRANSACTION_INACTIVE, 'TransactionInactiveError');
  if (write && state.mode === 'readonly') throw domException(`${failed(operation, owner)}The transaction is read-only.`, 'ReadOnlyError');
};
const touchStore = (state, store) => {
  if (state.mode === 'versionchange' || state.snapshots.has(store)) return;
  state.snapshots.set(store, snapshotStore(store));
};
// Pode começar: nenhuma transação anterior e ainda aberta do mesmo banco divide um store com ela, salvo duas de leitura.
const canStart = (state) => {
  for (const other of state.database.transactions) {
    if (other === state) return true;
    if (other.phase === 'finished') continue;
    if (state.mode === 'readonly' && other.mode === 'readonly') continue;
    if (other.mode === 'versionchange' || other.scope.some((name) => state.scope.includes(name))) return false;
  }
  return true;
};
const startWaiting = (database) => {
  for (const state of database.transactions) {
    if (!state.started && state.phase !== 'finished' && canStart(state)) {
      state.started = true;
      schedulePump(state);
    }
  }
};
// A ida e volta ao processo de armazenamento: o resultado chega depois das tarefas que a página agendou com
// `setTimeout(0)` na mesma volta (medido: a transação ainda está viva e inativa nelas). O timer do motor leva o atraso
// 0 a 1 ms, então a resposta vem a 2 ms.
const STORAGE_ROUNDTRIP_MS = 2;
const schedulePump = (state) => {
  if (!state.started || state.pumping || state.phase === 'finished') return;
  if (state.queue.length === 0) {
    maybeCommit(state);
    return;
  }
  state.pumping = true;
  setTimeout(() => {
    state.pumping = false;
    if (state.phase === 'finished' || state.queue.length === 0) return;
    const { request, run } = state.queue.shift();
    let result;
    let error = null;
    try { result = run(); } catch (thrown) { error = thrown; }
    completeRequest(state, request, result, error);
  }, STORAGE_ROUNDTRIP_MS);
};
// Entrega o resultado: o evento `success` (ou `error`, que sobe à transação e ao banco) com a transação ativa.
const completeRequest = (state, request, result, error) => {
  const fields = request[P.__idbRequest];
  fields.done = true;
  fields.result = error === null ? result : undefined;
  fields.error = error;
  const before = listenerFailures;
  const previous = state.phase;
  if (state.phase === 'inactive') state.phase = 'active';
  const event = new Event(error === null ? 'success' : 'error', { bubbles: error !== null, cancelable: error !== null });
  event.isTrusted = true;
  dispatchFromTask(request, event);
  if (state.phase === 'active') state.phase = previous === 'committing' ? 'committing' : 'inactive';
  if (state.phase === 'finished') return;
  if (listenerFailures !== before) {
    abortTransaction(state, domException('Uncaught exception in event handler.', 'AbortError'));
    return;
  }
  if (error !== null && !event.defaultPrevented) {
    abortTransaction(state, error);
    return;
  }
  schedulePump(state);
};
const maybeCommit = (state) => {
  if ((state.phase !== 'inactive' && state.phase !== 'committing') || state.queue.length > 0 || state.pumping || !state.started) return;
  state.phase = 'committing';
  state.pumping = true;
  setTimeout(() => {
    state.pumping = false;
    if (state.phase === 'finished') return;
    if (state.queue.length > 0) {
      schedulePump(state);
      return;
    }
    if (state.mode !== 'readonly') commitJournal(state);
    finishTransaction(state);
    const event = new Event('complete');
    event.isTrusted = true;
    dispatchFromTask(state.transaction, event);
    if (state.onFinish) state.onFinish(null);
  }, 0);
};
const finishTransaction = (state) => {
  state.phase = 'finished';
  state.database.transactions = state.database.transactions.filter((other) => other !== state);
  state.connection.transactions.delete(state);
  startWaiting(state.database);
  closeIfDone(state.connection);
};
// O `abort`: reverte as escritas, falha as requisições pendentes e dispara `abort` (que sobe ao banco) numa tarefa.
const abortTransaction = (state, error) => {
  if (state.phase === 'finished') return;
  const pending = state.queue.splice(0);
  state.error = error;
  if (state.mode === 'versionchange') restoreSchema(state.database, state.schema);
  else for (const snapshot of state.snapshots.values()) restoreStore(snapshot);
  finishTransaction(state);
  setTimeout(() => {
    for (const { request } of pending) {
      const fields = request[P.__idbRequest];
      fields.done = true;
      fields.result = undefined;
      fields.error = domException(ABORTED_REQUEST, 'AbortError');
      const event = new Event('error', { bubbles: true, cancelable: true });
      event.isTrusted = true;
      dispatchFromTask(request, event);
    }
    const event = new Event('abort', { bubbles: true });
    event.isTrusted = true;
    dispatchFromTask(state.transaction, event);
    if (state.onFinish) state.onFinish(error === null ? domException('The transaction was aborted.', 'AbortError') : error);
  }, 0);
};
// Uma requisição de `source` na transação: `run` executa a operação na vez dela e devolve o resultado (ou lança o
// erro da requisição). `request` reaproveita a do cursor.
const queueRequest = (transaction, source, run, request = undefined) => {
  const state = stateOf(transaction);
  if (request === undefined) {
    request = new IDBRequest(ILLEGAL);
    request[P.__idbRequest].source = source;
    request[P.__idbRequest].transaction = transaction;
    eventParents.set(request, transaction);
  } else {
    request[P.__idbRequest].done = false;
  }
  state.queue.push({ request, run });
  schedulePump(state);
  return request;
};

// ---- Leitura com faixa, contagem e direção: as entradas que uma consulta vê (registros do store ou do índice).
const DIRECTIONS = ['next', 'nextunique', 'prev', 'prevunique'];
const readDirection = (value, operation, owner) => {
  const direction = value === undefined ? 'next' : String(value);
  if (!DIRECTIONS.includes(direction)) throw new TypeError(`${failed(operation, owner)}The provided value '${direction}' is not a valid enum value of type IDBCursorDirection.`);
  return direction;
};
const readCount = (value, operation, owner) => {
  if (value === undefined) return 0;
  const number = Number(value);
  if (!Number.isFinite(number)) throw new TypeError(`${failed(operation, owner)}Value is not of type 'unsigned long'.`);
  const integer = Math.trunc(number);
  if (integer < 0 || integer > 0xFFFFFFFF) throw new TypeError(`${failed(operation, owner)}Value is outside the 'unsigned long' value range.`);
  return integer;
};
// As entradas do alvo (`{ key, primaryKey, value }`) dentro da faixa, na direção pedida e até `count` (0 é tudo).
const selectEntries = (target, range, count, direction = 'next') => {
  const store = target.store;
  let entries = target.index === null
    ? store.records.filter((record) => rangeIncludes(range, record.key)).map((record) => ({ key: record.key, primaryKey: record.key, value: record.value }))
    : target.index.entries.filter((entry) => rangeIncludes(range, entry.key)).map((entry) => ({ key: entry.key, primaryKey: entry.primaryKey, value: store.records[findRecord(store, entry.primaryKey)].value }));
  if (direction === 'nextunique') entries = entries.filter((entry, position) => position === 0 || compareKeys(entries[position - 1].key, entry.key) !== 0);
  if (direction === 'prev' || direction === 'prevunique') {
    if (direction === 'prevunique') entries = entries.filter((entry, position) => position === 0 || compareKeys(entries[position - 1].key, entry.key) !== 0);
    entries.reverse();
  }
  return count === 0 ? entries : entries.slice(0, count);
};
const readValue = (value) => engineStructuredClone(value);

class IDBRecord {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBRecord');
    hidden(this, '__idbRecord', fields);
  }
  get key() { receiver(this, P.__idbRecord); return keyValue(this[P.__idbRecord].key); }
  get primaryKey() { receiver(this, P.__idbRecord); return keyValue(this[P.__idbRecord].primaryKey); }
  get value() { receiver(this, P.__idbRecord); return this[P.__idbRecord].value; }
}
blinkInterface(IDBRecord, 'IDBRecord', ['constructor']);

// ---- Requisições.
class IDBRequest extends EventTarget {
  constructor(key = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBRequest');
    super();
    hidden(this, '__idbRequest', { done: false, result: undefined, error: null, source: null, transaction: null });
  }
  get result() {
    receiver(this, P.__idbRequest);
    if (!this[P.__idbRequest].done) throw domException("Failed to read the 'result' property from 'IDBRequest': The request has not finished.", 'InvalidStateError');
    return this[P.__idbRequest].result;
  }
  get error() {
    receiver(this, P.__idbRequest);
    if (!this[P.__idbRequest].done) throw domException("Failed to read the 'error' property from 'IDBRequest': The request has not finished.", 'InvalidStateError');
    return this[P.__idbRequest].error;
  }
  get source() { receiver(this, P.__idbRequest); return this[P.__idbRequest].source; }
  get transaction() { receiver(this, P.__idbRequest); return this[P.__idbRequest].transaction; }
  get readyState() { receiver(this, P.__idbRequest); return this[P.__idbRequest].done ? 'done' : 'pending'; }
}
defineHandlerProperties(IDBRequest.prototype, ['success', 'error']);
blinkInterface(IDBRequest, 'IDBRequest', ['constructor']);

class IDBOpenDBRequest extends IDBRequest {
  constructor(key = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBOpenDBRequest');
    super(key);
  }
}
defineHandlerProperties(IDBOpenDBRequest.prototype, ['blocked', 'upgradeneeded']);
blinkInterface(IDBOpenDBRequest, 'IDBOpenDBRequest', ['constructor']);

class IDBVersionChangeEvent extends Event {
  constructor(type, init = {}) {
    if (arguments.length < 1) throw new TypeError("Failed to construct 'IDBVersionChangeEvent': 1 argument required, but only 0 present.");
    super(type, init);
    const options = init === null ? {} : init;
    hidden(this, '__versionChange', {
      oldVersion: options.oldVersion === undefined ? 0 : Number(options.oldVersion), newVersion: options.newVersion === undefined || options.newVersion === null ? null : Number(options.newVersion),
      dataLoss: options.dataLoss === undefined ? 'none' : String(options.dataLoss), dataLossMessage: options.dataLossMessage === undefined ? '' : String(options.dataLossMessage),
    });
  }
  get oldVersion() { receiver(this, P.__versionChange); return this[P.__versionChange].oldVersion; }
  get newVersion() { receiver(this, P.__versionChange); return this[P.__versionChange].newVersion; }
  get dataLoss() { receiver(this, P.__versionChange); return this[P.__versionChange].dataLoss; }
  get dataLossMessage() { receiver(this, P.__versionChange); return this[P.__versionChange].dataLossMessage; }
}
blinkInterface(IDBVersionChangeEvent, 'IDBVersionChangeEvent', ['constructor']);
const fireVersionChange = (target, type, oldVersion, newVersion) => {
  const event = new IDBVersionChangeEvent(type, { oldVersion, newVersion });
  event.isTrusted = true;
  dispatchFromTask(target, event);
  return event;
};

// ---- Cursores.
class IDBCursor {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBCursor');
    hidden(this, '__idbCursor', fields);
  }
  get source() { receiver(this, P.__idbCursor); return this[P.__idbCursor].source; }
  get direction() { receiver(this, P.__idbCursor); return this[P.__idbCursor].direction; }
  get key() { receiver(this, P.__idbCursor); const key = this[P.__idbCursor].key; return key === null ? undefined : keyValue(key); }
  get primaryKey() { receiver(this, P.__idbCursor); const key = this[P.__idbCursor].primaryKey; return key === null ? undefined : keyValue(key); }
  get request() { receiver(this, P.__idbCursor); return this[P.__idbCursor].request; }
  advance(count) {
    receiver(this, P.__idbCursor);
    requireCount(arguments.length, 1, 'advance', 'IDBCursor');
    const steps = readCount(count, 'advance', 'IDBCursor');
    if (steps === 0) throw new TypeError(`${failed('advance', 'IDBCursor')}A count argument with value 0 (zero) was supplied, must be greater than 0.`);
    iterateCursor(this, 'advance', steps, null, null);
  }
  continue(key = undefined) {
    receiver(this, P.__idbCursor);
    iterateCursor(this, 'continue', 1, key === undefined ? null : requireKey(key, 'continue', 'IDBCursor'), null);
  }
  continuePrimaryKey(key, primaryKey) {
    receiver(this, P.__idbCursor);
    requireCount(arguments.length, 2, 'continuePrimaryKey', 'IDBCursor');
    const cursor = this[P.__idbCursor];
    const target = requireKey(key, 'continuePrimaryKey', 'IDBCursor');
    const primary = requireKey(primaryKey, 'continuePrimaryKey', 'IDBCursor');
    if (cursor.target.index === null) throw domException(`${failed('continuePrimaryKey', 'IDBCursor')}The cursor's source is not an index.`, 'InvalidAccessError');
    if (cursor.direction === 'nextunique' || cursor.direction === 'prevunique') throw domException(`${failed('continuePrimaryKey', 'IDBCursor')}The cursor's direction is not 'next' or 'prev'.`, 'InvalidAccessError');
    iterateCursor(this, 'continuePrimaryKey', 1, target, primary);
  }
  delete() {
    receiver(this, P.__idbCursor);
    const cursor = this[P.__idbCursor];
    checkCursorWrite(this, 'delete', 'deleted');
    const primaryKey = cursor.primaryKey;
    const store = cursor.target.store;
    return queueRequest(cursor.transaction, this, () => {
      const position = findRecord(store, primaryKey);
      if (position !== -1) {
        removeRecordAt(store, position);
        stateOf(cursor.transaction).journal.push(['delete', store.name, primaryKey]);
      }
      return undefined;
    });
  }
  update(value) {
    receiver(this, P.__idbCursor);
    requireCount(arguments.length, 1, 'update', 'IDBCursor');
    const cursor = this[P.__idbCursor];
    checkCursorWrite(this, 'update', 'updated');
    const store = cursor.target.store;
    const clone = cloneFor(value, failed('update', 'IDBCursor'));
    if (store.keyPath !== null) {
      const key = keyFromPath(clone, store.keyPath);
      if (key === undefined || key === null || compareKeys(key, cursor.primaryKey) !== 0) {
        throw domException(`${failed('update', 'IDBCursor')}The effective object store of this cursor uses in-line keys and evaluating the key path of the value parameter results in a different value than the cursor's effective key.`, 'DataError');
      }
    }
    const primaryKey = cursor.primaryKey;
    return queueRequest(cursor.transaction, this, () => keyValue(storeRecord(store, clone, primaryKey, false, stateOf(cursor.transaction).journal)));
  }
}
blinkInterface(IDBCursor, 'IDBCursor', ['constructor']);
class IDBCursorWithValue extends IDBCursor {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBCursorWithValue');
    super(key, fields);
  }
  get value() { receiver(this, P.__idbCursor); return this[P.__idbCursor].value; }
}
blinkInterface(IDBCursorWithValue, 'IDBCursorWithValue', ['constructor']);
const checkCursorWrite = (cursor, operation, verb) => {
  const fields = cursor[P.__idbCursor];
  checkUsable(fields.transaction, operation, 'IDBCursor', false);
  if (stateOf(fields.transaction).mode === 'readonly') throw domException(`${failed(operation, 'IDBCursor')}The record may not be ${verb} inside a read-only transaction.`, 'ReadOnlyError');
  if (!fields.gotValue || fields.keyOnly) throw domException(`${failed(operation, 'IDBCursor')}The cursor is being iterated or has iterated past its end.`, 'InvalidStateError');
};
// A posição seguinte do cursor (o "iterate a cursor" da especificação): `count` passos, com a chave (e a chave
// primária) mínima pedida por `continue`/`continuePrimaryKey`.
const nextPosition = (cursor, count, key, primaryKey) => {
  const forward = cursor.direction === 'next' || cursor.direction === 'nextunique';
  const unique = cursor.direction === 'nextunique' || cursor.direction === 'prevunique';
  const entries = selectEntries(cursor.target, cursor.range, 0, 'next');
  let found = null;
  let position = cursor.key;
  let objectPosition = cursor.primaryKey;
  for (let step = 0; step < count; step++) {
    const candidates = entries.filter((entry) => {
      if (forward) {
        if (key !== null && step === 0) {
          const order = compareKeys(entry.key, key);
          if (order < 0) return false;
          if (primaryKey !== null && order === 0 && compareKeys(entry.primaryKey, primaryKey) < 0) return false;
        }
        if (position === null) return true;
        const order = compareKeys(entry.key, position);
        if (unique || cursor.target.index === null) return order > 0;
        return order > 0 || (order === 0 && compareKeys(entry.primaryKey, objectPosition) > 0);
      }
      if (key !== null && step === 0) {
        const order = compareKeys(entry.key, key);
        if (order > 0) return false;
        if (primaryKey !== null && order === 0 && compareKeys(entry.primaryKey, primaryKey) > 0) return false;
      }
      if (position === null) return true;
      const order = compareKeys(entry.key, position);
      if (unique || cursor.target.index === null) return order < 0;
      return order < 0 || (order === 0 && compareKeys(entry.primaryKey, objectPosition) < 0);
    });
    if (candidates.length === 0) return null;
    if (forward) found = candidates[0];
    else if (unique) {
      // `prevunique`: a maior chave, e nela o primeiro registro (o de menor chave primária).
      const last = candidates[candidates.length - 1];
      found = candidates.find((entry) => compareKeys(entry.key, last.key) === 0);
    } else found = candidates[candidates.length - 1];
    position = found.key;
    objectPosition = found.primaryKey;
  }
  return found;
};
const iterateCursor = (cursorObject, operation, count, key, primaryKey) => {
  const cursor = cursorObject[P.__idbCursor];
  checkUsable(cursor.transaction, operation, 'IDBCursor', false);
  if (cursor.target.deleted()) throw domException(`${failed(operation, 'IDBCursor')}The cursor's source or effective object store has been deleted.`, 'InvalidStateError');
  if (!cursor.gotValue) throw domException(`${failed(operation, 'IDBCursor')}The cursor is being iterated or has iterated past its end.`, 'InvalidStateError');
  if (key !== null && cursor.key !== null) {
    const forward = cursor.direction === 'next' || cursor.direction === 'nextunique';
    const order = compareKeys(key, cursor.key);
    const behind = forward ? order < 0 || (order === 0 && (primaryKey === null || compareKeys(primaryKey, cursor.primaryKey) <= 0)) : order > 0 || (order === 0 && (primaryKey === null || compareKeys(primaryKey, cursor.primaryKey) >= 0));
    if (behind) throw domException(`${failed(operation, 'IDBCursor')}The parameter is ${forward ? 'less' : 'greater'} than or equal to this cursor's position.`, 'DataError');
  }
  cursor.gotValue = false;
  queueRequest(cursor.transaction, cursor.source, () => advanceCursor(cursorObject, count, key, primaryKey), cursor.request);
};
// Move o cursor e devolve o resultado da requisição: o próprio cursor, ou `null` no fim.
const advanceCursor = (cursorObject, count, key, primaryKey) => {
  const cursor = cursorObject[P.__idbCursor];
  const found = nextPosition(cursor, count, key, primaryKey);
  if (found === null) {
    cursor.key = null;
    cursor.primaryKey = null;
    cursor.value = undefined;
    return null;
  }
  cursor.key = found.key;
  cursor.primaryKey = found.primaryKey;
  cursor.value = cursor.keyOnly ? undefined : readValue(found.value);
  cursor.gotValue = true;
  return cursorObject;
};
const openCursor = (transaction, source, target, query, direction, keyOnly, owner, operation) => {
  checkUsable(transaction, operation, owner, false);
  const range = queryRange(query, operation, owner, false);
  const cursorDirection = readDirection(direction, operation, owner);
  const fields = { source, target, transaction, range, direction: cursorDirection, keyOnly, key: null, primaryKey: null, value: undefined, gotValue: false, request: null };
  const cursor = keyOnly ? new IDBCursor(ILLEGAL, fields) : new IDBCursorWithValue(ILLEGAL, fields);
  const request = queueRequest(transaction, source, () => advanceCursor(cursor, 1, null, null));
  fields.request = request;
  return request;
};

// ---- As leituras comuns a store e índice (`target` é `{ store, index, deleted }`). `targetOf(self, operation)` lança
// se o store ou o índice foi apagado.
const readOperations = (owner, targetOf, transactionOf) => {
  const begin = (self, operation) => {
    const target = targetOf(self, operation);
    checkUsable(transactionOf(self), operation, owner, false);
    return target;
  };
  return {
    count(query) {
      const target = begin(this, 'count');
      const range = queryRange(query, 'count', owner, false);
      return queueRequest(transactionOf(this), this, () => selectEntries(target, range, 0).length);
    },
    get(query) {
      requireCount(arguments.length, 1, 'get', owner);
      const target = begin(this, 'get');
      const range = queryRange(query, 'get', owner, true);
      return queueRequest(transactionOf(this), this, () => {
        const [entry] = selectEntries(target, range, 1);
        return entry === undefined ? undefined : readValue(entry.value);
      });
    },
    getAll(query, count) {
      const target = begin(this, 'getAll');
      const range = queryRange(query, 'getAll', owner, false);
      const limit = readCount(count, 'getAll', owner);
      return queueRequest(transactionOf(this), this, () => selectEntries(target, range, limit).map((entry) => readValue(entry.value)));
    },
    getAllKeys(query, count) {
      const target = begin(this, 'getAllKeys');
      const range = queryRange(query, 'getAllKeys', owner, false);
      const limit = readCount(count, 'getAllKeys', owner);
      return queueRequest(transactionOf(this), this, () => selectEntries(target, range, limit).map((entry) => keyValue(entry.primaryKey)));
    },
    getAllRecords(options = {}) {
      const target = begin(this, 'getAllRecords');
      const opts = options === null ? {} : options;
      const range = queryRange(opts.query, 'getAllRecords', owner, false);
      const limit = readCount(opts.count, 'getAllRecords', owner);
      const direction = readDirection(opts.direction, 'getAllRecords', owner);
      return queueRequest(transactionOf(this), this, () => selectEntries(target, range, limit, direction).map((entry) => new IDBRecord(ILLEGAL, { key: entry.key, primaryKey: entry.primaryKey, value: readValue(entry.value) })));
    },
    getKey(query) {
      requireCount(arguments.length, 1, 'getKey', owner);
      const target = begin(this, 'getKey');
      const range = queryRange(query, 'getKey', owner, true);
      return queueRequest(transactionOf(this), this, () => {
        const [entry] = selectEntries(target, range, 1);
        return entry === undefined ? undefined : keyValue(entry.primaryKey);
      });
    },
    openCursor(query, direction) {
      return openCursor(transactionOf(this), this, targetOf(this, 'openCursor'), query, direction, false, owner, 'openCursor');
    },
    openKeyCursor(query, direction) {
      return openCursor(transactionOf(this), this, targetOf(this, 'openKeyCursor'), query, direction, true, owner, 'openKeyCursor');
    },
  };
};
// Põe as operações no protótipo na ordem e com a aridade do WebIDL.
const installOperations = (prototype, operations, arities) => {
  for (const [name, length] of Object.entries(arities)) {
    Object.defineProperty(operations[name], 'length', { value: length });
    Object.defineProperty(prototype, name, { value: operations[name], writable: true, enumerable: true, configurable: true });
  }
};
const READ_ARITIES = { count: 0, get: 1, getAll: 0, getAllKeys: 0, getAllRecords: 0, getKey: 1, openCursor: 0, openKeyCursor: 0 };
const notVersionChange = (operation, owner) => domException(`${failed(operation, owner)}The database is not running a version change transaction.`, 'InvalidStateError');
const storeDeleted = (operation) => domException(`${failed(operation, 'IDBObjectStore')}The object store has been deleted.`, 'InvalidStateError');

// ---- Stores.
class IDBObjectStore {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBObjectStore');
    hidden(this, '__idbStore', fields);
  }
  get name() { receiver(this, P.__idbStore); return this[P.__idbStore].store.name; }
  set name(value) {
    receiver(this, P.__idbStore);
    const { store, transaction } = this[P.__idbStore];
    const state = stateOf(transaction);
    const name = String(value);
    if (state.mode !== 'versionchange') throw notVersionChange('name', 'IDBObjectStore');
    if (state.phase !== 'active') throw domException(`Failed to set the 'name' property on 'IDBObjectStore': ${TRANSACTION_INACTIVE}`, 'TransactionInactiveError');
    if (name === store.name) return;
    if (state.database.stores.has(name)) throw domException("Failed to set the 'name' property on 'IDBObjectStore': An object store with the specified name already exists.", 'ConstraintError');
    state.database.stores.delete(store.name);
    state.journal.push(['renameStore', store.name, name]);
    store.name = name;
    state.database.stores.set(name, store);
  }
  get keyPath() { receiver(this, P.__idbStore); return keyPathValue(this[P.__idbStore].store.keyPath); }
  get indexNames() { receiver(this, P.__idbStore); return stringList(this[P.__idbStore].store.indexes.keys()); }
  get transaction() { receiver(this, P.__idbStore); return this[P.__idbStore].transaction; }
  get autoIncrement() { receiver(this, P.__idbStore); return this[P.__idbStore].store.autoIncrement; }
}
const storeFields = (self) => {
  receiver(self, P.__idbStore);
  return self[P.__idbStore];
};
const storeTarget = (self, operation) => {
  const fields = storeFields(self);
  if (fields.store.deleted) throw storeDeleted(operation);
  return { store: fields.store, index: null, deleted: () => fields.store.deleted };
};
const storeTransaction = (self) => storeFields(self).transaction;
// `add` e `put`: as validações na ordem do Blink, o clone e a chave do caminho, e a escrita na vez da requisição.
const writeRecord = (self, operation, value, key, noOverwrite) => {
  const { store, transaction } = storeFields(self);
  if (store.deleted) throw storeDeleted(operation);
  checkUsable(transaction, operation, 'IDBObjectStore', true);
  const prefix = failed(operation, 'IDBObjectStore');
  if (store.keyPath !== null && key !== undefined) throw domException(`${prefix}The object store uses in-line keys and the key parameter was provided.`, 'DataError');
  if (store.keyPath === null && !store.autoIncrement && key === undefined) throw domException(`${prefix}The object store uses out-of-line keys and has no key generator and the key parameter was not provided.`, 'DataError');
  let recordKey;
  if (key !== undefined) recordKey = requireKey(key, operation, 'IDBObjectStore');
  const clone = cloneFor(value, prefix);
  if (store.keyPath !== null) {
    const pathKey = keyFromPath(clone, store.keyPath);
    if (pathKey === null) throw domException(`${prefix}Evaluating the object store's key path yielded a value that is not a valid key.`, 'DataError');
    if (pathKey === undefined) {
      if (!store.autoIncrement) throw domException(`${prefix}Evaluating the object store's key path did not yield a value.`, 'DataError');
      if (!canInjectKey(clone, store.keyPath)) throw domException(`${prefix}A generated key could not be inserted into the value.`, 'DataError');
    } else {
      recordKey = pathKey;
    }
  }
  const state = stateOf(transaction);
  return queueRequest(transaction, self, () => {
    touchStore(state, store);
    return keyValue(storeRecord(store, clone, recordKey, noOverwrite, state.journal));
  });
};
const storeOperations = {
  ...readOperations('IDBObjectStore', storeTarget, storeTransaction),
  add(value, key) {
    requireCount(arguments.length, 1, 'add', 'IDBObjectStore');
    return writeRecord(this, 'add', value, key, true);
  },
  clear() {
    const { store, transaction } = storeFields(this);
    if (store.deleted) throw storeDeleted('clear');
    checkUsable(transaction, 'clear', 'IDBObjectStore', true);
    const state = stateOf(transaction);
    return queueRequest(transaction, this, () => {
      touchStore(state, store);
      store.records = [];
      for (const index of store.indexes.values()) index.entries = [];
      state.journal.push(['clear', store.name]);
      return undefined;
    });
  },
  createIndex(name, keyPath, options = {}) {
    requireCount(arguments.length, 2, 'createIndex', 'IDBObjectStore');
    const { store, transaction } = storeFields(this);
    const state = stateOf(transaction);
    const prefix = failed('createIndex', 'IDBObjectStore');
    if (state.mode !== 'versionchange') throw notVersionChange('createIndex', 'IDBObjectStore');
    if (store.deleted) throw storeDeleted('createIndex');
    if (state.phase !== 'active') throw domException(prefix + TRANSACTION_INACTIVE, 'TransactionInactiveError');
    const indexName = String(name);
    if (store.indexes.has(indexName)) throw domException(`${prefix}An index with the specified name already exists.`, 'ConstraintError');
    const path = normalizeKeyPath(keyPath);
    if (!validKeyPath(path)) throw domException(`${prefix}The keyPath argument contains an invalid key path.`, 'SyntaxError');
    const opts = options === null ? {} : options;
    const multiEntry = !!opts.multiEntry;
    if (multiEntry && Array.isArray(path)) throw domException(`${prefix}The keyPath argument was an array and the multiEntry option is true.`, 'InvalidAccessError');
    const index = { name: indexName, keyPath: path, unique: !!opts.unique, multiEntry, entries: [] };
    for (const record of store.records) addIndexEntries(index, record.key, record.value);
    store.indexes.set(indexName, index);
    state.journal.push(['createIndex', store.name, { name: indexName, keyPath: path, unique: index.unique, multiEntry }, store.records.map((record) => [record.key, indexKeysOf(index, record.value)])]);
    if (index.unique && index.entries.some((entry, position) => position > 0 && compareKeys(index.entries[position - 1].key, entry.key) === 0)) {
      setTimeout(() => abortTransaction(state, constraintError(`Unable to add key to index '${indexName}': at least one key does not satisfy the uniqueness requirements.`)), 0);
    }
    return indexObject(this, index);
  },
  delete(query) {
    requireCount(arguments.length, 1, 'delete', 'IDBObjectStore');
    const { store, transaction } = storeFields(this);
    if (store.deleted) throw storeDeleted('delete');
    checkUsable(transaction, 'delete', 'IDBObjectStore', true);
    const range = queryRange(query, 'delete', 'IDBObjectStore', true);
    const state = stateOf(transaction);
    return queueRequest(transaction, this, () => {
      touchStore(state, store);
      deleteRange(store, range, state.journal);
      return undefined;
    });
  },
  deleteIndex(name) {
    requireCount(arguments.length, 1, 'deleteIndex', 'IDBObjectStore');
    const { store, transaction } = storeFields(this);
    const state = stateOf(transaction);
    const prefix = failed('deleteIndex', 'IDBObjectStore');
    if (state.mode !== 'versionchange') throw notVersionChange('deleteIndex', 'IDBObjectStore');
    if (store.deleted) throw storeDeleted('deleteIndex');
    if (state.phase !== 'active') throw domException(prefix + TRANSACTION_INACTIVE, 'TransactionInactiveError');
    const index = store.indexes.get(String(name));
    if (index === undefined) throw domException(`${prefix}The specified index was not found.`, 'NotFoundError');
    index.deleted = true;
    store.indexes.delete(index.name);
    state.journal.push(['deleteIndex', store.name, index.name]);
  },
  index(name) {
    requireCount(arguments.length, 1, 'index', 'IDBObjectStore');
    const { store, transaction } = storeFields(this);
    const state = stateOf(transaction);
    const prefix = failed('index', 'IDBObjectStore');
    if (store.deleted) throw storeDeleted('index');
    if (state.phase === 'finished') throw domException(prefix + TRANSACTION_FINISHED, 'InvalidStateError');
    const index = store.indexes.get(String(name));
    if (index === undefined) throw domException(`${prefix}The specified index was not found.`, 'NotFoundError');
    return indexObject(this, index);
  },
  put(value, key) {
    requireCount(arguments.length, 1, 'put', 'IDBObjectStore');
    return writeRecord(this, 'put', value, key, false);
  },
};
installOperations(IDBObjectStore.prototype, storeOperations, {
  add: 1, clear: 0, count: 0, createIndex: 2, delete: 1, deleteIndex: 1, get: 1, getAll: 0, getAllKeys: 0, getAllRecords: 0, getKey: 1, index: 1, openCursor: 0, openKeyCursor: 0, put: 1,
});
blinkInterface(IDBObjectStore, 'IDBObjectStore', ['constructor']);

// ---- Índices.
class IDBIndex {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBIndex');
    hidden(this, '__idbIndex', fields);
  }
  get name() { receiver(this, P.__idbIndex); return this[P.__idbIndex].index.name; }
  set name(value) {
    receiver(this, P.__idbIndex);
    const { index, objectStore } = this[P.__idbIndex];
    const { store, transaction } = objectStore[P.__idbStore];
    const state = stateOf(transaction);
    const name = String(value);
    if (state.mode !== 'versionchange') throw notVersionChange('name', 'IDBIndex');
    if (state.phase !== 'active') throw domException(`Failed to set the 'name' property on 'IDBIndex': ${TRANSACTION_INACTIVE}`, 'TransactionInactiveError');
    if (name === index.name) return;
    if (store.indexes.has(name)) throw domException("Failed to set the 'name' property on 'IDBIndex': An index with the specified name already exists.", 'ConstraintError');
    store.indexes.delete(index.name);
    state.journal.push(['renameIndex', store.name, index.name, name]);
    index.name = name;
    store.indexes.set(name, index);
  }
  get objectStore() { receiver(this, P.__idbIndex); return this[P.__idbIndex].objectStore; }
  get keyPath() { receiver(this, P.__idbIndex); return keyPathValue(this[P.__idbIndex].index.keyPath); }
  get multiEntry() { receiver(this, P.__idbIndex); return this[P.__idbIndex].index.multiEntry; }
  get unique() { receiver(this, P.__idbIndex); return this[P.__idbIndex].index.unique; }
}
const indexTarget = (self, operation) => {
  receiver(self, P.__idbIndex);
  const { index, objectStore } = self[P.__idbIndex];
  const store = objectStore[P.__idbStore].store;
  if (index.deleted || store.deleted) throw domException(`${failed(operation, 'IDBIndex')}The index or its object store has been deleted.`, 'InvalidStateError');
  return { store, index, deleted: () => index.deleted || store.deleted };
};
const indexTransaction = (self) => self[P.__idbIndex].objectStore[P.__idbStore].transaction;
installOperations(IDBIndex.prototype, readOperations('IDBIndex', indexTarget, indexTransaction), READ_ARITIES);
blinkInterface(IDBIndex, 'IDBIndex', ['constructor']);
// O mesmo objeto de índice para o mesmo store da transação.
const indexObject = (objectStore, index) => {
  const cache = objectStore[P.__idbStore].indexes;
  if (!cache.has(index)) cache.set(index, new IDBIndex(ILLEGAL, { index, objectStore }));
  return cache.get(index);
};

// ---- Transações.
class IDBTransaction extends EventTarget {
  constructor(key = undefined, state = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBTransaction');
    super();
    hidden(this, '__idbTransaction', state);
  }
  get objectStoreNames() {
    receiver(this, P.__idbTransaction);
    const state = stateOf(this);
    return stringList(state.mode === 'versionchange' ? state.database.stores.keys() : state.scope);
  }
  get mode() { receiver(this, P.__idbTransaction); return stateOf(this).mode; }
  get durability() { receiver(this, P.__idbTransaction); return stateOf(this).durability; }
  get db() { receiver(this, P.__idbTransaction); return stateOf(this).connection.object; }
  get error() { receiver(this, P.__idbTransaction); return stateOf(this).error; }
  abort() {
    receiver(this, P.__idbTransaction);
    const state = stateOf(this);
    if (state.phase === 'finished' || state.phase === 'committing') throw domException(`${failed('abort', 'IDBTransaction')}${TRANSACTION_FINISHED}`, 'InvalidStateError');
    abortTransaction(state, null);
  }
  commit() {
    receiver(this, P.__idbTransaction);
    const state = stateOf(this);
    if (state.phase === 'finished' || state.phase === 'committing') throw domException(`${failed('commit', 'IDBTransaction')}${TRANSACTION_FINISHED}`, 'InvalidStateError');
    if (state.phase !== 'active') throw domException(`${failed('commit', 'IDBTransaction')}${TRANSACTION_INACTIVE}`, 'InvalidStateError');
    state.phase = 'committing';
    schedulePump(state);
  }
  objectStore(name) {
    receiver(this, P.__idbTransaction);
    requireCount(arguments.length, 1, 'objectStore', 'IDBTransaction');
    const state = stateOf(this);
    const prefix = failed('objectStore', 'IDBTransaction');
    if (state.phase === 'finished' || state.phase === 'committing') throw domException(prefix + TRANSACTION_FINISHED, 'InvalidStateError');
    const storeName = String(name);
    const store = state.database.stores.get(storeName);
    if (store === undefined || (state.mode !== 'versionchange' && !state.scope.includes(storeName))) throw domException(`${prefix}The specified object store was not found.`, 'NotFoundError');
    return storeObject(state, store);
  }
}
defineHandlerProperties(IDBTransaction.prototype, ['abort', 'complete', 'error']);
blinkInterface(IDBTransaction, 'IDBTransaction', ['abort', 'commit', 'objectStore', 'constructor']);
const storeObject = (state, store) => {
  if (!state.stores.has(store)) state.stores.set(store, new IDBObjectStore(ILLEGAL, { store, transaction: state.transaction, indexes: new Map() }));
  return state.stores.get(store);
};
// Cria a transação: ativa até o fim da tarefa que a criou.
const createTransaction = (connection, scope, mode, durability) => {
  const state = {
    connection, database: connection.database, mode, durability, scope, phase: 'active', queue: [], pumping: false, started: false, snapshots: new Map(), schema: null, error: null,
    stores: new Map(), onFinish: null, transaction: null, journal: [],
  };
  state.transaction = new IDBTransaction(ILLEGAL, state);
  eventParents.set(state.transaction, connection.object);
  connection.database.transactions.push(state);
  connection.transactions.add(state);
  setTimeout(() => {
    if (state.phase === 'active') state.phase = 'inactive';
    schedulePump(state);
  }, 0);
  state.started = canStart(state);
  return state;
};

// ---- Conexões.
class IDBDatabase extends EventTarget {
  constructor(key = undefined, connection = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBDatabase');
    super();
    hidden(this, '__idbConnection', connection);
  }
  get name() { receiver(this, P.__idbConnection); return this[P.__idbConnection].database.name; }
  get version() { receiver(this, P.__idbConnection); return this[P.__idbConnection].version; }
  get objectStoreNames() { receiver(this, P.__idbConnection); return stringList(this[P.__idbConnection].database.stores.keys()); }
  close() {
    receiver(this, P.__idbConnection);
    const connection = this[P.__idbConnection];
    connection.closePending = true;
    closeIfDone(connection);
  }
  createObjectStore(name, options = {}) {
    receiver(this, P.__idbConnection);
    requireCount(arguments.length, 1, 'createObjectStore', 'IDBDatabase');
    const connection = this[P.__idbConnection];
    const prefix = failed('createObjectStore', 'IDBDatabase');
    const upgrade = connection.upgrade;
    if (upgrade === null || upgrade.phase === 'finished') throw notVersionChange('createObjectStore', 'IDBDatabase');
    if (upgrade.phase !== 'active') throw domException(prefix + TRANSACTION_INACTIVE, 'TransactionInactiveError');
    const storeName = String(name);
    const opts = options === null ? {} : options;
    const keyPath = normalizeKeyPath(opts.keyPath);
    if (keyPath !== null && !validKeyPath(keyPath)) throw domException(`${prefix}The keyPath option is not a valid key path.`, 'SyntaxError');
    if (connection.database.stores.has(storeName)) throw domException(`${prefix}An object store with the specified name already exists.`, 'ConstraintError');
    const autoIncrement = !!opts.autoIncrement;
    if (autoIncrement && (keyPath === '' || Array.isArray(keyPath))) throw domException(`${prefix}The autoIncrement option was set but the keyPath option was empty or an array.`, 'InvalidAccessError');
    const store = { name: storeName, keyPath, autoIncrement, generator: 1, records: [], indexes: new Map(), deleted: false };
    connection.database.stores.set(storeName, store);
    upgrade.journal.push(['createStore', { name: storeName, keyPath, autoIncrement }]);
    return storeObject(upgrade, store);
  }
  deleteObjectStore(name) {
    receiver(this, P.__idbConnection);
    requireCount(arguments.length, 1, 'deleteObjectStore', 'IDBDatabase');
    const connection = this[P.__idbConnection];
    const prefix = failed('deleteObjectStore', 'IDBDatabase');
    const upgrade = connection.upgrade;
    if (upgrade === null || upgrade.phase === 'finished') throw notVersionChange('deleteObjectStore', 'IDBDatabase');
    if (upgrade.phase !== 'active') throw domException(prefix + TRANSACTION_INACTIVE, 'TransactionInactiveError');
    const store = connection.database.stores.get(String(name));
    if (store === undefined) throw domException(`${prefix}The specified object store was not found.`, 'NotFoundError');
    store.deleted = true;
    connection.database.stores.delete(store.name);
    upgrade.journal.push(['deleteStore', store.name]);
  }
  transaction(storeNames, mode = 'readonly', options = {}) {
    receiver(this, P.__idbConnection);
    requireCount(arguments.length, 1, 'transaction', 'IDBDatabase');
    const connection = this[P.__idbConnection];
    const prefix = failed('transaction', 'IDBDatabase');
    const text = String(mode);
    if (!['readonly', 'readwrite', 'versionchange'].includes(text)) throw new TypeError(`${prefix}The provided value '${text}' is not a valid enum value of type IDBTransactionMode.`);
    const opts = options === null ? {} : options;
    const durability = opts.durability === undefined ? 'default' : String(opts.durability);
    if (!['default', 'strict', 'relaxed'].includes(durability)) throw new TypeError(`${prefix}Failed to read the 'durability' property from 'IDBTransactionOptions': The provided value '${durability}' is not a valid enum value of type IDBTransactionDurability.`);
    if (connection.upgrade !== null && connection.upgrade.phase !== 'finished') throw domException(`${prefix}A version change transaction is running.`, 'InvalidStateError');
    if (connection.closePending) throw domException(`${prefix}The database connection is closing.`, 'InvalidStateError');
    const names = typeof storeNames === 'string' ? [storeNames] : Array.from(storeNames, String);
    const scope = [...new Set(names)].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    if (scope.some((name) => !connection.database.stores.has(name))) throw domException(`${prefix}One of the specified object stores was not found.`, 'NotFoundError');
    if (scope.length === 0) throw domException(`${prefix}The storeNames parameter was empty.`, 'InvalidAccessError');
    if (text === 'versionchange') throw new TypeError(`${prefix}The mode provided ('versionchange') is not one of 'readonly' or 'readwrite'.`);
    return createTransaction(connection, scope, text, durability).transaction;
  }
}
defineHandlerProperties(IDBDatabase.prototype, ['abort', 'close', 'error', 'versionchange']);
blinkInterface(IDBDatabase, 'IDBDatabase', ['close', 'createObjectStore', 'deleteObjectStore', 'transaction', 'constructor']);
const closeIfDone = (connection) => {
  if (!connection.closePending || connection.closed || connection.transactions.size > 0) return;
  connection.closed = true;
  connection.database.connections.delete(connection);
  if (connection.database.waiting !== null && connection.database.connections.size === 0) {
    const resume = connection.database.waiting;
    connection.database.waiting = null;
    setTimeout(resume, 0);
  }
};

// ---- Abertura e exclusão, uma de cada vez por nome.
const pendingByName = new Map();
const enqueueConnectionRequest = (name, run) => {
  const queue = pendingByName.get(name) || [];
  pendingByName.set(name, queue);
  queue.push(run);
  if (queue.length === 1) setTimeout(() => run(() => nextConnectionRequest(name)), 0);
};
const nextConnectionRequest = (name) => {
  const queue = pendingByName.get(name);
  queue.shift();
  if (queue.length > 0) setTimeout(() => queue[0](() => nextConnectionRequest(name)), 0);
  else pendingByName.delete(name);
};
const finishRequest = (request, result, error, type = error === null ? 'success' : 'error', event = undefined) => {
  const fields = request[P.__idbRequest];
  fields.done = true;
  fields.result = result;
  fields.error = error;
  fields.transaction = null;
  const fired = event || new Event(type, { bubbles: error !== null, cancelable: error !== null });
  fired.isTrusted = true;
  dispatchFromTask(request, fired);
};
// Avisa as outras conexões (`versionchange`) e, se alguma continuar aberta, a requisição (`blocked`); `proceed` roda
// quando todas fecharem.
const awaitOtherConnections = (database, request, oldVersion, newVersion, proceed) => {
  for (const connection of [...database.connections]) {
    if (!connection.closePending) fireVersionChange(connection.object, 'versionchange', oldVersion, newVersion);
  }
  if (database.connections.size === 0) {
    proceed();
    return;
  }
  fireVersionChange(request, 'blocked', oldVersion, newVersion);
  if (database.connections.size === 0) {
    database.waiting = null;
    setTimeout(proceed, 0);
    return;
  }
  database.waiting = proceed;
};
const UPGRADE_ABORTED = 'Version change transaction was aborted in upgradeneeded event handler.';
const openDatabase = (name, version, request, next) => {
  loadDatabases();
  let database = databases.get(name);
  if (database === undefined) {
    database = { name, version: 0, stores: new Map(), connections: new Set(), transactions: [], waiting: null, creating: true };
    databases.set(name, database);
  }
  const requested = version === undefined ? Math.max(database.version, 1) : version;
  if (requested < database.version) {
    finishRequest(request, undefined, domException(`The requested version (${requested}) is less than the existing version (${database.version}).`, 'VersionError'));
    next();
    return;
  }
  const connection = { database, version: database.version, closePending: false, closed: false, transactions: new Set(), upgrade: null, object: null };
  connection.object = new IDBDatabase(ILLEGAL, connection);
  if (requested === database.version) {
    database.connections.add(connection);
    finishRequest(request, connection.object, null);
    next();
    return;
  }
  awaitOtherConnections(database, request, database.version, requested, () => {
    database.connections.add(connection);
    const oldVersion = database.version;
    const schema = snapshotSchema(database);
    database.version = requested;
    connection.version = requested;
    const state = createTransaction(connection, [], 'versionchange', 'default');
    state.schema = schema;
    state.started = true;
    connection.upgrade = state;
    const fields = request[P.__idbRequest];
    fields.done = true;
    fields.result = connection.object;
    fields.transaction = state.transaction;
    state.onFinish = (error) => {
      connection.upgrade = null;
      if (error === null) {
        database.creating = false;
        finishRequest(request, connection.object, null);
      } else {
        connection.version = oldVersion;
        connection.closePending = true;
        closeIfDone(connection);
        database.connections.delete(connection);
        if (database.creating) databases.delete(name);
        finishRequest(request, undefined, domException(UPGRADE_ABORTED, 'AbortError'));
      }
      next();
    };
    const before = listenerFailures;
    fireVersionChange(request, 'upgradeneeded', oldVersion, requested);
    if (state.phase === 'active') state.phase = 'inactive';
    if (listenerFailures !== before) abortTransaction(state, domException(UPGRADE_ABORTED, 'AbortError'));
    else schedulePump(state);
  });
};
const deleteNamedDatabase = (name, request, next) => {
  loadDatabases();
  const database = databases.get(name);
  const done = (oldVersion) => {
    const event = new IDBVersionChangeEvent('success', { oldVersion, newVersion: null });
    finishRequest(request, undefined, null, 'success', event);
    next();
  };
  if (database === undefined || database.creating) {
    done(0);
    return;
  }
  awaitOtherConnections(database, request, database.version, null, () => {
    databases.delete(name);
    op('idb', 'delete', name);
    done(database.version);
  });
};

// ---- A fábrica.
const readVersion = (value) => {
  if (value === undefined) return undefined;
  const number = Number(value);
  if (!Number.isFinite(number)) throw new TypeError(`${failed('open', 'IDBFactory')}Value is not of type 'unsigned long long'.`);
  const integer = Math.trunc(number);
  if (integer < 0 || integer > Number.MAX_SAFE_INTEGER) throw new TypeError(`${failed('open', 'IDBFactory')}Value is outside the 'unsigned long long' value range.`);
  if (integer === 0) throw new TypeError(`${failed('open', 'IDBFactory')}The version provided must not be 0.`);
  return integer;
};
class IDBFactory {
  constructor(key = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('IDBFactory');
    hidden(this, '__idbFactory', true);
  }
  cmp(first, second) {
    receiver(this, P.__idbFactory);
    requireCount(arguments.length, 2, 'cmp', 'IDBFactory');
    return compareKeys(requireKey(first, 'cmp', 'IDBFactory'), requireKey(second, 'cmp', 'IDBFactory'));
  }
  databases() {
    receiver(this, P.__idbFactory);
    loadDatabases();
    return new Promise((resolve) => setTimeout(() => resolve([...databases.values()].filter((database) => !database.creating).map((database) => ({ name: database.name, version: database.version }))), 0));
  }
  deleteDatabase(name) {
    receiver(this, P.__idbFactory);
    requireCount(arguments.length, 1, 'deleteDatabase', 'IDBFactory');
    const request = new IDBOpenDBRequest(ILLEGAL);
    const text = String(name);
    enqueueConnectionRequest(text, (next) => deleteNamedDatabase(text, request, next));
    return request;
  }
  open(name, version = undefined) {
    receiver(this, P.__idbFactory);
    requireCount(arguments.length, 1, 'open', 'IDBFactory');
    const text = String(name);
    const requested = readVersion(version);
    // Origem opaca (documento `data:`): o Blink recusa depois de validar a versão (medido no oráculo).
    if (pageState.url.startsWith('data:')) throw domException(`${failed('open', 'IDBFactory')}access to the Indexed Database API is denied in this context.`, 'SecurityError');
    const request = new IDBOpenDBRequest(ILLEGAL);
    enqueueConnectionRequest(text, (next) => openDatabase(text, requested, request, next));
    return request;
  }
}
blinkInterface(IDBFactory, 'IDBFactory', ['constructor']);

const indexedDB = new IDBFactory(ILLEGAL);
readonly('indexedDB', () => indexedDB);
const indexedDBInterfaces = {
  DOMStringList, IDBCursor, IDBCursorWithValue, IDBDatabase, IDBFactory, IDBIndex, IDBKeyRange, IDBObjectStore, IDBOpenDBRequest, IDBRecord, IDBRequest, IDBTransaction, IDBVersionChangeEvent,
};
Object.assign(globalThis, indexedDBInterfaces);
for (const key of Object.keys(indexedDBInterfaces)) Object.defineProperty(globalThis, key, { enumerable: false });
