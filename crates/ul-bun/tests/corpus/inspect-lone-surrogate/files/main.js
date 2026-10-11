const lone = ["\ud800", "\ude40", "a😀b", "\ud83d", "\ude00\ud83d", "\ud83d😀", "é\u0001"];
console.log(lone);
console.log({ k: "\ude40" });
console.log(Bun.inspect("\ude40"), Bun.inspect(["\ud800"]));
process.exit(0);
