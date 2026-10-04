// Dialog admission and retries follow the original create/rename/trash forms.
import {prepareFileAction, completeRequest, forgetRequest, faultCode} from 'sailry/sdk';
import {parent, join} from './explorer.js';

export function failure(kind,code) {
  const prefix = kind === 'rename' ? 'files_rename' : kind === 'trash' ? 'files_trash' : 'files_create';
  if (code === 'outcome_unknown') return `${prefix}_unknown`;
  if (code === 'permission_denied') return `${prefix}_denied`;
  if (code === 'conflict' || code === 'revision_conflict') return kind === 'create' ? 'files_entry_exists' : `${prefix}_conflict`;
  if (code === 'invalid_request' && kind === 'create') return 'files_name_invalid';
  return `${prefix}_failed`;
}

export class Mutation {
  constructor(kind,paths,directory = false) {
    this.kind = kind; this.paths = [...paths]; this.directory = directory;
    this.pending = null; this.busy = false; this.error = null; this.done = false;
    this.completed = []; this.closed = false;
  }

  arguments(name) {
    if (this.kind === 'trash') return this.paths.map(path => ({kind:'trash',path}));
    if (!name || name.includes('/')) { this.error = 'files_name_required'; return null; }
    if (this.kind === 'rename') return [{kind:'rename',from:this.paths[0],to:join(parent(this.paths[0]),name)}];
    const path = join(this.paths[0] ?? '',name);
    return [this.directory ? {kind:'create_directory',path} : {kind:'write',path,text:'',revision:null}];
  }

  async submit(name,cx) {
    if (this.busy || this.closed || this.done) return;
    const actions = this.pending ? this.pending.actions : this.arguments(name);
    if (!actions) { cx.notify(); return; }
    this.busy = true; this.error = null; cx.notify();
    try {
      for (let index = this.completed.length; index < actions.length; index++) {
        if (!this.pending) this.pending = {id:prepareFileAction(actions[index]),actions};
        const result = await completeRequest(this.pending.id);
        if (result.Err) {
          this.error = failure(this.kind,result.Err.code);
          if (result.Err.code !== 'outcome_unknown') {
            forgetRequest(this.pending.id); this.pending = null;
          }
          return;
        }
        this.completed.push(actions[index]);
        forgetRequest(this.pending.id); this.pending = null;
        if (this.closed) return;
      }
      this.done = true;
    } catch (error) { this.error = failure(this.kind,this.pending ? 'outcome_unknown' : faultCode(error.message ?? String(error))); }
    finally { this.busy = false; cx.notify(); }
  }
}
