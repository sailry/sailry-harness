// Original generated artwork; UI surfaces and controls use the application theme.
const root = "dev.sailry.platform/desktop/assets";
const buildings = {
  1: "01-canal-walk", 2: "02-lockside", 3: "03-waterfront",
  5: "05-market-lane", 6: "06-foundry", 7: "07-arcade",
  9: "09-orchard", 10: "10-greenway", 11: "11-botanical",
  13: "13-west-station", 14: "14-terminal", 15: "15-platform",
  17: "17-marina", 18: "18-pier", 19: "19-lighthouse",
  21: "21-atrium", 22: "22-terrace", 23: "23-summit",
};
export const districts = ["#67aacf", "#da9658", "#83b46f", "#b48acb", "#65b8b0", "#c9ae62"];
export const playerColors = ["#df904e", "#629dd5", "#ad82c1", "#79a37e", "#c5a061", "#ce887e"];
export const building = tile => `${root}/buildings/${buildings[tile.art] || `special-${tile.kind === "park" ? "bonus" : tile.kind}`}.png`;
export const tileName = (text, tile) => tile.kind === "park" ? text.kinds.park : text.tiles[tile.art];
export const character = (index, portrait = false) => `${root}/characters/${portrait ? "portraits" : "bodies"}/character-${String(index + 1).padStart(2, "0")}.png`;
export const diceImage = (value, frame = null, settling = false) => frame === null
  ? `${root}/dice/outcomes/${value}/idle.png`
  : `${root}/dice/${settling ? `outcomes/${value}` : "roll"}/${String(frame).padStart(2, "0")}.png`;
export const diceShadow = `${root}/dice/shadow.png`;
export const townPattern = `${root}/interface/town-pattern.png`;
export const actionSheet = `${root}/interface/actions.png`;
export const currencies = { cash: `${root}/interface/coin.png`, worth: `${root}/interface/gem.png` };
export const validCharacters = value => Array.isArray(value) && value.length === 3
  && value.every(index => Number.isInteger(index) && index >= 0 && index < 6)
  && new Set(value).size === 3;
