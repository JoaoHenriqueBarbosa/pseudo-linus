function over(a: string): string;
function over(a: number): number;
function over(a: any): any { return a; }
class C {
  declare d: number;
  x!: number;
  y?: string;
  readonly [key: string]: any;
  static [k: string]: number;
  override m(): void {}
  public abstract_?: void;
  private static readonly s = 1;
  constructor(public readonly a: number, private b?: string) {}
  get v(): number { return 1; }
  set v(x: number) {}
  m2<T extends object = {}>(this: C, ...args: T[]): asserts args is T[] {}
  m3(x: unknown): x is string { return true; }
}
type Tup = [a: number, b?: string, ...rest: boolean[]];
type Mapped<T> = { readonly [K in keyof T]?: T[K] };
type Tmpl = `a${string}b`;
type Fn = (a: number) => void; type Ctor = new (a: number) => object; type ACtor = abstract new () => object;
declare global { interface Window { x: number } }
declare module "m" { export default function f(): void; }
export = C;
