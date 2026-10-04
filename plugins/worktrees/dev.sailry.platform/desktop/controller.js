// Package-owned forms and retries; core retains source scope, admission and session moves.
import {listing,errorKey,current} from './policy.js';

export class Controller {
  constructor(api,text,changed) {
    this.api = api; this.text = text; this.changed = changed;
    this.location = api.readLocation(); this.serial = 0; this.dialog = null; this.catalog = null;
    this.operation = null; this.disposed = false;
  }
  notify() { if (!this.disposed) this.changed(); }
  owner(kind, extra = {}) {
    return {token:`worktrees-${++this.serial}`,kind,bound:this.location,pending:false,error:null,request:null,uncertain:false,...extra};
  }
  accept(location) { this.location = location; this.notify(); }
  valid(owner) { return current(owner.bound,this.api.readLocation()); }
  show(owner) { this.dialog = owner; this.notify(); return owner; }
  close() { this.dialog = null; this.notify(); }
  fail(owner,key) {
    owner.error = key;
    this.api.toast({id:owner.token,message:this.text(key),kind:'error'});
    this.notify();
  }
  async open(intent = 'manage', entry) {
    this.location = this.api.readLocation();
    if (!this.location.can_move) return;
    if (!this.location.project) { await this.api.selectLocation(this.location.cursor,{projects:true}); return; }
    if (intent === 'create' && entry) return this.create({branch:entry.branch,head:entry.commit});
    if (intent === 'create' || intent === 'fork') return this.createManaged(intent === 'fork');
    const owner = this.show(this.owner('list',{loading:true,choices:[]}));
    try {
      const [catalog,found] = await Promise.all([this.api.readWorktreeCatalog(),this.api.listWorktrees()]);
      if (this.dialog !== owner) return;
      this.catalog = catalog;
      owner.choices = listing(catalog,found,owner.bound,this.text);
      if (entry) return this.create(entry);
    } catch (_) { if (this.dialog === owner) this.fail(owner,'worktree_load_failed'); }
    finally { owner.loading = false; this.notify(); }
  }
  async choose(id, captured) {
    const owner = this.dialog;
    if (!owner || owner.pending) return;
    const choice = captured?.token === owner.token ? captured.choice : owner.choices?.find(choice => choice.id === id && !choice.disabled);
    if (!choice?.action) return;
    if (!this.valid(owner)) { this.fail(owner,'worktree_context_changed'); return; }
    const action = choice.action;
    if (action.kind === 'actions') {
      owner.choices = action.actions.map((action,index) => ({id:`action-${index}`,label:this.text(action.kind === 'open' ? 'workspace_open_worktree' : 'worktree_remove'),detail:choice.label,action}));
      owner.kind = 'actions'; this.notify();
    } else if (action.kind === 'projects') {
      this.close(); await this.api.selectLocation(owner.bound.cursor,{projects:true});
    } else if (action.kind === 'managed') await this.createManaged(action.fork);
    else if (action.kind === 'create') this.create(action.entry);
    else if (action.kind === 'prune') await this.prune(owner,action.entry);
    else await this.register(owner,action);
  }
  create(entry) {
    const owner = this.owner('create',{values:{revision:entry.branch || entry.head || '',branch:'',path:''},fields:{}});
    this.show(owner); return owner;
  }
  async createManaged(fork) {
    const owner = this.show(this.owner('managed',{fork,values:{branch:`sailry/task-${this.api.newId().slice(0,8)}`},fields:{},include_changes:true,status:null,pending:true}));
    try { owner.status = await this.api.inspectGit(); }
    catch (_) { if (this.dialog === owner) this.fail(owner,'worktree_load_failed'); }
    finally { owner.pending = false; this.notify(); }
    return owner;
  }
  async register(owner,action) {
    if (owner.pending) return;
    owner.pending = true; this.operation = {owner,action}; this.notify();
    try {
      let id = action.worktree;
      if (!id) {
        const signature = `register:${action.entry.path}`;
        if (owner.request && owner.signature !== signature) {
          if (owner.uncertain) { this.fail(owner,'worktree_outcome_unknown'); return; }
          this.api.forgetRequest(owner.request); owner.request = null;
        }
        if (!owner.request) {owner.signature = signature; owner.request = this.api.prepareWorktreeChange({kind:'register',project:owner.bound.project,path:action.entry.path});}
        const result = await this.api.completeRequest(owner.request);
        if (result.Err) {
          owner.uncertain = result.Err.code === 'outcome_unknown'; this.fail(owner,errorKey(result.Err.code));
          this.api.toast({id:owner.token,message:this.text(errorKey(result.Err.code)),kind:'error',action:{id:'retry',label:this.text('worktree_retry')}}); return;
        }
        id = result.Ok.data.id;
        owner.uncertain = false;
      }
      if (this.dialog !== owner) return;
      if (!this.valid(owner)) { this.fail(owner,'worktree_context_changed'); return; }
      if (action.kind === 'open') {
        await this.api.selectLocation(owner.bound.cursor,{worktree:id,fork:false,...(owner.request ? {request:owner.request} : {})});
        if (owner.request) {this.api.forgetRequest(owner.request); owner.request = null;}
        if (this.dialog === owner) this.close();
      } else {
        if (await this.api.worktreeHasDrafts(id,owner.request ?? undefined)) { this.fail(owner,'worktree_remove_unsaved'); return; }
        if (owner.request) {this.api.forgetRequest(owner.request); owner.request = null;}
        const removal = this.owner('remove',{entry:action.entry,worktree:id});
        removal.bound = owner.bound;
        this.show(removal);
      }
    } catch (_) {
      owner.uncertain = !!owner.request;
      this.fail(owner,owner.request ? 'worktree_outcome_unknown' : 'worktree_context_changed');
      if (owner.request) this.api.toast({id:owner.token,message:this.text('worktree_outcome_unknown'),kind:'error',action:{id:'retry',label:this.text('worktree_retry')}});
    } finally { owner.pending = false; this.notify(); }
  }
  async submit(values = this.dialog?.values) {
    const owner = this.dialog;
    if (!owner || owner.pending) return;
    if (!owner.request && !this.valid(owner)) { this.fail(owner,'worktree_context_changed'); return; }
    if (owner.kind === 'create') await this.submitCreate(owner,values);
    else if (owner.kind === 'managed') await this.submitManaged(owner,values);
    else if (owner.kind === 'remove') await this.submitRemoval(owner);
  }
  async submitCreate(owner,values) {
    const signature = JSON.stringify(values);
    if (owner.uncertain && owner.signature !== signature) return;
    owner.pending = true; owner.error = null; owner.values = {...values}; this.notify();
    try {
      if (owner.request && owner.signature !== signature) { this.api.forgetRequest(owner.request); owner.request = null; }
      if (!owner.request) {
        let resolved;
        try { resolved = await this.api.resolveGitRevision(values.revision); }
        catch (_) { this.fail(owner,'worktree_revision_failed'); return; }
        if (this.dialog !== owner) return;
        if (!this.valid(owner)) { this.fail(owner,'worktree_context_changed'); return; }
        owner.request = this.api.prepareWorktreeChange({kind:'create',project:owner.bound.project,path:values.path,branch:values.branch,commit:resolved.commit});
        owner.signature = signature;
      }
      await this.execute(owner);
    } finally { owner.pending = false; this.notify(); }
  }
  async submitManaged(owner,values) {
    const branch = values.branch.trim();
    if (!owner.status) { this.fail(owner,'worktree_load_failed'); return; }
    if (!owner.status.head || !owner.status.index_revision) { this.fail(owner,'worktree_commit_first'); return; }
    if (!branch) { this.fail(owner,'git_branch_name_required'); return; }
    owner.pending = true; owner.error = null; this.notify();
    try {
      if (!owner.request) owner.request = this.api.prepareWorktreeChange({kind:'managed',project:owner.bound.project,branch,
        expected_head:owner.status.head,expected_index:owner.status.index_revision,include_changes:owner.include_changes});
      await this.execute(owner);
    } finally { owner.pending = false; this.notify(); }
  }
  async submitRemoval(owner) {
    if (!owner.request && !this.valid(owner)) { this.fail(owner,'worktree_context_changed'); return; }
    if (await this.api.worktreeHasDrafts(owner.worktree)) { this.fail(owner,'worktree_remove_unsaved'); return; }
    if (this.dialog !== owner || owner.pending) return;
    owner.pending = true; owner.error = null; this.notify();
    try {
      if (!owner.request) owner.request = this.api.prepareWorktreeChange({kind:'remove',worktree:owner.worktree,expected_head:owner.entry.head,expected_branch:owner.entry.branch});
      await this.execute(owner);
    } finally { owner.pending = false; this.notify(); }
  }
  async execute(owner) {
    try {
      const result = await this.api.completeRequest(owner.request);
      if (result.Err) { owner.uncertain = result.Err.code === 'outcome_unknown'; this.fail(owner,errorKey(result.Err.code,owner.kind === 'remove')); return; }
      if (owner.kind === 'remove') await this.api.releaseWorktree(owner.worktree,owner.request);
      owner.uncertain = false;
      if (this.dialog !== owner) {this.api.forgetRequest(owner.request);owner.request = null;return;}
      if (owner.kind !== 'remove') {
        if (!this.valid(owner)) { this.fail(owner,owner.kind === 'managed' ? 'location_created_changed' : 'worktree_context_changed'); return; }
        await this.api.selectLocation(owner.bound.cursor,{worktree:result.Ok.data.id,fork:owner.fork ?? false,request:owner.request});
      }
      this.api.forgetRequest(owner.request); owner.request = null;
      if (this.dialog === owner) this.close();
    } catch (_) { owner.uncertain = !!owner.request; this.fail(owner,owner.request ? 'worktree_outcome_unknown' : 'worktree_context_changed'); }
  }
  async prune(owner,entry) {
    owner.pending = true; this.notify();
    try {
      const signature = `prune:${entry.path}`;
      if (owner.request && owner.signature !== signature) {
        if (owner.uncertain) {this.fail(owner,'worktree_outcome_unknown'); return;}
        this.api.forgetRequest(owner.request); owner.request = null;
      }
      if (!owner.request) {
        const status = await this.api.inspectGit();
        if (this.dialog !== owner || !this.valid(owner)) return;
        owner.signature = signature; owner.request = this.api.prepareGitChange({kind:'action',action:{prune_worktree:{path:entry.path}},
          expected_head:status.head,expected_branch:status.branch,expected_index:status.index_revision});
      }
      const result = await this.api.completeRequest(owner.request);
      if (result.Err) { owner.uncertain = result.Err.code === 'outcome_unknown'; this.fail(owner,'worktree_prune_failed'); return; }
      this.api.forgetRequest(owner.request); owner.request = null;
      this.api.toast({id:`${owner.token}-complete`,message:this.text('worktree_pruned'),kind:'info'});
      if (this.dialog === owner) await this.open();
    } catch (_) { owner.uncertain = !!owner.request; this.fail(owner,'worktree_prune_failed'); }
    finally { owner.pending = false; this.notify(); }
  }
  retry() {
    const pending = this.operation;
    if (pending && this.dialog === pending.owner) return this.register(pending.owner,pending.action);
  }
  dispose() { this.disposed = true; }
}
