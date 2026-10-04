// Tool descriptions, schemas and approval groups are adapted from Sailry c4bb60a2:
// plugins/builtin/browser/agent.rs. Native browser execution remains in the core.
import {captions} from './locales.js';

function utf8Length(value) {
  let bytes = 0;
  for (const character of value) {
    const code = character.codePointAt(0);
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return bytes;
}

function prepare(action, args) {
  if (!args || typeof args !== 'object' || Array.isArray(args)) {
    return {isError:true,error:{code:'invalid_request',message:'browser arguments must be an object'}};
  }
  if (action === 'wait') {
    const {condition, timeout_ms} = args;
    const query = condition?.kind === 'text' ? condition.text
      : ['visible', 'hidden'].includes(condition?.kind) ? condition.selector : null;
    if (!Number.isInteger(timeout_ms) || timeout_ms < 1 || timeout_ms > 20000 ||
        typeof query !== 'string' || !query.length || utf8Length(query) > 2000) {
      return {isError:true,error:{code:'invalid_request',message:'invalid browser wait condition or timeout'}};
    }
  }
  return {action, arguments:args};
}

export function tabs(args) { return prepare('tabs', args); }
export function read(args) { return prepare('read', args); }
export function navigate(args) { return prepare('navigate', args); }
export function click(args) { return prepare('click', args); }
export function input(args) { return prepare('input', args); }
export function scroll(args) { return prepare('scroll', args); }
export function back(args) { return prepare('back', args); }
export function forward(args) { return prepare('forward', args); }
export function refresh(args) { return prepare('refresh', args); }
export function open(args) { return prepare('open', args); }
export function close(args) { return prepare('close', args); }
export function focus(args) { return prepare('focus', args); }
export function select(args) { return prepare('select', args); }
export function hover(args) { return prepare('hover', args); }
export function key(args) { return prepare('key', args); }
export function frame(args) { return prepare('frame', args); }
export function wait(args) { return prepare('wait', args); }
export function screenshot(args) { return prepare('screenshot', args); }

export function result({output}) {
  const message = captions[output.error?.message];
  if (message) return {sailry_content:{version:1,blocks:[{kind:'notice',message}]}};
  if (output.isError) return output;
  return output.data;
}
