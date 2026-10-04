import {getValue,searchValues} from 'sailry/sdk';
import {catalog,clone,identifier,summary,revision,validate,limits} from './model.js';
import {fail} from './policy.js';

export const headKey = 'memory/catalog';
export const entryPrefix = 'memory/entry/';
export const bodyKey = page => `memory/body/${page}`;
export const scopeTag = project => `scope:${project ?? 'global'}`;
export const visible = (summary,project) => project === undefined || summary.project === null || summary.project === project;
export const entryKey = id => entryPrefix+identifier(id);
export const write = (record,value) => ({kind:'write',data:{key:record.key,value,expected_revision:record.revision}});
export const remove = record => ({kind:'remove',data:{key:record.key,expected_revision:record.revision}});

export async function head() {
  const record = await getValue(headKey);
  return {record,value:catalog(record.present ? record.value : null)};
}
export function metadata(record) {
  if (!record.present) fail('memory does not exist','not_found');
  const value = record.value;
  if (!value || value.v !== 1 || !/^(0|[1-9][0-9]*)$/.test(value.body_page)
    || typeof value.hash !== 'string' || !/^[0-9a-f]{16}$/.test(value.hash)
    || !record.key.startsWith(entryPrefix)) fail('memory storage is incompatible');
  summary(value.summary);
  revision(value.body_revision);
  if (value.body_revision < 1 || value.body_revision > value.summary.revision) fail('memory storage is incompatible');
  if (entryKey(value.summary.id) !== record.key) fail('memory storage is incompatible');
  return clone(value);
}
export async function record(id,project) {
  const current = await getValue(entryKey(id));
  if (current.present && !visible(metadata(current).summary,project)) fail('memory belongs to another project','permission_denied');
  return current;
}
export async function page(id,expectedCount) {
  const record = await getValue(bodyKey(id));
  const value = record.present ? record.value : {v:1,bodies:{}};
  if (!value || value.v !== 1 || !value.bodies || Array.isArray(value.bodies) || typeof value.bodies !== 'object'
    || Object.keys(value.bodies).length > limits.page
    || (expectedCount !== undefined && Object.keys(value.bodies).length !== expectedCount)
    || Object.entries(value.bodies).some(([id,content]) => !content || typeof content.body !== 'string'
      || !Number.isSafeInteger(content.revision) || content.revision < 1 || entryKey(id).length === 0)) fail('memory storage is incompatible');
  return {record,value:clone(value)};
}
export async function body(value,source) {
  source ??= await page(value.body_page);
  if (!(value.summary.id in source.value.bodies)) fail('memory storage is incompatible');
  const content = source.value.bodies[value.summary.id];
  if (content.revision !== value.body_revision) fail('memory body changed','revision_conflict');
  return validate({summary:clone(value.summary),body:content.body});
}
export async function readMemory(id,project) {
  return body(metadata(await record(id,project)));
}
export async function clock() {
  const result = await searchValues({terms:[],all:['state:clock'],any:[],offset:0,limit:1});
  if (!Number.isSafeInteger(result.now_ms) || result.now_ms < 0) fail('memory clock is unavailable');
  return result.now_ms;
}
export async function indexed(filter,limit = Infinity) {
  const values = [];
  let offset = 0;
  do {
    const result = await searchValues({...filter,offset,limit:Math.min(100,limit-values.length)});
    for (const entry of result.entries) { metadata(entry); values.push(entry); }
    if (values.length >= limit || result.next === null) break;
    if (!Number.isSafeInteger(result.next) || result.next <= offset) fail('memory search cursor is invalid');
    offset = result.next;
  } while (true);
  return values;
}
