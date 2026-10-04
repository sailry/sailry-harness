import {getValue,prepareTransaction} from 'sailry/sdk';
import {head,write} from './storage.js';
import {clone,revision} from './model.js';
import {fail} from './policy.js';

export const settingsKey = 'memory/settings';
const defaults = {enabled:true,auto_write:true,context_bytes:8192,review_after_days:90};
function validate(value) {
  revision(value.revision);
  if (typeof value.enabled !== 'boolean' || typeof value.auto_write !== 'boolean'
    || !Number.isInteger(value.context_bytes) || value.context_bytes < 2048 || value.context_bytes > 65536
    || !Number.isInteger(value.review_after_days) || value.review_after_days < 1 || value.review_after_days > 3650) fail('invalid memory settings');
  return value;
}
export function settings(record) {
  if (!record.present) return {revision:0,...defaults};
  if (!record.value || record.value.v !== 1 || !/^(0|[1-9][0-9]*)$/.test(record.revision)) fail('memory settings are incompatible');
  return validate({revision:Number(record.revision),...clone(record.value.settings)});
}
export async function readMemorySettings() { return settings(await getValue(settingsKey)); }
export async function prepareMemorySettings(value) {
  validate(value);
  const current = await head(), stored = clone(value);
  delete stored.revision;
  // Settings changes invalidate a mutation already waiting on approval or admission.
  return prepareTransaction([write(current.record,current.value),
    {kind:'write',data:{key:settingsKey,value:{v:1,settings:stored},expected_revision:String(value.revision)}}]);
}
export function settingsOutput(output) {
  if (output?.kind !== 'plugin_transaction' || !Array.isArray(output.data)) fail('memory settings outcome is unavailable','outcome_unknown');
  const saved = output.data.find(value => value.kind === 'plugin_value' && value.data?.key === settingsKey);
  if (!saved) fail('memory settings outcome is unavailable','outcome_unknown');
  return settings(saved.data);
}
