export const FLOAT_SIGNS: [string, RegExp][] = [
  ["a Math function that rounds or draws", /\bMath\.(ceil|floor|round|trunc|fround|random)\b/],
  ["parseFloat", /\bparseFloat\b/],
  ["toFixed or toPrecision", /\.(toFixed|toPrecision)\b/],
  ["a decimal or exponent literal", /\b[0-9][0-9_]*\.[0-9]|\b[0-9][0-9_]*e[-+]?[0-9]/],
  ["the division operator", /\//],
];

export function codeOf(text: string): string[] {
  return text
    .replace(/`(?:[^`\\]|\\.)*`/g, "``")
    .replace(/"(?:[^"\\\n]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\\n]|\\.)*'/g, "''")
    .split("\n");
}
