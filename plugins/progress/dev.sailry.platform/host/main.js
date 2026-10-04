// Task-plan policy from Sailry ef5e82ea; canonical tool history stays in core.
function bytes(value) {
  let length = 0;
  for (const character of value) {
    const point = character.codePointAt(0);
    length += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return length;
}

const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = (value,limit) => typeof value === 'string' && value.trim().length > 0 && bytes(value) <= limit;
const states = ['pending','in_progress','completed','skipped'];

export function update(args) {
  const invalid = () => ({isError:true,error:{code:'invalid_request',message:'Invalid task progress'}});
  if (!object(args) || Object.keys(args).some(key => !['title','steps'].includes(key)) ||
      !Object.hasOwn(args,'title') || args.title !== null && !text(args.title,512) ||
      !Array.isArray(args.steps) || args.steps.length < 1 || args.steps.length > 256) return invalid();
  let size = args.title === null ? 0 : bytes(args.title);
  for (const step of args.steps) {
    if (!object(step) || Object.keys(step).some(key => !['description','state'].includes(key)) ||
        !text(step.description,8192) || !states.includes(step.state)) return invalid();
    size += bytes(step.description);
  }
  if (size > 64 * 1024) return invalid();
  return {progress:{title:args.title,steps:args.steps.map(step => ({description:step.description,state:step.state}))}};
}
