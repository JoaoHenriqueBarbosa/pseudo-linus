function logged<T extends (...a: any[]) => any>(fn: T, ctx: ClassMethodDecoratorContext) {
  return function (this: unknown, ...args: Parameters<T>): ReturnType<T> {
    console.log('call', String(ctx.name), args);
    return fn.apply(this, args);
  } as T;
}
function sealed<C extends new (...a: any[]) => object>(c: C, ctx: ClassDecoratorContext) { console.log('class', ctx.name); return c; }
@sealed
class Service {
  @logged add(a: number, b: number) { return a + b; }
  @logged static make() { return new Service(); }
}
console.log(Service.make().add(2, 3));
