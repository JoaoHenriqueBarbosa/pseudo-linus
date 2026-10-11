const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, e.message); }
};
(async () => {
  await show('undefined', async () => ReadableStream.from());
  await show('null', async () => ReadableStream.from(null));
  await show('undefined arg', async () => ReadableStream.from(undefined));
  await show('through variable', async () => { const missing = null; return ReadableStream.from(missing); });
})();
