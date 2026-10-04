// Counters cross the SDK as decimal strings so exact values survive the script boundary.
export const integer = value => value == null ? null : BigInt(value);
export const total = usage => usage == null ? null : integer(usage.input) + integer(usage.output);
export function compact(value) {
  if (value == null) return null;
  value = integer(value);
  for (const [scale,suffix] of [[1000000000000n,'T'],[1000000000n,'B'],[1000000n,'M'],[1000n,'k']]) {
    if (value >= scale) return (Number(value) / Number(scale)).toFixed(1) + suffix;
  }
  return String(value);
}
export function amount(micros) {
  if (micros == null) return null;
  micros = integer(micros);
  return `${micros / 1000000n}.${String(micros % 1000000n / 10000n).padStart(2,'0')}`;
}
export function cache(usage) {
  if (!usage || integer(usage.input) === 0n) return null;
  return (100 * Number(usage.cached_input) / Number(usage.input)).toFixed(0) + '%';
}
export function speed(generation) {
  return generation && integer(generation.elapsed_us) > 0n
    ? (Number(generation.output_tokens) * 1000000 / Number(generation.elapsed_us)).toFixed(1) : null;
}
export function cost(statistics) {
  if (!statistics?.cost) return null;
  return amount(statistics.cost.usd_micros) + (integer(statistics.cost.responses) < integer(statistics.responses) ? '+' : '');
}
export function context(tokens,limit) {
  return tokens != null && limit != null && Number(limit) > 0 ? 100 * Number(tokens) / Number(limit) : null;
}
export function severity(percent) {
  return percent == null || percent < 80 ? 'muted' : percent < 100 ? 'warning' : 'danger';
}
export function ranking(groups) {
  return groups.filter(entry => entry.group.key.kind === 'model').map(entry => ({...entry,tokens:total(entry.group.metrics.tokens)}))
    .sort((left,right) => {
      if (left.tokens !== right.tokens) {
        if (left.tokens === null) return 1;
        if (right.tokens === null) return -1;
        return left.tokens > right.tokens ? -1 : 1;
      }
      const a = left.group.key.data, b = right.group.key.data;
      for (const [first,second] of [[a.model,b.model],[left.node,right.node],[a.provider,b.provider]]) {
        if (first !== second) return first < second ? -1 : 1;
      }
      return 0;
    });
}
