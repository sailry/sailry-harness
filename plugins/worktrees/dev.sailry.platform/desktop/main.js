import {View} from 'gpui-kit';
import {context} from 'sailry';
import {readWorktreeCatalog,listWorktrees,inspectGit,resolveGitRevision,prepareWorktreeChange,
  prepareGitChange,completeRequest,forgetRequest,newId,publishContributions,nextContributionEvent} from 'sailry/sdk';
import {readLocation,nextLocationChange,selectLocation,worktreeHasDrafts,releaseWorktree,
  nextPickerEvent,modal_closed,toast,dismissToast,nextToastEvent} from 'sailry/ui';
import {createText,readText,releaseText,focusText,nextTextEvent} from 'sailry/forms';
import {Controller} from './controller.js';
import {messages} from './locales.js';
import {render} from './view.js';

export default class Worktrees extends View {
  init(_props,cx) {
    cx.spawn(async cx => { this.notify = () => cx.notify(); });
    const strings = messages(JSON.parse(context()).locale);
    this.text = key => strings[key] ?? key;
    this.fieldsOwner = null;
    this.model = new Controller({readLocation,selectLocation,readWorktreeCatalog,listWorktrees,
      inspectGit,resolveGitRevision,prepareWorktreeChange,prepareGitChange,completeRequest,
      forgetRequest,newId,worktreeHasDrafts,releaseWorktree,toast},this.text,() => {
        this.syncFields(); this.publish(); this.notify();
      });
    this.publish();
    cx.spawn(async cx => {
      try { while (true) {
        this.model.accept(await nextLocationChange(this.model.location.cursor)); cx.notify();
      } } catch (_) { /* The captured controller owns its observation. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextContributionEvent();
        this.publish(event);
        this.run(() => this.model.open(event.handler,event.value && typeof event.value === 'object' ? event.value : undefined),cx);
      } } catch (_) { /* Unmounting closes the contribution channel. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextPickerEvent();
        if (this.model.dialog?.token !== event.picker) continue;
        if (event.kind === 'close') this.close(cx);
        else this.run(() => this.model.choose(event.id,event.value),cx);
      } } catch (_) { /* Native picker state belongs to this controller. */ }
    });
    cx.spawn(async cx => {
      try { while (true) { if (await modal_closed() === this.model.dialog?.token) this.close(cx); } }
      catch (_) { /* Closing the owner releases its modal. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextTextEvent(), owner = this.model.dialog;
        if (owner && Object.values(owner.fields ?? {}).includes(event.id) && event.kind === 'enter') this.submit(cx);
      } } catch (_) { /* Native fields are released with the form. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextToastEvent();
        if (event.action === 'retry' && event.id === this.model.operation?.owner.token) this.run(() => this.model.retry(),cx);
      } } catch (_) { /* The mounted controller owns retry affordances. */ }
    });
  }
  publish(event) {
    const state = this.model.location;
    const reply = id => event?.id === id ? {reply_to:event.sequence} : {};
    const label = !state.project ? this.text('composer_no_project') : state.main ? state.project_name : state.branch;
    publishContributions([
      {id:'location',enabled:state.can_move && !(state.session && !state.project),label:{label},icon:state.main ? 'folder' : 'network',dropdown:true,...reply('location')},
      {id:'manage',enabled:state.can_move,visible:!!state.project,...reply('manage')},
      {id:'create',enabled:state.can_move,visible:!!state.project && state.git,...reply('create')},
      {id:'fork',enabled:state.can_move,visible:!!state.project && !!state.session && state.git,...reply('fork')},
    ]);
  }
  syncFields() {
    const owner = this.model.dialog;
    if (owner === this.fieldsOwner) return;
    if (this.fieldsOwner) {
      for (const field of Object.values(this.fieldsOwner.fields ?? {})) releaseText(field);
      dismissToast(this.fieldsOwner.token);
    }
    this.fieldsOwner = owner;
    if (!owner?.fields) return;
    const labels = {revision:'worktree_base',branch:'worktree_branch',path:'worktree_path'};
    const hints = {revision:'form_revision_hint',branch:'form_branch_hint',path:'form_worktree_path_hint'};
    for (const [key,value] of Object.entries(owner.values)) owner.fields[key] = createText(value,{label:this.text(labels[key]),placeholder:this.text(hints[key])});
    focusText(owner.fields.branch);
  }
  run(action,cx) {
    cx.spawn(async cx => {
      try { await action(); }
      catch (_) { if (this.model.dialog) this.model.fail(this.model.dialog,'worktree_request_failed'); }
      cx.notify();
    });
  }
  submit(cx) {
    const owner = this.model.dialog;
    if (!owner || owner.pending) return;
    const values = Object.fromEntries(Object.entries(owner.fields ?? {}).map(([name,id]) => [name,readText(id)]));
    this.run(() => this.model.submit(values),cx);
  }
  overview(cx) {
    const owner = this.model.dialog;
    this.close(cx);
    this.run(() => selectLocation(owner.bound.cursor,{overview:true}),cx);
  }
  close(cx) { this.model.close(); cx.notify(); }
  release() { this.model.dispose(); }
  render() { return render(this); }
}
