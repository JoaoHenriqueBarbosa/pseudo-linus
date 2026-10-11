// A entrada de navegação: forma, protótipos, chaves do toJSON, quais instantes são zero e a ordem dos demais; o legado
// performance.timing e performance.navigation; os tamanhos conferidos contra o corpo buscado de novo.
const out = [];
// Instantes e a duração mudam a cada carga (e o grampo de 100 µs decide se saem inteiros): só zero ou não.
const shape = (v) => typeof v === 'number' ? (v === 0 ? '0' : v > 0 ? 'positive' : 'negative:' + v) : JSON.stringify(v);
addEventListener('load', () => setTimeout(async () => {
  const list = performance.getEntriesByType('navigation');
  out.push('count ' + list.length + ' ' + performance.getEntries().filter((e) => e.entryType === 'navigation').length);
  const e = list[0];
  let p = Object.getPrototypeOf(e); const chain = [];
  while (p && p !== Object.prototype) { chain.push(p.constructor.name + '[' + Object.getOwnPropertyNames(p).join(',') + ']'); p = Object.getPrototypeOf(p); }
  out.push('chain ' + chain.join(' > '));
  out.push('own ' + Object.getOwnPropertyNames(e).join(','));
  out.push('tag ' + Object.prototype.toString.call(e) + ' ' + (e === performance.getEntriesByType('navigation')[0]));
  const json = e.toJSON();
  const masked = { navigationId: (v) => typeof v, confidence: (v) => Object.keys(v).join('+') + ' ' + typeof v.randomizedTriggerRate + ' ' + ['low', 'high'].includes(v.value), encodedBodySize: () => 'body', decodedBodySize: () => 'body', transferSize: () => 'body+300' };
  for (const k of Object.keys(json)) out.push('json ' + k + ' ' + (masked[k] ? masked[k](json[k]) : Array.isArray(json[k]) ? 'array' + json[k].length : shape(json[k])));
  out.push('same ' + Object.keys(json).every((k) => k === 'serverTiming' || k === 'confidence' || json[k] === e[k]));
  const order = ['startTime', 'redirectStart', 'redirectEnd', 'workerStart', 'fetchStart', 'domainLookupStart', 'domainLookupEnd', 'connectStart', 'secureConnectionStart', 'connectEnd', 'requestStart', 'finalResponseHeadersStart', 'responseStart', 'responseEnd', 'domInteractive', 'domContentLoadedEventStart', 'domContentLoadedEventEnd', 'domComplete', 'loadEventStart', 'loadEventEnd'];
  out.push('zero ' + order.filter((k) => e[k] === 0).join(','));
  const nz = order.filter((k) => e[k] !== 0);
  out.push('monotone ' + nz.every((k, i) => i === 0 || e[k] >= e[nz[i - 1]]) + ' ' + (e.responseStart === e.finalResponseHeadersStart) + ' ' + (e.duration === e.loadEventEnd));
  out.push('byName ' + performance.getEntriesByName(location.href).length + ' ' + performance.getEntriesByName(location.href, 'navigation').length + ' ' + performance.getEntriesByName(location.href, 'resource').length);
  out.push('entries0 ' + performance.getEntries()[0].entryType);
  out.push('legacy ' + JSON.stringify(performance.navigation.toJSON()) + ' ' + Object.prototype.toString.call(performance.navigation));
  const timing = performance.timing.toJSON();
  out.push('timing ' + Object.keys(timing).join(','));
  out.push('tzero ' + Object.entries(timing).filter(([, v]) => v === 0).map(([k]) => k).join(','));
  out.push('tmatch ' + ['fetchStart', 'requestStart', 'responseEnd', 'domInteractive', 'loadEventEnd'].every((k) => Math.abs(timing[k] - performance.timeOrigin - e[k]) < 2));
  const c = e.confidence;
  out.push('confidence ' + Object.prototype.toString.call(c) + ' ' + Object.getOwnPropertyNames(Object.getPrototypeOf(c)).join(',') + ' ' + JSON.stringify(Object.keys(c.toJSON())) + ' ' + (c === e.confidence) + ' ' + (c.randomizedTriggerRate > 0 && c.randomizedTriggerRate < 1));
  out.push('serverTiming ' + JSON.stringify(e.serverTiming) + ' ' + Array.isArray(e.serverTiming) + ' ' + Object.isFrozen(e.serverTiming));
  const ctor = [];
  for (const C of [PerformanceNavigationTiming, PerformanceResourceTiming]) { try { new C(); ctor.push('built'); } catch (error) { ctor.push(error.constructor.name + ': ' + error.message); } }
  out.push('ctor ' + ctor.join(' | '));
  try { Object.getOwnPropertyDescriptor(PerformanceNavigationTiming.prototype, 'type').get.call({}); } catch (error) { out.push('brand ' + error.constructor.name + ': ' + error.message); }
  const body = await (await fetch(location.href)).text();
  out.push('size ' + (e.encodedBodySize === body.length) + ' ' + (e.decodedBodySize === body.length) + ' ' + (e.transferSize - e.encodedBodySize));
  console.log(out.join('\n'));
}, 50));
