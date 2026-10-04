export const ranks = ["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"];
export const suits = ["♠", "♥", "♣", "♦"];

export function rank(card) { return Math.floor(card / 4) + 2; }
export function suit(card) { return card % 4; }
export function label(card) { return `${ranks[rank(card) - 2]}${suits[suit(card)]}`; }
export function red(card) { return suit(card) === 1 || suit(card) === 3; }

function straight(values) {
  const unique = [...new Set(values)].sort((a, b) => b - a);
  if (unique.includes(14)) unique.push(1);
  for (let index = 0; index <= unique.length - 5; index++) {
    if (unique[index] - unique[index + 4] === 4) return unique[index];
  }
  return 0;
}

function five(cards) {
  const values = cards.map(rank).sort((a, b) => b - a);
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) || 0) + 1);
  const groups = [...counts].sort((a, b) => b[1] - a[1] || b[0] - a[0]);
  const flush = cards.every(card => suit(card) === suit(cards[0]));
  const run = straight(values);
  if (flush && run) return [8, run];
  if (groups[0][1] === 4) return [7, groups[0][0], groups[1][0]];
  if (groups[0][1] === 3 && groups[1][1] === 2) return [6, groups[0][0], groups[1][0]];
  if (flush) return [5, ...values];
  if (run) return [4, run];
  if (groups[0][1] === 3) return [3, groups[0][0], ...groups.slice(1).map(group => group[0]).sort((a, b) => b - a)];
  if (groups[0][1] === 2 && groups[1][1] === 2) {
    const pairs = [groups[0][0], groups[1][0]].sort((a, b) => b - a);
    return [2, ...pairs, groups[2][0]];
  }
  if (groups[0][1] === 2) return [1, groups[0][0], ...groups.slice(1).map(group => group[0]).sort((a, b) => b - a)];
  return [0, ...values];
}

export function compare(left, right) {
  for (let index = 0; index < Math.max(left.length, right.length); index++) {
    const difference = (left[index] || 0) - (right[index] || 0);
    if (difference) return Math.sign(difference);
  }
  return 0;
}

export function evaluate(cards) {
  if (cards.length < 5 || cards.length > 7 || new Set(cards).size !== cards.length
    || cards.some(card => !Number.isInteger(card) || card < 0 || card >= 52)) throw new Error("Invalid poker cards");
  let best = null;
  for (let a = 0; a < cards.length - 4; a++)
    for (let b = a + 1; b < cards.length - 3; b++)
      for (let c = b + 1; c < cards.length - 2; c++)
        for (let d = c + 1; d < cards.length - 1; d++)
          for (let e = d + 1; e < cards.length; e++) {
            const value = five([cards[a], cards[b], cards[c], cards[d], cards[e]]);
            if (!best || compare(value, best) > 0) best = value;
          }
  return best;
}
