// GERADO por `scripts/gen-node-internals.js` a partir dos `src/js/internal/*` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, toClass, codes, makeAbortError } = host.internal;
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  const isBlob = value => typeof Blob === "function" && value instanceof Blob;
  // O que ainda não existe (TLS, `BlockList`...): lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
  const cache = { __proto__: null };
  const definitions = {
    "internal/shared": () => {
      const ObjectFreeze = Object.freeze;

      class NotImplementedError extends Error {
        code;
        constructor(feature, issue, extra) {
          super(feature + " is not yet implemented in Bun." + (issue ? " Track the status & thumbs up the issue: https://github.com/oven-sh/bun/issues/" + issue : "") + (extra ? ". " + extra : ""));
          this.name = "NotImplementedError";
          this.code = "ERR_NOT_IMPLEMENTED";
          hideFromStack(NotImplementedError);
        }
        get ["constructor"]() {
          return Error;
        }
      }
      function throwNotImplemented(feature, issue, extra) {
        hideFromStack(throwNotImplemented);
        throw new NotImplementedError(feature, issue, extra);
      }
      function hideFromStack(...fns) {
        for (const fn of fns) {
          Object.defineProperty(fn, "name", {
            value: "::bunternal::"
          });
        }
      }
      let warned;
      function warnNotImplementedOnce(feature, issue) {
        if (!warned) {
          warned = new Set;
        }
        if (warned.has(feature)) {
          return;
        }
        warned.add(feature);
        console.warn(new NotImplementedError(feature, issue));
      }
      let util;

      class ExceptionWithHostPort extends Error {
        errno;
        syscall;
        port;
        address;
        constructor(err, syscall, address, port, additional) {
          util ??= require("node:util");
          const code = util.getSystemErrorName(err);
          let details = "";
          if (port && port > 0) {
            details = ` ${address}:${port}`;
          } else if (address) {
            details = ` ${address}`;
          }
          if (additional) {
            details += ` - Local (${additional})`;
          }
          super(`${syscall} ${code}${details}`);
          this.errno = err;
          this.code = code;
          this.syscall = syscall;
          this.address = address;
          if (port) {
            this.port = port;
          }
        }
        get ["constructor"]() {
          return Error;
        }
      }

      class NodeAggregateError extends AggregateError {
        constructor(errors, message) {
          super(new (require("internal/primordials")).SafeArrayIterator(errors), message);
          this.code = errors[0]?.code;
        }
        get ["constructor"]() {
          return AggregateError;
        }
      }

      class ConnResetException extends Error {
        constructor(msg) {
          super(msg);
          this.code = "ECONNRESET";
        }
        get ["constructor"]() {
          return Error;
        }
      }

      class ErrnoException extends Error {
        errno;
        syscall;
        constructor(err, syscall, original) {
          util ??= require("node:util");
          const code = util.getSystemErrorName(err);
          const message = original ? `${syscall} ${code} ${original}` : `${syscall} ${code}`;
          super(message);
          this.errno = err;
          this.code = code;
          this.syscall = syscall;
        }
        get ["constructor"]() {
          return Error;
        }
      }
      function once(callback, { preserveReturnValue = false } = kEmptyObject) {
        let called = false;
        let returnValue;
        return function(...args) {
          if (called)
            return returnValue;
          called = true;
          const fn = callback;
          callback = undefined;
          const result = fn.apply(this, args);
          returnValue = preserveReturnValue ? result : undefined;
          return result;
        };
      }
      const kEmptyObject = ObjectFreeze(Object.create(null));
      const kInternalSendOptions = Object.create(null);
      ObjectFreeze(kInternalSendOptions);
      const reportUncaughtException = error => queueMicrotask(() => { throw error; });
      function guardCallback(callback) {
        return function guarded(a, b, c) {
          try {
            switch (arguments.length) {
              case 0:
                return callback();
              case 1:
                return callback(a);
              case 2:
                return callback(a, b);
              case 3:
                return callback(a, b, c);
              default:
                return callback.apply(undefined, arguments);
            }
          } catch (e) {
            reportUncaughtException(e);
          }
        };
      }
      const nodeModulesRE = /[\\/]node_modules[\\/]/;
      const ErrorCaptureStackTrace = Error.captureStackTrace;
      function returnStackFrames(_err, frames) {
        return frames;
      }
      function isInsideNodeModules(frameLimit) {
        let prevLimit, prevPrepare;
        let frames;
        try {
          prevLimit = Error.stackTraceLimit;
          prevPrepare = Error.prepareStackTrace;
          Error.stackTraceLimit = frameLimit;
          Error.prepareStackTrace = returnStackFrames;
          const target = {};
          ErrorCaptureStackTrace(target, isInsideNodeModules);
          frames = target.stack;
        } catch {} finally {
          try {
            Error.stackTraceLimit = prevLimit;
            Error.prepareStackTrace = prevPrepare;
          } catch {}
        }
        if (!Array.isArray(frames))
          return false;
        try {
          for (const frame of frames) {
            const filename = frame.getFileName();
            if (!filename || filename.startsWith("node:") || filename.startsWith("internal:") || filename === "native") {
              continue;
            }
            return nodeModulesRE.test(filename);
          }
        } catch {}
        return false;
      }
      function resistStopPropagation(options) {
        options[host.internal.kResistStopPropagation] = true;
        return options;
      }
      function getLazy(initializer) {
        let value;
        let initialized = false;
        return function() {
          if (initialized)
            return value;
          value = initializer();
          initialized = true;
          return value;
        };
      }
      const observerCounts = new Map;
      const kObservers = new Set;
      const kNodeEntryTypes = new Set(["net", "dns", "http", "http2", "function", "quic"]);
      function hasObserver(type) {
        return (observerCounts.get(type) ?? 0) > 0;
      }
      function enqueueNodeEntry(entry) {
        for (const observer of kObservers) {
          observer.bufferEntry(entry);
        }
      }

      class PerformanceNodeEntry {
        name;
        entryType;
        startTime;
        duration;
        detail;
        constructor(name, entryType, startTime, duration, detail) {
          this.name = name;
          this.entryType = entryType;
          this.startTime = startTime;
          this.duration = duration;
          this.detail = detail;
        }
        toJSON() {
          return {
            name: this.name,
            entryType: this.entryType,
            startTime: this.startTime,
            duration: this.duration,
            detail: this.detail
          };
        }
      }
      function startPerf(target, key, context) {
        context.startTime = performance.now();
        target[key] = context;
      }
      function stopPerf(target, key, context) {
        const ctx = target[key];
        if (!ctx) {
          return;
        }
        target[key] = undefined;
        const startTime = ctx.startTime;
        const detail = ctx.detail !== undefined || context?.detail !== undefined ? { ...ctx.detail, ...context?.detail } : undefined;
        enqueueNodeEntry(new PerformanceNodeEntry(ctx.name, ctx.type, startTime, performance.now() - startTime, detail));
      }

      class NodeEntryObserver {
        callback;
        owner;
        types = new Set;
        buffer = [];
        scheduled = false;
        constructor(callback, owner) {
          this.callback = callback;
          this.owner = owner;
        }
        observe(types) {
          for (const type of this.types) {
            observerCounts.set(type, (observerCounts.get(type) ?? 1) - 1);
          }
          this.types = new Set(types);
          for (const type of this.types) {
            observerCounts.set(type, (observerCounts.get(type) ?? 0) + 1);
          }
          kObservers.add(this);
        }
        disconnect() {
          for (const type of this.types) {
            observerCounts.set(type, (observerCounts.get(type) ?? 1) - 1);
          }
          this.types.clear();
          this.buffer = [];
          kObservers.delete(this);
        }
        bufferEntry(entry) {
          if (!this.types.has(entry.entryType)) {
            return;
          }
          this.buffer.push(entry);
          if (!this.scheduled) {
            this.scheduled = true;
            setImmediate(() => {
              this.scheduled = false;
              const entries = this.buffer;
              if (entries.length === 0) {
                return;
              }
              this.buffer = [];
              this.callback.call(undefined, makeNodeEntryList(entries), this.owner);
            });
          }
        }
      }
      function makeNodeEntryList(entries) {
        const sorted = entries.slice().sort((a, b) => a.startTime - b.startTime);
        return {
          getEntries() {
            return sorted.slice();
          },
          getEntriesByType(type) {
            return sorted.filter((entry) => entry.entryType === type);
          },
          getEntriesByName(name, type) {
            return sorted.filter((entry) => entry.name === name && (type === undefined || entry.entryType === type));
          }
        };
      }
      const kInternalAssertionSuffix = `
      This is caused by either a bug in Node.js or incorrect usage of Node.js internals.
      ` + `Please open an issue with this stack trace at https://github.com/nodejs/node/issues
      `;
      return {
        kInternalAssertionSuffix,
        throwNotImplemented,
        hideFromStack,
        warnNotImplementedOnce,
        ExceptionWithHostPort,
        NodeAggregateError,
        ConnResetException,
        ErrnoException,
        once,
        getLazy,
        guardCallback,
        isInsideNodeModules,
        reportUncaughtException,
        resistStopPropagation,
        hasObserver,
        startPerf,
        stopPerf,
        enqueueNodeEntry,
        kNodeEntryTypes,
        NodeEntryObserver,
        PerformanceNodeEntry,
        kHandle: Symbol("kHandle"),
        kClusterOwner: Symbol("kClusterOwner"),
        kAutoDestroyed: Symbol("kAutoDestroyed"),
        kWeakHandler: Symbol("kWeak"),
        kCustomPromisifyArgsSymbol: Symbol("customPromisifyArgs"),
        kEmptyObject,
        kInternalSendOptions
      };
    },
    "internal/timers": () => {
      const { validateNumber } = require("internal/validators");
      const NumberIsFinite = Number.isFinite;
      const TIMEOUT_MAX = 2 ** 31 - 1;
      function getTimerDuration(msecs, name) {
        validateNumber(msecs, name);
        if (msecs < 0 || !NumberIsFinite(msecs)) {
          throw codes.OUT_OF_RANGE(name, "a non-negative finite number", msecs);
        }
        if (msecs > TIMEOUT_MAX) {
          process.emitWarning(`${msecs} does not fit into a 32-bit signed integer.` + `
      Timer duration was truncated to ${TIMEOUT_MAX}.`, "TimeoutOverflowWarning");
          return TIMEOUT_MAX;
        }
        return msecs;
      }
      return {
        kTimeout: Symbol.for("::buntimeout::"),
        getTimerDuration
      };
    },
    "internal/net/isIP": () => {
      const v4Seg = "(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";
      const v4Str = `(?:${v4Seg}\\.){3}${v4Seg}`;
      var IPv4Reg;
      const v6Seg = "(?:[0-9a-fA-F]{1,4})";
      var IPv6Reg;
      function isIPv4(s) {
        return (IPv4Reg ??= new RegExp(`^${v4Str}$`)).test(s);
      }
      function isIPv6(s) {
        return (IPv6Reg ??= new RegExp("^(?:" + `(?:${v6Seg}:){7}(?:${v6Seg}|:)|` + `(?:${v6Seg}:){6}(?:${v4Str}|:${v6Seg}|:)|` + `(?:${v6Seg}:){5}(?::${v4Str}|(?::${v6Seg}){1,2}|:)|` + `(?:${v6Seg}:){4}(?:(?::${v6Seg}){0,1}:${v4Str}|(?::${v6Seg}){1,3}|:)|` + `(?:${v6Seg}:){3}(?:(?::${v6Seg}){0,2}:${v4Str}|(?::${v6Seg}){1,4}|:)|` + `(?:${v6Seg}:){2}(?:(?::${v6Seg}){0,3}:${v4Str}|(?::${v6Seg}){1,5}|:)|` + `(?:${v6Seg}:){1}(?:(?::${v6Seg}){0,4}:${v4Str}|(?::${v6Seg}){1,6}|:)|` + `(?::(?:(?::${v6Seg}){0,5}:${v4Str}|(?::${v6Seg}){1,7}|:))` + ")(?:%[0-9a-zA-Z-.:]{1,})?$")).test(s);
      }
      function isIP(s) {
        if (isIPv4(s))
          return 4;
        if (isIPv6(s))
          return 6;
        return 0;
      }
      return {
        isIPv4,
        isIPv6,
        isIP
      };
    },
    "internal/net/symbols": () => {
      return {
        kArmHandshakeTimeout: Symbol("kArmHandshakeTimeout"),
        kSecureConnectDone: Symbol("kSecureConnectDone"),
        kVerifyError: Symbol("kVerifyError")
      };
    },
    "internal/async_hooks": () => {
      let activeHooks = 0;
      function enabledHooksExist() {
        return activeHooks > 0;
      }
      function markHookEnabled() {
        activeHooks += 1;
      }
      function markHookDisabled() {
        if (activeHooks > 0)
          activeHooks -= 1;
      }
      const owner_symbol = Symbol("owner_symbol");
      return {
        enabledHooksExist,
        markHookEnabled,
        markHookDisabled,
        symbols: { owner_symbol }
      };
    },
    "internal/cluster/isPrimary": () => {
      return {
        isPrimary: !Object.prototype.hasOwnProperty.call(process.env, "NODE_UNIQUE_ID")
      };
    },
    "internal/tls": () => {
      const { isTypedArray, isArrayBuffer } = require("node:util/types");
      function isPemObject(obj) {
        return isObject(obj) && "pem" in obj;
      }
      function isPemArray(obj) {
        return Array.isArray(obj) && obj.every(isPemObject);
      }
      function isValidTLSItem(obj) {
        if (typeof obj === "string" || isTypedArray(obj) || isArrayBuffer(obj) || isBlob(obj) || isPemArray(obj)) {
          return true;
        }
        return false;
      }
      function findInvalidTLSItem(obj) {
        if (Array.isArray(obj)) {
          for (var i = 0, length = obj.length;i < length; i++) {
            const item = obj[i];
            if (!isValidTLSItem(item))
              return item;
          }
        }
        return obj;
      }
      function throwOnInvalidTLSArray(name, value) {
        if (!isValidTLSArray(value)) {
          throw codes.INVALID_ARG_TYPE(name, VALID_TLS_ERROR_MESSAGE_TYPES, findInvalidTLSItem(value));
        }
      }
      function isValidTLSArray(obj) {
        if (isValidTLSItem(obj))
          return true;
        if (Array.isArray(obj)) {
          for (var i = 0, length = obj.length;i < length; i++) {
            const item = obj[i];
            if (!isValidTLSItem(item))
              return false;
          }
          return true;
        }
        return false;
      }
      const VALID_TLS_ERROR_MESSAGE_TYPES = "string or an instance of Buffer, TypedArray, or DataView";
      const TLS1_VERSION = 769;
      const TLS1_1_VERSION = 770;
      const TLS1_2_VERSION = 771;
      const TLS1_3_VERSION = 772;
      function tlsStringToProtocolVersion(v) {
        switch (v) {
          case "TLSv1":
            return TLS1_VERSION;
          case "TLSv1.1":
            return TLS1_1_VERSION;
          case "TLSv1.2":
            return TLS1_2_VERSION;
          case "TLSv1.3":
            return TLS1_3_VERSION;
          default:
            return 0;
        }
      }
      let _SECURE_PROTOCOL_METHODS;
      function validateSecureProtocol(secureProtocol) {
        if (secureProtocol === undefined || secureProtocol === null)
          return;
        if (typeof secureProtocol !== "string") {
          throw codes.INVALID_ARG_TYPE("options.secureProtocol", "string", secureProtocol);
        }
        let message;
        if (secureProtocol.startsWith("SSLv2_"))
          message = "SSLv2 methods disabled";
        else if (secureProtocol.startsWith("SSLv3_"))
          message = "SSLv3 methods disabled";
        else {
          _SECURE_PROTOCOL_METHODS ??= new Set([
            "TLS_method",
            "TLS_client_method",
            "TLS_server_method",
            "SSLv23_method",
            "SSLv23_client_method",
            "SSLv23_server_method",
            "TLSv1_method",
            "TLSv1_client_method",
            "TLSv1_server_method",
            "TLSv1_1_method",
            "TLSv1_1_client_method",
            "TLSv1_1_server_method",
            "TLSv1_2_method",
            "TLSv1_2_client_method",
            "TLSv1_2_server_method"
          ]);
          if (!_SECURE_PROTOCOL_METHODS.has(secureProtocol))
            message = `Unknown method: ${secureProtocol}`;
        }
        if (message !== undefined)
          throw codes.TLS_INVALID_PROTOCOL_METHOD(message);
      }
      function secureProtocolToVersionRange(secureProtocol) {
        if (typeof secureProtocol !== "string")
          return null;
        if (secureProtocol === "TLSv1_method" || secureProtocol === "TLSv1_client_method" || secureProtocol === "TLSv1_server_method")
          return [TLS1_VERSION, TLS1_VERSION];
        if (secureProtocol === "TLSv1_1_method" || secureProtocol === "TLSv1_1_client_method" || secureProtocol === "TLSv1_1_server_method")
          return [TLS1_1_VERSION, TLS1_1_VERSION];
        if (secureProtocol === "TLSv1_2_method" || secureProtocol === "TLSv1_2_client_method" || secureProtocol === "TLSv1_2_server_method")
          return [TLS1_2_VERSION, TLS1_2_VERSION];
        return null;
      }
      let NativeSecureContext;
      function processPfxOptions(options) {
        if (options == null || options.pfx == null)
          return options;
        NativeSecureContext ??= require("native/secure-context");
        const out = { ...options };
        const keys = out.key == null ? [] : Array.isArray(out.key) ? [...out.key] : [out.key];
        const certs = out.cert == null ? [] : Array.isArray(out.cert) ? [...out.cert] : [out.cert];
        const pfxCAs = [];
        const entries = Array.isArray(out.pfx) ? out.pfx : [out.pfx];
        for (const entry of entries) {
          let buf = entry;
          let passphrase = out.passphrase;
          if (entry != null && typeof entry === "object" && !Buffer.isBuffer(entry) && !(ArrayBuffer.isView(entry) && !(entry instanceof DataView))) {
            const entryBuf = entry.buf;
            if (entryBuf !== undefined) {
              buf = entryBuf;
              passphrase = entry.passphrase || passphrase;
            }
          }
          const parsed = NativeSecureContext.parsePkcs12(buf, passphrase);
          keys.push(parsed.key);
          certs.push(parsed.cert);
          const parsedCA = parsed.ca;
          if (parsedCA)
            pfxCAs.push(parsedCA);
        }
        out.key = keys.length === 1 ? keys[0] : keys;
        out.cert = certs.length === 1 ? certs[0] : certs;
        if (pfxCAs.length)
          out._pfxExtraCACerts = pfxCAs;
        out.pfx = undefined;
        return out;
      }

      return {
        processPfxOptions,
        secureProtocolToVersionRange,
        throwOnInvalidTLSArray,
        tlsStringToProtocolVersion,
        validateSecureProtocol
      };
    },
    "native/secure-context": () => {
      function SecureContext() {
        return host.internal.throwNotImplemented("tls SecureContext");
      }
      for (const name of ["intern", "createPrivate", "parsePkcs12"]) SecureContext[name] = () => host.internal.throwNotImplemented("tls SecureContext");
      return SecureContext;
    },
  };
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/primordials") return host.internal;
    if (name in definitions) {
      if (!(name in cache)) cache[name] = definitions[name]();
      return cache[name];
    }
    return host.requireBuiltin(name);
  };
  return require;
})
