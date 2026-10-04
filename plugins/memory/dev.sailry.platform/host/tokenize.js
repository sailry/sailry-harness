const cjk = character => {
  const point = character.codePointAt(0);
  return (point >= 0x3400 && point <= 0x4dbf) || (point >= 0x4e00 && point <= 0x9fff)
    || (point >= 0xf900 && point <= 0xfaff) || (point >= 0x20000 && point <= 0x323af);
};
const alphanumeric = /[\p{Alphabetic}\p{Number}]/u;
const uppercase = /\p{Uppercase}/u;
const lowercase = /\p{Lowercase}/u;

export function terms(text) {
  const tokens = [];
  let word = [], previous = null;
  const flush = () => {
    if (!word.length) return;
    let start = 0, split = false;
    for (let index = 1; index < word.length; index++) {
      if (uppercase.test(word[index]) && (lowercase.test(word[index-1])
        || (uppercase.test(word[index-1]) && lowercase.test(word[index+1] ?? '')))) {
        tokens.push(word.slice(start,index).join('').toLowerCase());
        start = index; split = true;
      }
    }
    if (split) tokens.push(word.slice(start).join('').toLowerCase());
    tokens.push(word.join('').toLowerCase());
    word = [];
  };
  for (const character of text) {
    if (cjk(character)) {
      flush(); tokens.push(character);
      if (previous !== null) tokens.push(`${previous}${character}`);
      previous = character;
    } else {
      previous = null;
      if (alphanumeric.test(character) || (word.length && ['+','#'].includes(character))) word.push(character);
      else flush();
    }
  }
  flush();
  return tokens;
}

const stopwords = new Set(['a','an','and','are','as','at','be','by','do','does','for','from','how','i','in','is','it',
  'of','on','or','our','that','the','these','this','to','was','we','what','when','where','which','with','you',
  '\u5982\u4f55','\u4ec0\u4e48','\u600e\u4e48','\u6211\u4eec','\u8fd9\u4e2a','\u54ea\u4e9b','\u662f\u5426']);

export function query(text) {
  const selected = Array.from(new Set(terms(text)));
  const bigram = selected.some(term => Array.from(term).length === 2 && Array.from(term).every(cjk));
  return selected.filter(term => !stopwords.has(term)
    && !(bigram && Array.from(term).length === 1 && cjk(term))).sort();
}
