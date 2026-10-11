interface Point { x: number; y: number }
enum Color { Red, Green = 'green', Blue = 4 }
type Pair<T> = [T, T];
class Shape<T extends Point = Point> {
  constructor(private readonly origin: T, public label: string = 'shape') {}
  describe(): string { return `${this.label}@${this.origin.x},${this.origin.y}`; }
}
function swap<T>(p: Pair<T>): Pair<T> { return [p[1], p[0]]; }
const s = new Shape({ x: 1, y: 2 }, 'a');
console.log(s.describe(), Color.Red, Color.Green, Color.Blue, Color[4]);
console.log(swap<number>([1, 2]), (s as unknown as { label: string }).label);
const cfg = { port: 8080, host: 'localhost' } satisfies Record<string, string | number>;
console.log(cfg, [1, 2, 3].map((n): number => n * 2));
