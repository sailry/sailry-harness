import { legal, pattern, sort } from "./cards.js";

export function sweep(hand, selected, start, end) {
  const range = hand.slice(Math.min(start, end), Math.max(start, end) + 1);
  if (selected.includes(hand[start])) return selected.filter(card => !range.includes(card));
  const merged = sort([...new Set([...selected, ...range])]);
  if (pattern(merged)) return merged;
  let next = range;
  if (!pattern(range)) {
    const moves = legal(range);
    next = moves[0]?.length > 1 ? moves[0] : [hand[end]];
  }
  const combined = sort([...new Set([...selected, ...next])]);
  return pattern(combined) ? combined : sort(next);
}
