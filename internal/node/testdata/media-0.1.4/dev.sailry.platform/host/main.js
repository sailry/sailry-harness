// Policy from Sailry 5d0ac251 plugins/builtin/media/agent; execution stays in core.
import {newId} from 'sailry/sdk';

function invalid(message) { return {isError:true,error:{code:'invalid_request',message}}; }

function bytes(value) {
  let length = 0;
  for (const character of value) {
    const code = character.codePointAt(0);
    length += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return length;
}

function validate(args) {
  if (!args || typeof args !== 'object' || Array.isArray(args) ||
      Object.keys(args).some(key => !['prompt','path','attachment','file_name'].includes(key)) ||
      typeof args.prompt !== 'string' ||
      ['path','attachment','file_name'].some(key => Object.hasOwn(args,key) && typeof args[key] !== 'string')) {
    return invalid('invalid media arguments');
  }
  if (!args.prompt.trim() || bytes(args.prompt) > 32000) {
    return invalid('media prompt must be nonempty and bounded');
  }
  return null;
}

export function inspect(args) {
  const error = validate(args);
  if (error) return error;
  const path = Object.hasOwn(args,'path'), attachment = Object.hasOwn(args,'attachment');
  if (path === attachment || Object.hasOwn(args,'file_name')) return invalid('select one image source');
  return {prompt:args.prompt,source:{kind:path ? 'path' : 'attachment',value:path ? args.path : args.attachment}};
}

function generate(args, extension) {
  const error = validate(args);
  if (error) return error;
  if (Object.hasOwn(args,'path') || Object.hasOwn(args,'attachment')) {
    return invalid('generation does not accept input files');
  }
  const name = args.file_name ?? `${newId()}.${extension}`;
  if (!name.endsWith(`.${extension}`)) return invalid('media output has an unsupported extension');
  if (/[\\/:\u0000-\u001f\u007f-\u009f]/.test(name)) {
    return invalid('media output requires a plain file name');
  }
  return {prompt:args.prompt,path:`assets/generated/${name}`};
}

export function image(args) { return generate(args,'png'); }
export function video(args) { return generate(args,'mp4'); }
export function result({output}) { return output.isError ? output : output.data; }
