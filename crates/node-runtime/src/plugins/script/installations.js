// Bounded callbacks do not retain a live subscription beyond their execution budget.
import {modelCall as bridge} from 'sailry/sdk';
export async function read(ids) { return bridge('read', {kind:'read_installations',data:{requests:ids}}).data; }
export function watch(ids) { throw unavailable(); }
export async function next() { throw unavailable(); }
export async function dismiss(id) { bridge('read', {kind:'dismiss_installation',data:{request:id}}); }
function unavailable() { const error = new Error('live installation observation is unavailable in this callback'); error.code = 'unavailable'; return error; }
