// Worktree action policy adapted from Sailry ef5e82ea's native worktree manager.
export function label(entry, project) {
  return entry.main ? project : entry.branch || entry.head?.slice(0,8)
    || entry.path.split(/[\\/]/).filter(Boolean).at(-1) || entry.path;
}
export function listing(catalog, found, location, text) {
  const project = catalog.projects.find(project => project.id === location.project);
  const entries = [...found.entries];
  const git = found.kind !== 'directory';
  if (!git) entries.push(...catalog.worktrees.filter(tree => tree.main).map(tree => ({...tree,branch:null,head:null,locked:false,available:true})));
  const choices = entries.map((entry,index) => {
    const registered = catalog.worktrees.find(tree => tree.path === entry.path);
    const prune = !entry.available && !entry.main && !entry.locked;
    const actions = [];
    if (entry.available) actions.push({kind:'open',entry,worktree:registered?.id});
    if (entry.available && !entry.main && !entry.locked && entry.branch && entry.head) actions.push({kind:'remove',entry,worktree:registered?.id});
    return {id:`entry-${index}`,label:label(entry,project?.name ?? location.project_name),
      icon:entry.main ? 'folder' : 'network',group:text('workspace_worktrees'),
      checked:registered?.id === location.worktree,
      detail:prune ? text('worktree_prune_on_select') : !entry.available ? text('worktree_unavailable')
        : entry.locked ? text('worktree_locked') : entry.main ? entry.branch ?? '' : project?.name ?? location.project_name,
      disabled:!prune && actions.length === 0,
      action:prune ? {kind:'prune',entry} : {kind:'actions',entry,actions}};
  });
  if (location.surface === 'composer') {
    if (git) {
      choices.push({id:'create',label:text('location_create'),icon:'plus',group:text('workspace_session_actions'),action:{kind:'managed',fork:false}});
      if (location.session) choices.push({id:'fork',label:text('location_fork'),icon:'plus',group:text('workspace_session_actions'),action:{kind:'managed',fork:true}});
    }
    if (!location.session) choices.push({id:'projects',label:text('location_project'),group:text('workspace_session_actions'),action:{kind:'projects'}});
  } else if (entries.some(entry => entry.available && entry.head)) {
    const base = entries.find(entry => catalog.worktrees.some(tree => tree.id === location.worktree && tree.path === entry.path)) ?? entries.find(entry => entry.available && entry.head);
    choices.push({id:'create',label:text('worktree_create'),icon:'plus',group:text('worktree_actions'),action:{kind:'create',entry:base}});
  }
  if (found.truncated || Number(found.omitted_paths) > 0) choices.push({id:'partial',label:text('worktree_listing_partial'),disabled:true,action:null});
  return choices;
}
export function errorKey(code, removal = false) {
  if (removal && code === 'busy') return 'worktree_remove_busy';
  if (removal && ['conflict','revision_conflict'].includes(code)) return 'worktree_remove_conflict';
  return ({outcome_unknown:'worktree_outcome_unknown',conflict:'worktree_conflict',revision_conflict:'worktree_conflict',invalid_request:'worktree_invalid',not_found:'worktree_not_found'})[code] ?? 'worktree_request_failed';
}
export function current(bound, state) {
  return bound.cursor === state.cursor && bound.node === state.node && bound.project === state.project
    && bound.worktree === state.worktree && bound.session === state.session && state.can_move;
}
