// Menu policy and immutable targets belong to the package; Kit owns the controls.
import {indexPaths} from './changes.js';
const separator = () => ({separator:true});
export class Menus {
  constructor(view) { this.view = view; }
  item(key,value,enabled = true,extra = {}) {
    return {id:key,label:this.view.text[key] ?? key,enabled,value,...extra};
  }
  run(key,action,enabled = true) { return this.item(key,{kind:'run',action},this.view.repo.writable() && enabled); }
  confirm(key,action,target,enabled = true) { return this.item(key,{kind:'confirm',title:key,action,target},this.view.repo.writable() && enabled); }
  choose(key,items) { return this.item(key,{kind:'choose',title:key,items},this.view.repo.writable() && items.length > 0); }
  section(key,children) { return this.item(key,null,true,{children}); }
  choice(label,value,extra = {}) { return {id:label,label,value,...extra}; }
  remote(action) {
    const sync = this.view.repo.branches?.sync;
    return (sync?.remotes ?? []).map(name => this.choice(name,action(name),{detail:sync.remote_urls[name] ?? ''}));
  }
  branches(operation = 'switch') {
    const repo = this.view.repo, text = this.view.text, sync = repo.branches?.sync;
    return (repo.branches?.entries ?? []).filter(branch => operation !== 'rename' || !branch.remote)
      .filter(branch => operation !== 'delete' || !branch.remote && !branch.current)
      .filter(branch => !['merge','rebase'].includes(operation) || !branch.current)
      .map(branch => this.choice(branch.name,{kind:'branch',operation,branch}, {
        checked:branch.current,icon:'network',group:text[branch.remote ? 'git_remote_branches' : 'git_local_branches'],
        detail:sync.tracking[branch.name] ?? branch.commit.slice(0,7)
      }));
  }
  branchActions() {
    const text = this.view.text, entries = this.view.repo.branches?.entries ?? [];
    return [this.choice(text.git_new_branch,{kind:'form',form:'branch'},{icon:'plus',group:text.git_branch_actions}),
      ...(entries.length ? [this.choice(text.git_branch_from,{kind:'choose',title:'git_branch_from',items:this.branches('create')},{icon:'network',group:text.git_branch_actions}),
        this.choice(text.git_copy_branch,{kind:'choose',title:'git_copy_branch',items:this.branches('copy')},{icon:'copy',group:text.git_branch_actions})] : [])];
  }
  checkout() {
    const repo = this.view.repo, items = this.branches();
    if (!items.length && repo.branches?.current) items.push(this.choice(repo.branches.current,{kind:'noop'},
      {checked:true,icon:'network',group:this.view.text.git_local_branches,detail:this.view.text.git_unborn}));
    return items.concat(this.branchActions());
  }
  management() {
    const repo = this.view.repo, text = this.view.text;
    return (repo.branches?.entries ?? []).map(branch => {
      const items = [['git_checkout','switch',!branch.current],['git_copy_branch','copy',true],['git_branch_from','create',true],
        ['git_rename_branch','rename',!branch.remote],['git_delete_branch','delete',!branch.remote && !branch.current],
        ['git_merge_branch','merge',!branch.current],['git_rebase','rebase',!branch.current]]
        .filter(([, ,enabled]) => enabled).map(([key,operation]) => this.choice(text[key],{kind:'branch',operation,branch}));
      if (branch.remote) items.push(this.choice(text.git_delete_remote_branch,{kind:'confirm',title:'git_delete_remote_branch',target:branch.name,
        action:{delete_remote_branch:{branch:branch.name,commit:branch.commit}}}));
      if (this.view.worktrees) items.push(this.choice(text.worktree_create,{kind:'worktree',branch:branch.name,commit:branch.commit}));
      return this.choice(branch.name,{kind:'choose',title:branch.name,literal:true,items}, {
        icon:'network',checked:branch.current,group:text[branch.remote ? 'git_remote_branches' : 'git_local_branches'],
        detail:repo.branches.sync.tracking[branch.name] ?? branch.commit.slice(0,7)
      });
    }).concat(this.branchActions());
  }
  push(force = false) {
    const repo = this.view.repo, key = force ? 'git_force_push' : 'git_push', sync = repo.branches?.sync;
    const action = remote => ({push:{remote,publish:!sync?.upstream,force}});
    const value = remote => force ? {kind:'confirm',title:key,target:remote ?? sync.upstream,action:action(remote)} : {kind:'run',action:action(remote)};
    return sync?.upstream ? this.item(key,value(null),repo.writable() && !!repo.status?.head)
      : this.choose(key,this.remote(value));
  }
  viewOptions() {
    const current = this.view.repo.navigation;
    return [['git_layout_list','hierarchical',false],['git_layout_tree','hierarchical',true],
      ['git_sort_path','sort','path'],['git_sort_name','sort','name'],['git_sort_status','sort','status'],
      ['git_group_none','grouping','none'],['git_group_tracked','grouping','tracked'],['git_group_staged','grouping','staged']]
      .flatMap(([key,field,value],index) => [...([2,5].includes(index) ? [separator()] : []),
        this.item(key,{kind:'view',field,value},true,{checked:current[field] === value})]);
  }
  commits() {
    const repo = this.view.repo, items = [];
    for (const [amend,signoff,keys] of [[false,false,['git_commit','git_commit_staged','git_commit_all']],
      [true,false,['git_commit_amend','git_commit_staged_amend','git_commit_all_amend']],
      [false,true,['git_commit_signoff','git_commit_staged_signoff','git_commit_all_signoff']]]) {
      if (amend || signoff) items.push(separator());
      keys.forEach((key,index) => items.push(this.item(key,{kind:'commit',scope:['auto','staged','all'][index],amend,signoff,after:'none'},
        repo.writable() && (amend && !!repo.status?.head || (index === 1 ? repo.status?.entries.some(entry => entry.staged !== null) : !!repo.status?.entries.length)))));
      if (!amend && !signoff) {
        items.push(this.confirm('git_undo_commit','undo_commit',repo.branches?.sync.head_summary ?? '',!!repo.status?.head),
          this.run('git_abort','abort',!!repo.branches?.sync.operation));
        if (repo.branches?.sync.operation) items.push(this.run('git_continue','continue',!repo.status?.entries.some(entry => entry.conflicted)));
      }
    }
    return items.concat(separator(),this.item('git_skip_hooks',{kind:'skip_hooks'},repo.writable(),{checked:repo.options.skip_hooks}));
  }
  commitSplit() {
    const repo = this.view.repo;
    return [['git_commit',false,'none'],['git_commit_amend',true,'none'],['git_commit_push',false,'push'],['git_commit_sync',false,'sync']]
      .flatMap(([key,amend,after],index) => [...(index === 2 ? [separator()] : []),this.item(key,{kind:'commit',scope:'auto',amend,signoff:false,after},repo.writable() && (!amend || !!repo.status?.head))]);
  }
  changes() {
    const entries = this.view.repo.status?.entries ?? [], paths = entries.map(entry => entry.path);
    return [this.item('git_stage_all',{kind:'index',operation:'stage',paths:indexPaths(entries,'stage')},this.view.repo.writable() && !!indexPaths(entries,'stage').length),
      this.item('git_unstage_all',{kind:'index',operation:'unstage',paths:indexPaths(entries,'unstage')},this.view.repo.writable() && !!indexPaths(entries,'unstage').length),
      this.confirm('git_discard_all',{discard_all:{paths}},paths.join('\n'),!!paths.length)];
  }
  network() {
    const repo = this.view.repo, sync = repo.branches?.sync, remotes = sync?.remotes ?? [], upstream = !!sync?.upstream;
    const pulls = (repo.branches?.entries ?? []).filter(branch => branch.remote).flatMap(branch => {
      const remote = remotes.filter(remote => branch.name.startsWith(`${remote}/`)).sort((left,right) => right.length - left.length)[0];
      return remote ? [this.choice(branch.name,{kind:'run',action:{pull:{rebase:false,remote,branch:branch.name.slice(remote.length + 1)}}})] : [];
    });
    return [this.run('git_sync',{sync:{rebase:false}},upstream),this.run('git_sync_rebase',{sync:{rebase:true}},upstream),separator(),
      this.run('git_pull',{pull:{rebase:false,remote:null,branch:null}},upstream),this.run('git_pull_rebase',{pull:{rebase:true,remote:null,branch:null}},upstream),this.choose('git_pull_from',pulls),separator(),
      this.push(),this.choose('git_push_to',this.remote(remote => ({kind:'run',action:{push:{remote,publish:false,force:false}}}))),this.push(true),separator(),
      ...[['git_fetch',false,false],['git_fetch_prune',true,false],['git_fetch_all',false,true]].map(([key,prune,all]) => this.run(key,{fetch:{remote:null,prune,all}},!!remotes.length))];
  }
  remotes() {
    return [this.item('git_add_remote',{kind:'form',form:'remote'},this.view.repo.writable()),
      this.choose('git_remove_remote',this.remote(name => ({kind:'confirm',title:'git_remove_remote',target:name,action:{remove_remote:{name}}})))];
  }
  stashes() {
    const repo = this.view.repo, stashes = repo.branches?.sync.stashes ?? [];
    const items = [['git_stash_tracked','tracked'],['git_stash_all','all'],['git_stash_staged','staged']]
      .map(([key,mode]) => this.run(key,{stash:{mode}},mode === 'staged' ? repo.status?.entries.some(entry => entry.staged !== null) : !!repo.status?.entries.length));
    for (const [latest,pick,pop] of [['git_stash_apply_latest','git_stash_apply',false],['git_stash_pop_latest','git_stash_pop',true]]) {
      const choices = stashes.map(stash => this.choice(stash.message,{kind:'run',action:{apply_stash:{commit:stash.commit,pop}}}));
      items.push(separator(),this.item(latest,choices[0]?.value,repo.writable() && !!choices.length),this.choose(pick,choices));
    }
    return items.concat(separator(),this.choose('git_stash_drop',stashes.map(stash => this.choice(stash.message,{kind:'confirm',title:'git_stash_drop',target:stash.message,action:{drop_stash:{commit:stash.commit}}}))),
      this.confirm('git_stash_drop_all',{drop_all_stashes:{commits:stashes.map(stash => stash.commit)}},stashes.map(stash => stash.message).join('\n'),!!stashes.length),separator(),
      this.choose('git_stash_view',stashes.map(stash => this.choice(stash.message,{kind:'open',request:{kind:'stash',commit:stash.commit}}))));
  }
  tags() {
    const repo = this.view.repo;
    return [this.item('git_tag_create',{kind:'form',form:'tag'},repo.writable() && !!repo.status?.head),
      this.choose('git_tag_delete',(repo.branches?.sync.tags ?? []).map(tag => this.choice(tag.name,{kind:'confirm',title:'git_tag_delete',target:tag.name,action:{delete_tag:{name:tag.name,commit:tag.commit,remote:null}}}))),
      this.choose('git_tag_delete_remote',this.remote(remote => ({kind:'remote_tags',remote}))),separator(),
      this.choose('git_tag_push',this.remote(remote => ({kind:'run',action:{push_tags:{remote}}})))];
  }
  root() {
    const repo = this.view.repo;
    return [this.section('git_view_options',this.viewOptions()),separator(),
      this.run('git_pull',{pull:{rebase:false,remote:null,branch:null}},!!repo.branches?.sync.upstream),this.push(),
      this.item('git_clone',{kind:'clone'}),this.choose('git_checkout',this.checkout()),this.run('git_fetch',{fetch:{remote:null,prune:false,all:false}},!!repo.branches?.sync.remotes.length),separator(),
      this.section('git_commit',this.commits()),this.section('git_changes_menu',this.changes()),this.section('git_pull_push',this.network()),
      this.choose('git_manage_branches',this.management()),this.section('git_remotes',this.remotes()),this.section('git_stash_menu',this.stashes()),this.section('git_tags',this.tags()),
      ...(this.view.worktrees ? [this.item('worktree_manage',{kind:'worktrees'},repo.writable())] : []),separator(),
      this.item('git_output',{kind:'open',request:{kind:'output'}})];
  }
  files(entries,scope,directory = false) {
    const paths = entries.map(entry => entry.path), repo = this.view.repo;
    const items = [];
    if (entries.length === 1 && !directory) {
      const entry = entries[0];
      if (entry.unstaged !== 'deleted' && entry.staged !== 'deleted') items.push(this.item('git_open_source',{kind:'source',path:entry.path}));
      items.push(this.item('git_unstaged',{kind:'open',request:{kind:'file',path:entry.path,scope:'unstaged'}},entry.unstaged !== null || entry.untracked),
        this.item('git_staged',{kind:'open',request:{kind:'file',path:entry.path,scope:'staged'}},entry.staged !== null),separator());
    }
    items.push(this.item('git_stage',{kind:'index',operation:'stage',paths:indexPaths(entries,'stage')},repo.writable() && !!indexPaths(entries,'stage').length),
      this.item('git_unstage',{kind:'index',operation:'unstage',paths:indexPaths(entries,'unstage')},repo.writable() && !!indexPaths(entries,'unstage').length),
      this.confirm('git_discard_changes',{discard:{paths}},paths.join('\n'),!!paths.length),this.run('git_ignore',{ignore:{paths,directories:directory}},!!paths.length),separator(),
      this.item('git_copy_path',{kind:'copy',text:paths.join('\n')}));
    return items;
  }
}
