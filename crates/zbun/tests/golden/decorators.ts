function sealed(c: Function) {}
function log(t: any, k: string, d: PropertyDescriptor) {}
@sealed
class Greeter {
  @log greet() { return "hi"; }
  @sealed prop = 1;
}
export { Greeter };
