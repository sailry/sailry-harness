import {captions} from './locales.js';

function object(args, keys) {
  return args && typeof args === 'object' && !Array.isArray(args) &&
    Object.keys(args).every(key => keys.includes(key));
}

function invalid(message) {
  return {isError:true,error:{code:'invalid_request',message}};
}

export function run(args) {
  if (!object(args, ['command', 'cwd', 'timeout_ms', 'background', 'attachments']))
    return invalid('Invalid command arguments');
  const { command, cwd = '', timeout_ms = 120000, background = false, attachments = [] } = args;
  if (typeof command !== 'string' || typeof cwd !== 'string' || typeof background !== 'boolean' ||
      !Number.isSafeInteger(timeout_ms) || timeout_ms < 1 || timeout_ms > 900000 ||
      !Array.isArray(attachments) || attachments.length > 8 || attachments.some(id => typeof id !== 'string') ||
      new Set(attachments).size !== attachments.length) {
    return invalid('Invalid command arguments');
  }
  return { command, cwd, timeout_ms, background, attachments };
}

export function read(args) {
  if (!object(args, ['id']) || (args.id !== undefined && typeof args.id !== 'string'))
    return invalid('Invalid command id');
  return args;
}

export function stop(args) {
  if (!object(args, ['id']) || typeof args.id !== 'string' || !args.id)
    return invalid('Command id is required');
  return args;
}

export function result({ output }) {
  const outcome = output.kind === 'command_result' ? output.data.outcome.kind : null;
  let blocks;
  if (output.kind === 'command_result' || output.kind === 'command_output') {
    blocks = ['stdout', 'stderr'].map(stream => {
      const capture = output.data[stream], notices = [];
      if (capture.truncated) notices.push(captions.truncated);
      if (capture.invalid_utf8) notices.push(captions.encoding);
      return {kind:'text',path:`/data/${stream}/text`,notices};
    });
  } else if (output.kind === 'command_started') {
    blocks = [{kind:'notice',message:captions.started}];
  }
  const result = blocks ? {...output,sailry_content:{version:1,blocks}} : output;
  // A nonzero exit is still a completed command with inspectable output.
  if (outcome === 'signal' || outcome === 'timed_out' || outcome === 'unknown') return { ...result, isError: true };
  return result;
}
