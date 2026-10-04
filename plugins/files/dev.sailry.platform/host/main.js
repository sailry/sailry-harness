// Argument policy from Sailry 5bfeaf05 plugins/builtin/files/agent and office/agent.rs.
// Confined file access, approval, revisions and execution remain in the Node.

function object(value, keys) {
  return value !== null && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).every(key => keys.includes(key));
}

function invalid() {
  return {isError:true,error:{code:'invalid_request',message:'Invalid file tool arguments'}};
}

function revision(value) {
  return value === undefined || value === null || typeof value === 'string';
}

export function list(args) {
  if (!object(args,['path','cursor']) ||
      (args.path !== undefined && typeof args.path !== 'string')) return invalid();
  const {path = '',cursor = null} = args;
  if (cursor !== null && (!object(cursor,['revision','directory','name']) ||
      typeof cursor.revision !== 'string' || typeof cursor.directory !== 'boolean' ||
      typeof cursor.name !== 'string')) return invalid();
  return {path:path === '.' ? '' : path,after:cursor};
}

export function read(args) {
  if (!object(args,['path']) || typeof args.path !== 'string') return invalid();
  return {path:args.path};
}

export function write(args) {
  if (!object(args,['path','text','expected_revision']) || typeof args.path !== 'string' ||
      typeof args.text !== 'string' || !revision(args.expected_revision)) return invalid();
  return {path:args.path,text:args.text,expected_revision:args.expected_revision ?? null};
}

export function search(args) {
  if (!object(args,['query','regex','case_sensitive','globs'])) return invalid();
  const {query,regex = false,case_sensitive = false,globs = []} = args;
  if (typeof query !== 'string' || typeof regex !== 'boolean' ||
      typeof case_sensitive !== 'boolean' || !Array.isArray(globs) ||
      globs.some(glob => typeof glob !== 'string')) return invalid();
  return {query,regex,case_sensitive,globs};
}

export function runtime(args) {
  return object(args,[]) ? {} : invalid();
}

export function office(args) {
  if (!object(args,['path','offset'])) return invalid();
  const {path,offset = 0} = args;
  if (typeof path !== 'string' || !Number.isSafeInteger(offset) || offset < 0) return invalid();
  return {path,offset};
}

export function pdf(args) {
  if (!object(args,['source','path','expected_revision']) || typeof args.source !== 'string' ||
      typeof args.path !== 'string' || !revision(args.expected_revision)) return invalid();
  return {source:args.source,path:args.path,expected_revision:args.expected_revision ?? null};
}
