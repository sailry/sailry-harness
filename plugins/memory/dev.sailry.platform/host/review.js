import {fail} from './policy.js';
import {terms} from './tokenize.js';

function fingerprint(text) {
  // Packed exact tokens keep the full catalog within the callback's memory limit.
  // No hash collisions may turn distinct facts into duplicate hints.
  const tokens = Array.from(new Set(terms(text))).sort(), offsets = new Uint32Array(tokens.length);
  let offset = 0;
  for (let index = 0; index < tokens.length; index++) {
    offsets[index] = offset;
    offset += tokens[index].length + 1;
  }
  return {text:tokens.join('\0')+'\0',offsets};
}

function compare(left, start, right, other) {
  while (true) {
    const a = left.charCodeAt(start++), b = right.charCodeAt(other++);
    if (a !== b) return a-b;
    if (a === 0) return 0;
  }
}

function duplicate(left, right) {
  if (!left.offsets.length || !right.offsets.length) return false;
  const required = Math.ceil(4*(left.offsets.length+right.offsets.length)/9);
  let a = 0, b = 0, common = 0;
  while (a < left.offsets.length && b < right.offsets.length) {
    if (common + Math.min(left.offsets.length-a,right.offsets.length-b) < required) return false;
    const order = compare(left.text,left.offsets[a],right.text,right.offsets[b]);
    if (order <= 0) a++;
    if (order >= 0) b++;
    if (order === 0 && ++common >= required) return true;
  }
  return false;
}

export async function review(summaries, settings, filter, read, now = Date.now()) {
  const selected = summaries.filter(summary => !summary.archived
    && (filter.all_projects || summary.project === null || summary.project === filter.project))
    .sort((left,right) => right.updated_at_ms-left.updated_at_ms || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0));
  const fingerprints = new Array(selected.length);
  let next = 0;
  await Promise.all(Array.from({length:Math.min(4,selected.length)},async () => {
    while (next < selected.length) {
      const index = next++, expected = selected[index], entry = await read(expected.id);
      if (entry.summary.id !== expected.id || entry.summary.revision !== expected.revision
        || entry.summary.project !== expected.project || entry.summary.archived !== expected.archived) {
        fail('memory revision changed during review','revision_conflict');
      }
      fingerprints[index] = fingerprint(entry.body);
    }
  }));
  const candidates = [];
  for (let index = 0; index < selected.length; index++) {
    const summary = selected[index];
    const previous = selected.slice(0,index).findIndex((other,otherIndex) => other.project === summary.project
      && duplicate(fingerprints[otherIndex],fingerprints[index]));
    const stale = Math.max(0,now-summary.updated_at_ms) >= settings.review_after_days*86_400_000;
    if (previous !== -1 || stale) candidates.push({summary,duplicate_of:previous === -1 ? null : selected[previous].id,stale});
  }
  return candidates;
}
