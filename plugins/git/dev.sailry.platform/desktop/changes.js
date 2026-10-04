// Change navigation from Sailry ef5e82ea, retaining the approved grouping order.
export function isNew(entry) { return entry.untracked || entry.staged === 'added'; }

export function belongs(entry, group) {
  if (group === 'staged') return entry.staged !== null;
  if (group === 'unstaged') return entry.unstaged !== null || entry.untracked || entry.conflicted;
  if (group === 'tracked') return !isNew(entry);
  if (group === 'untracked') return isNew(entry);
  return true;
}

export function indexPaths(entries, operation) {
  return entries.filter(entry => !entry.conflicted && (operation === 'stage'
    ? entry.unstaged !== null || entry.untracked : entry.staged !== null)).map(entry => entry.path);
}

export function checkbox(entries) {
  const checked = entries.length > 0 && entries.every(entry => entry.staged !== null && entry.unstaged === null && !entry.untracked);
  return {checked, mixed:!checked && entries.some(entry => entry.staged !== null),
    operation:checked ? 'unstage' : 'stage', paths:indexPaths(entries,checked ? 'unstage' : 'stage')};
}

export function selectedId(entry, grouping, scope) {
  if (!entry) return null;
  const staged = entry.staged !== null && entry.unstaged === null && !entry.untracked && !entry.conflicted;
  const prefix = grouping === 'none' ? 'all' : grouping === 'tracked' ? (isNew(entry) ? 'untracked' : 'tracked')
    : scope === 'staged' || (scope === 'all' && staged) ? 'staged' : 'unstaged';
  return `${prefix}/${entry.path}`;
}

function compare(left, right) { return left < right ? -1 : left > right ? 1 : 0; }
function basename(path) { return path.split('/').at(-1); }
function statusOrder(change) { return change === null ? 'None' : `Some(${change.split('_').map(word => word[0].toUpperCase() + word.slice(1)).join('')})`; }

function directories(prefix, parent, entries) {
  const folders = new Map(), files = [];
  for (const entry of entries) {
    const relative = entry.path.slice(parent.length), separator = relative.indexOf('/');
    if (separator < 0) files.push(entry);
    else {
      const folder = relative.slice(0,separator);
      if (!folders.has(folder)) folders.set(folder,[]);
      folders.get(folder).push(entry);
    }
  }
  return [...folders].sort(([left],[right]) => compare(left,right)).map(([label,children]) => ({
    id:`dir/${prefix}/${parent}${label}`,label,expanded:true,
    children:directories(prefix,`${parent}${label}/`,children),
  })).concat(files.sort((left,right) => compare(left.path,right.path)).map(entry => ({
    id:`${prefix}/${entry.path}`,label:basename(entry.path),entry,
  })));
}

export function rows(status, options, text) {
  const groups = options.grouping === 'none' ? [['all','git_all_changes']]
    : options.grouping === 'staged' ? [['staged','git_staged'],['unstaged','git_unstaged']]
    : [['tracked','git_tracked'],['untracked','git_untracked']];
  const output = [];
  for (const [prefix,key] of groups) {
    let entries = (status?.entries ?? []).filter(entry => belongs(entry,prefix));
    if (!entries.length) continue;
    if (options.sort === 'name') entries.sort((left,right) => compare(basename(left.path),basename(right.path)) || compare(left.path,right.path));
    if (options.sort === 'status') entries.sort((left,right) => Number(left.conflicted) - Number(right.conflicted)
      || Number(left.untracked) - Number(right.untracked)
      || compare(`${statusOrder(left.staged)}${statusOrder(left.unstaged)}`,`${statusOrder(right.staged)}${statusOrder(right.unstaged)}`)
      || compare(left.path,right.path));
    const children = options.hierarchical ? directories(prefix,'',entries)
      : entries.map(entry => ({id:`${prefix}/${entry.path}`,label:basename(entry.path),entry}));
    if (options.grouping === 'none') output.push(...children);
    else output.push({id:prefix,label:`${text[key]} · ${entries.length}`,expanded:true,children,entries});
  }
  return output;
}
