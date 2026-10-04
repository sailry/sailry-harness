export const SEATS = 3;
export const START_CASH = 1200;
export const PASS_START = 200;
export const BONUS = 150;
export const ROUND_LIMIT = 20;
export const MAX_LEVEL = 3;
export const BID_INCREMENT = 20;

const specials = new Map([
  [0, { kind: "start" }],
  [4, { kind: "event" }],
  [8, { kind: "tax", amount: 100 }],
  [12, { kind: "bonus", amount: BONUS }],
  [16, { kind: "event" }],
  [20, { kind: "tax", amount: 150 }],
]);

let property = 0;
export const CATALOG = Object.freeze(Array.from({ length: 24 }, (_, index) => {
  const special = specials.get(index);
  if (special) return Object.freeze({ art: index, key: `tile.${index}`, ...special });
  const price = 120 + property * 20;
  const tile = { art: index, key: `tile.${index}`, kind: "property",
    group: Math.floor(property / 3), price, upgrade: price / 2,
    baseRent: price / 10 };
  property++;
  return Object.freeze(tile);
}));

export const EVENTS = Object.freeze([
  Object.freeze({ key: "event.grant", amount: 120 }),
  Object.freeze({ key: "event.repair", amount: -80 }),
  Object.freeze({ key: "event.reward", amount: 150 }),
  Object.freeze({ key: "event.fee", amount: -100 }),
]);
