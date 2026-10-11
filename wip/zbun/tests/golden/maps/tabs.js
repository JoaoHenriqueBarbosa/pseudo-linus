function f(a, b) {
	if (a) {
		return b;
	}
	for (let i = 0; i < a; i++) {
		b += i;
	}
	return b;
}
console.log(f(1, 2));
