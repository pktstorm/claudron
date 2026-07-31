/// True when `a` is an earlier version than `b`.
///
/// Compares numerically per dotted segment. A string comparison would place
/// "2.1.99" above "2.1.220" and invert the whole feature.
export function isOlder(a: string, b: string): boolean {
  // Whole-string parse, matching Rust's `str::parse::<u32>`. Number.parseInt
  // prefix-parses, so "220-beta" would yield 220 here and 0 in Rust -- the
  // backend would pick a baseline the frontend then styles as current.
  const seg = (s: string) => (/^\d+$/.test(s) ? Number(s) : 0);
  const pa = a.split(".").map(seg);
  const pb = b.split(".").map(seg);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i += 1) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x < y;
  }
  return false;
}
