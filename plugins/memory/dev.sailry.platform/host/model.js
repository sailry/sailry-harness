import {bytes,fail} from './policy.js';

export const limits = {active:512,archived:2048,body:8192,title:160,query:512,sources:16,page:2};
const uuid = /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i;
const whitespace = /[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/u;
export const normalized = text => text.split(whitespace).filter(Boolean).join(' ').toLowerCase();
export const clone = value => JSON.parse(JSON.stringify(value));
export const ordered = (left,right) => right.updated_at_ms-left.updated_at_ms || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0);

export function identifier(value) {
  if (typeof value !== 'string' || !uuid.test(value)) fail('invalid memory ID');
  return value.toLowerCase();
}
export function revision(value) {
  if (!Number.isSafeInteger(value) || value < 0) fail('invalid memory revision');
  return value;
}
export function summary(summary) {
  if (!summary || typeof summary.title !== 'string' || !normalized(summary.title)
    || bytes(summary.title) > limits.title || /\p{Cc}/u.test(summary.title)
    || typeof summary.archived !== 'boolean' || !['user','feedback','project','reference'].includes(summary.kind)
    || !Number.isSafeInteger(summary.updated_at_ms) || summary.updated_at_ms < 0) fail('invalid memory entry');
  identifier(summary.id);
  if (summary.project !== null) identifier(summary.project);
  revision(summary.revision);
  return summary;
}
export function validate(entry) {
  summary(entry?.summary);
  if (typeof entry.body !== 'string' || !normalized(entry.body) || bytes(entry.body) > limits.body || entry.body.includes('\0')) fail('invalid memory entry');
  return entry;
}
export function next(entry,now = Date.now()) {
  const saved = clone(validate(entry));
  if (saved.summary.revision === Number.MAX_SAFE_INTEGER) fail('memory revision exhausted');
  saved.summary.id = identifier(saved.summary.id);
  if (saved.summary.project !== null) saved.summary.project = identifier(saved.summary.project);
  saved.summary.revision++;
  saved.summary.updated_at_ms = now;
  return saved;
}
export function equivalence(body) {
  let first = 0x811c9dc5, second = 0x9e3779b9;
  for (const character of normalized(body)) {
    const point = character.codePointAt(0);
    first = Math.imul(first^point,0x01000193) >>> 0;
    second = Math.imul(second^point,0x85ebca6b) >>> 0;
  }
  return `${first.toString(16).padStart(8,'0')}${second.toString(16).padStart(8,'0')}`;
}
export function catalog(value) {
  if (value === null) return {v:1,pages:{},active:0,archived:0,next:0};
  if (!value || value.v !== 1 || !value.pages || Array.isArray(value.pages) || typeof value.pages !== 'object'
    || Object.keys(value.pages).length > Math.ceil((limits.active+limits.archived)/limits.page)
    || Object.entries(value.pages).some(([page,count]) => !/^(0|[1-9][0-9]*)$/.test(page) || !Number.isInteger(count) || count < 1 || count > limits.page)
    || !Number.isSafeInteger(value.next) || value.next < 0
    || !Number.isSafeInteger(value.active) || value.active < 0 || value.active > limits.active
    || !Number.isSafeInteger(value.archived) || value.archived < 0 || value.archived > limits.archived
    || Object.values(value.pages).reduce((total,count)=>total+count,0) !== value.active+value.archived) fail('memory storage is incompatible');
  return clone(value);
}
export function capacity(head,changes) {
  const counts = {active:head.active,archived:head.archived};
  for (const {before,after} of changes) {
    if (before) counts[before.archived ? 'archived' : 'active']--;
    if (after) counts[after.archived ? 'archived' : 'active']++;
  }
  if (counts.active > limits.active || counts.archived > limits.archived) fail('memory catalog is full','busy');
  if (counts.active < 0 || counts.archived < 0) fail('memory storage is incompatible');
  return counts;
}
