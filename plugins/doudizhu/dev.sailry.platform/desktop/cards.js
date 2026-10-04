// Cards are stable IDs; ranks run from 3 through 2, then the two jokers.
export const rank = card => card < 52 ? Math.floor(card / 4) + 3 : card - 36;
export const sort = cards => [...cards].sort((a, b) => rank(b) - rank(a) || b - a);
export const label = card => card >= 52 ? (card === 52 ? "☆" : "★")
  : `${["3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A", "2"][Math.floor(card / 4)]}${["♠", "♥", "♣", "♦"][card % 4]}`;
export const red = card => card === 53 || (card < 52 && card % 2 === 1);

function groups(cards) {
  const result = new Map();
  for (const card of cards) {
    const value = rank(card);
    if (!result.has(value)) result.set(value, []);
    result.get(value).push(card);
  }
  return [...result].sort((a, b) => a[0] - b[0]);
}
const sequence = values => values.length > 0 && values.at(-1) < 15
  && values.every((value, index) => value === values[0] + index);

export function pattern(cards) {
  if (!cards.length || new Set(cards).size !== cards.length
    || cards.some(card => !Number.isInteger(card) || card < 0 || card > 53)) return null;
  const grouped = groups(cards), size = cards.length;
  const values = grouped.map(([value]) => value);
  const counts = grouped.map(([, cards]) => cards.length);
  const result = (kind, top, span = 1) => ({ kind, top, span, size });
  if (size === 2 && cards.includes(52) && cards.includes(53)) return result("rocket", 17);
  if (grouped.length === 1) return result([null, "single", "pair", "triple", "bomb"][size], values[0]);
  if (size === 4 && counts.includes(3)) return result("tripleSingle", values[counts.indexOf(3)]);
  if (size === 5 && counts.includes(3) && counts.includes(2)) return result("triplePair", values[counts.indexOf(3)]);
  if (size >= 5 && counts.every(count => count === 1) && sequence(values)) return result("straight", values.at(-1), size);
  if (size >= 6 && counts.every(count => count === 2) && sequence(values)) return result("pairs", values.at(-1), size / 2);
  // Wings never reuse a body rank. Single wings may contain a pair; pair wings
  // must be distinct pairs. Neither 2 nor jokers may occur in a sequence body.
  for (const [kind, width] of [["plane", 3], ["planeSingle", 4], ["planePair", 5]]) {
    const span = size / width;
    if (!Number.isInteger(span) || span < 2) continue;
    for (let start = 3; start + span - 1 <= 14; start++) {
      const body = Array.from({ length: span }, (_, index) => start + index);
      if (!body.every(value => grouped.some(([r, c]) => r === value && c.length === 3))) continue;
      const wings = grouped.filter(([value]) => !body.includes(value));
      if (kind === "plane" && wings.length) continue;
      if (kind === "planePair" && (wings.length !== span || wings.some(([, c]) => c.length !== 2))) continue;
      if (kind === "planeSingle" && wings.some(([, c]) => c.length > 2)) continue;
      return result(kind, start + span - 1, span);
    }
  }
  if (counts.includes(4)) {
    const top = values[counts.indexOf(4)];
    if (size === 6) return result("fourSingle", top);
    if (size === 8 && counts.filter(count => count === 2).length === 2) return result("fourPair", top);
  }
  return null;
}

export function beats(next, previous) {
  if (!next) return false;
  if (!previous) return true;
  if (previous.kind === "rocket") return false;
  if (next.kind === "rocket") return true;
  if (next.kind === "bomb" && previous.kind !== "bomb") return true;
  return next.kind === previous.kind && next.span === previous.span
    && next.size === previous.size && next.top > previous.top;
}

function choose(items, count, visit, chosen = [], start = 0) {
  if (!count) { visit(chosen); return; }
  for (let index = start; index <= items.length - count; index++) {
    choose(items, count - 1, visit, [...chosen, items[index]], index + 1);
  }
}

export function legal(hand, previous = null) {
  const grouped = groups(hand), found = new Map();
  const add = cards => {
    const shape = pattern(cards);
    if (!beats(shape, previous)) return;
    const key = cards.map(rank).sort((a, b) => a - b).join(",");
    if (!found.has(key)) found.set(key, sort(cards));
  };
  const wings = (body, singles, pairs) => {
    const excluded = new Set(body.map(rank));
    const rest = grouped.filter(([value]) => !excluded.has(value));
    if (singles) choose(rest.flatMap(([, cards]) => cards.slice(0, 2)), singles, tail => add([...body, ...tail]));
    if (pairs) choose(rest.filter(([, cards]) => cards.length >= 2), pairs,
      tail => add([...body, ...tail.flatMap(([, cards]) => cards.slice(0, 2))]));
  };
  for (const [, cards] of grouped) {
    for (let count = 1; count <= cards.length; count++) add(cards.slice(0, count));
    if (cards.length >= 3) wings(cards.slice(0, 3), 1, 1);
    if (cards.length === 4) wings(cards, 2, 2);
  }
  if (hand.includes(52) && hand.includes(53)) add([52, 53]);
  for (const [width, minimum] of [[1, 5], [2, 3], [3, 2]]) {
    for (let start = 3; start <= 14; start++) {
      const body = [];
      for (let end = start; end <= 14; end++) {
        const cards = grouped.find(([value]) => value === end)?.[1];
        if (!cards || cards.length < width) break;
        body.push(...cards.slice(0, width));
        const span = end - start + 1;
        if (span < minimum) continue;
        add(body);
        if (width === 3) wings(body, span, span);
      }
    }
  }
  return [...found.values()].sort((a, b) => b.length - a.length || rank(a[0]) - rank(b[0]));
}
