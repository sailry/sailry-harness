// Worktree workflows originate in Sailry ef5e82ea's native feature package.
// Core resolves the admitted turn's project and checks the selected resource.
function object(value, keys) {
  return value !== null && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).every(key => keys.includes(key));
}
function invalid() { return {isError:true,error:{code:'invalid_request',message:'Invalid worktree arguments'}}; }

export function list(args) { return object(args,[]) ? {} : invalid(); }

export function create(args) {
  if (!object(args,['branch','expected_head','expected_index','include_changes']) ||
      typeof args.branch !== 'string' || typeof args.expected_head !== 'string' ||
      typeof args.expected_index !== 'string') return invalid();
  const include_changes = args.include_changes ?? false;
  if (typeof include_changes !== 'boolean' || args.include_changes === null) return invalid();
  return {branch:args.branch,expected_head:args.expected_head,
    expected_index:args.expected_index,include_changes};
}

export function register(args) {
  return object(args,['path']) && typeof args.path === 'string' ? {path:args.path} : invalid();
}

export function remove(args) {
  if (!object(args,['worktree','expected_head','expected_branch']) ||
      typeof args.worktree !== 'string' || typeof args.expected_head !== 'string' ||
      typeof args.expected_branch !== 'string') return invalid();
  return {worktree:args.worktree,expected_head:args.expected_head,expected_branch:args.expected_branch};
}
