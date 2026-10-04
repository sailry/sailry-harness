import {createText, readText, releaseText, setText, focusText} from 'sailry/forms';
import {writeClipboard, invokeContribution, openFile, openProjectCreation} from 'sailry/ui';
import {openSystem} from 'sailry/file-transfers';
import {prepareGitChange, completeRequest, forgetRequest, faultCode, listGitRemoteTags} from 'sailry/sdk';

export class Actions {
  constructor(view) { this.view = view; this.sequence = 0; this.dialog = null; this.picker = null; }
  choose(title,items,literal = false) {
    this.picker = {id:++this.sequence,title:literal ? title : this.view.text[title === 'git_checkout' ? 'git_search_branches' : title],items:items.map((item,index) => ({...item,id:String(index)}))};
    this.view.notify();
  }
  async invoke(value) {
    if (!value) return;
    const view = this.view, repo = view.repo;
    switch (value.kind) {
      case 'noop': break;
      case 'choose': this.choose(value.title,value.items,value.literal); break;
      case 'run': await repo.action(value.action); break;
      case 'index': await repo.index(value.paths,value.operation); break;
      case 'confirm': this.confirm(value); break;
      case 'commit': await this.commit(value); break;
      case 'skip_hooks': repo.options.skip_hooks = !repo.options.skip_hooks; break;
      case 'branch': await this.branch(value.operation,value.branch); break;
      case 'form': this.form(value.form,value.branch); break;
      case 'view': repo.navigation[value.field] = value.value; break;
      case 'open': await repo.open(value.request); break;
      case 'copy': writeClipboard(value.text); break;
      case 'source': await openFile(value.path); break;
      case 'system': await openSystem(value.path); break;
      case 'clone': await openProjectCreation({clone:true}); break;
      case 'worktrees': await invokeContribution('worktrees'); break;
      case 'worktree': await invokeContribution('create_worktree',{branch:value.branch,commit:value.commit}); break;
      case 'remote_tags': {
        const tags = await listGitRemoteTags(value.remote);
        this.choose('git_tag_delete_remote',tags.map(tag => ({label:tag.name,value:{kind:'confirm',title:'git_tag_delete_remote',target:`${value.remote}/${tag.name}`,
          action:{delete_tag:{name:tag.name,commit:tag.commit,remote:value.remote}}}}))); break;
      }
    }
    view.notify();
  }
  async commit({scope = 'auto',amend = this.view.repo.amend,signoff = this.view.repo.options.signoff,after = this.view.repo.options.after,remote}) {
    const view = this.view, repo = view.repo, sync = repo.branches?.sync;
    if (!repo.writable()) return;
    if (remote) repo.options.push_remote = remote;
    if (after !== 'none' && !repo.options.push_remote && !sync?.upstream) {
      if (!sync?.remotes.length) this.form('remote');
      else this.choose('git_publish',sync.remotes.map(remote => ({label:remote,value:{kind:'commit',scope,amend,signoff,after,remote}})));
      return;
    }
    const entering = amend && !repo.amend;
    if (entering) { repo.toggleAmend(); setText(view.message,repo.message); }
    repo.amend = amend; repo.options.signoff = signoff; repo.options.after = after;
    repo.options.tracked = scope === 'tracked'; repo.options.all = scope === 'all';
    if (entering || !repo.message.trim()) { focusText(view.message); return; }
    const choices = repo.commitChoices(scope);
    if (choices) {
      this.choose('git_commit_scope',choices.map(scope => ({label:view.text[`git_commit_${scope}`],value:{kind:'commit',scope,amend,signoff,after}}))); return;
    }
    await repo.commit(scope); setText(view.message,repo.message);
  }
  async branch(operation,branch) {
    const repo = this.view.repo;
    if (operation === 'copy') { writeClipboard(branch.name); return; }
    if (operation === 'create' || operation === 'rename') { this.form(operation === 'create' ? 'branch' : 'rename',branch); return; }
    if (operation === 'switch') {
      if (branch.current) return;
      if (!branch.remote) { await repo.branch('switch',{name:branch.name,commit:branch.commit}); return; }
      const sync = repo.branches.sync;
      const tracked = repo.branches.entries.find(entry => !entry.remote && sync.tracking[entry.name] === branch.name);
      const remote = sync.remotes.filter(remote => branch.name.startsWith(`${remote}/`)).sort((left,right) => right.length - left.length)[0];
      if (!remote) return;
      const local = tracked?.name ?? branch.name.slice(remote.length + 1);
      if (!tracked && repo.branches.entries.some(entry => !entry.remote && entry.name === local)) { this.form('tracking',branch); return; }
      await repo.action({switch_tracking:{remote:branch.name,local,stash:false}}); return;
    }
    this.confirm({title:operation === 'delete' ? 'git_delete_branch' : operation === 'merge' ? 'git_merge_branch' : 'git_rebase',
      target:branch.name,branch,operation,revisions:repo.revisions(),kind:'confirm',
      action:operation === 'rebase' ? {rebase:{commit:branch.commit}} : null});
  }
  confirm(value) {
    this.close();
    this.dialog = {...value,id:++this.sequence,form:'confirm',revisions:value.revisions ?? this.view.repo.revisions()};
    this.view.notify();
  }
  form(form,branch) {
    this.close();
    const repo = this.view.repo, text = this.view.text;
    const title = {branch:'git_new_branch',rename:'git_rename_branch',remote:'git_add_remote',tag:'git_tag_create',tracking:'git_tracking_name'}[form];
    const name = form === 'rename' ? branch.name : form === 'remote' ? 'origin' : form === 'tracking' ? branch.name.split('/').at(-1) : '';
    const input = createText(name,{label:text[form === 'remote' ? 'git_remote_name' : form === 'tag' ? 'git_name' : 'git_branch_name'],placeholder:text[form === 'remote' ? 'form_remote_hint' : 'form_branch_hint']});
    const extra = ['remote','tag'].includes(form) ? createText('',{label:text[form === 'remote' ? 'git_remote_url' : 'git_tag_message'],placeholder:form === 'remote' ? text.project_repository_placeholder : text.git_tag_message}) : null;
    this.dialog = {id:++this.sequence,form,title,input,extra,branch,commit:branch?.commit ?? repo.status?.head,
      base:branch?.name ?? repo.status?.branch,revisions:repo.revisions(),pending:null,busy:false,error:null};
    focusText(input); this.view.notify();
  }
  close() {
    const dialog = this.dialog;
    if (dialog) { dialog.closed = true; if (dialog.input) releaseText(dialog.input); if (dialog.extra) releaseText(dialog.extra); }
    this.dialog = null; this.view.notify();
  }
  async submit() {
    const dialog = this.dialog, repo = this.view.repo;
    if (!dialog || dialog.busy) return;
    let draft, name = dialog.input ? readText(dialog.input) : null, extra = dialog.extra ? readText(dialog.extra) : null;
    if (dialog.input && !name.length) { dialog.error = 'git_branch_name_required'; this.view.report(dialog.error); this.view.notify(); return; }
    if (dialog.form === 'branch') draft = dialog.commit ? {kind:'create',name,commit:dialog.commit}
      : {kind:'action',action:{start_branch:{name}},...dialog.revisions};
    else if (dialog.form === 'rename') draft = {kind:'rename',name:dialog.branch.name,new_name:name,commit:dialog.commit};
    else if (dialog.form === 'remote') {
      name = name.trim(); extra = extra.trim(); if (!name || !extra) return;
      draft = {kind:'action',action:{add_remote:{name,url:extra}},...dialog.revisions};
    } else if (dialog.form === 'tag') draft = {kind:'action',action:{create_tag:{name,commit:dialog.commit,message:extra || null}},...dialog.revisions};
    else if (dialog.form === 'tracking') draft = {kind:'action',action:{switch_tracking:{remote:dialog.branch.name,local:name,stash:false}},...dialog.revisions};
    else if (dialog.operation && ['delete','merge'].includes(dialog.operation)) {
      draft = {kind:dialog.operation,name:dialog.branch.name,commit:dialog.branch.commit,...dialog.revisions};
      if (dialog.operation === 'delete') delete draft.expected_index;
    } else draft = {kind:'action',action:dialog.action,...dialog.revisions};
    const key = JSON.stringify(draft);
    try {
      if (!dialog.pending || dialog.pending.key !== key) {
        if (dialog.pending) forgetRequest(dialog.pending.id);
        dialog.pending = {id:prepareGitChange(draft),key};
      }
      dialog.busy = true; dialog.error = null; this.view.notify();
      const receipt = await completeRequest(dialog.pending.id);
      if (dialog.closed) return;
      if (receipt.Err) { dialog.error = namingError(dialog.form,receipt.Err.code); this.view.report(dialog.error); return; }
      if (!receipt.Ok) { dialog.error = namingError(dialog.form,'outcome_unknown'); this.view.report(dialog.error); return; }
      forgetRequest(dialog.pending.id); dialog.pending = null;
      if (this.dialog === dialog) this.close();
      await repo.refresh();
    } catch (error) { if (!dialog.closed) { dialog.error = namingError(dialog.form,faultCode(error.message ?? String(error))); this.view.report(dialog.error); } }
    finally { dialog.busy = false; this.view.notify(); }
  }
  async stashSwitch() {
    const pending = this.view.repo.pending;
    if (pending?.kind !== 'switch' || pending.error !== 'git_branch_conflict') return;
    const draft = pending.branch, tracking = pending.action?.switch_tracking;
    if (!draft && !tracking) return;
    if (!await this.view.repo.refresh(true)) return;
    await this.view.repo.action(tracking ? {switch_tracking:{...tracking,stash:true}}
      : {switch_stashing:{branch:draft.name,commit:draft.commit}});
  }
}

export function namingError(form,code) {
  if (code === 'invalid_request') return 'git_branch_name_invalid';
  if (code === 'revision_conflict') return 'git_branch_revision';
  if (form === 'branch') return ({conflict:'git_branch_exists',permission_denied:'git_branch_create_denied',outcome_unknown:'git_branch_create_unknown'})[code] ?? 'git_branch_create_failed';
  if (form === 'rename') return code === 'conflict' ? 'git_branch_rename_conflict' : code === 'outcome_unknown' ? 'git_branch_rename_unknown' : 'git_branch_rename_failed';
  return code === 'outcome_unknown' ? 'git_action_unknown' : code === 'conflict' ? 'git_action_conflict' : 'git_action_failed';
}
