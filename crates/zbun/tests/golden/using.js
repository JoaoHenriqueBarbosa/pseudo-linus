{
  using a = getResource();
  await using b = getAsyncResource();
  using c = null, d = undefined;
  console.log(a, b, c, d);
}
for (using x of y) {}
for (await using z of w) {}
function getResource() {} function getAsyncResource() {} var y, w;
