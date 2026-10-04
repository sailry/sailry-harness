// Argument policy from Sailry ef5e82ea's native Git tools.
function object(value, keys) {
  return value !== null && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).every(key => keys.includes(key));
}

function target(value) { return value === undefined || value === null || typeof value === 'string'; }
function invalid() { return {isError:true,error:{code:'invalid_request',message:'Invalid Git arguments'}}; }

export function status(args) {
  if (!object(args,['worktree']) || !target(args.worktree)) return invalid();
  return {worktree:args.worktree ?? null};
}

export function diff(args) {
  if (!object(args,['worktree','path','scope']) || !target(args.worktree) ||
      typeof args.path !== 'string') return invalid();
  const scope = args.scope ?? 'all';
  if (!['all','staged','unstaged'].includes(scope) || args.scope === null) return invalid();
  return {worktree:args.worktree ?? null,path:args.path,scope};
}

export function log(args) {
  if (!object(args,['worktree','limit','cursor']) || !target(args.worktree)) return invalid();
  const {limit = 20,cursor = null} = args;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) return invalid();
  if (cursor !== null && (!object(cursor,['head','offset']) ||
      typeof cursor.head !== 'string' || !/^[0-9a-fA-F]{40}$/.test(cursor.head) ||
      !Number.isSafeInteger(cursor.offset) || cursor.offset < 0)) return invalid();
  return {worktree:args.worktree ?? null,limit,cursor};
}
