import {readLocation,nextLocationChange,openResource} from 'sailry/ui';
import {context as readScope,publishContributions,nextContributionEvent,readGitDiff} from 'sailry/sdk';
import Git from './main.js';
import {dialogs} from './view.js';

export default class Context extends Git {
  init(props,cx) {
    this.overlay = true;
    this.worktree = readScope().worktree;
    this.location = readLocation();
    super.init(props,cx);
    this.publish();
    cx.spawn(async cx => {
      try { while (true) {
        const previous = this.location;
        this.location = await nextLocationChange(previous.cursor); this.publish();
        if (this.location.git !== previous.git) await this.invalidate();
        cx.notify();
      } } catch (_) { /* The controller retains its original checkout. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextContributionEvent(); this.publish(event);
        this.run(async () => {
          if (event.id === 'changes') { await openResource('git',{path:null}); return; }
          if (await this.repo.refresh()) this.actions.choose('git_manage_branches',event.id === 'branch' ? this.menus.checkout() : this.menus.management());
        },cx);
      } } catch (_) { /* Disabling the package releases contribution events. */ }
    });
  }
  async invalidate() {
    if (!this.worktree || !this.location?.worktree) return;
    const sequence = this.badgeRead = (this.badgeRead ?? 0) + 1;
    if (this.location.surface === 'composer' && !this.location.git) {
      this.counts = null; this.publish(); return;
    }
    if (!await this.repo.refresh()) return;
    if (this.location.surface !== 'composer' || this.repo.status.kind === 'directory') { this.counts = null; this.publish(); return; }
    let added = 0, removed = 0, partial = this.repo.status.truncated || this.repo.status.omitted_paths > 0;
    try {
      for (const entry of this.repo.status.entries) {
        const diff = await readGitDiff(entry.path,'all');
        added += diff.additions; removed += diff.deletions; partial ||= diff.truncated;
      }
      if (this.badgeRead === sequence) { this.counts = {added,removed,partial}; this.publish(); this.notify(); }
    } catch (_) { if (this.badgeRead === sequence) { this.counts = null; this.publish(); } }
  }
  publish(event) {
    const state = this.location;
    if (!state) return;
    publishContributions([
      {id:'branch',...(state.git ? {label:{label:state.branch || this.text.git_detached}} : {}),icon:'network',enabled:state.can_move,visible:!!this.worktree && state.main && state.git,dropdown:true,...(event?.id === 'branch' ? {reply_to:event.sequence} : {})},
      {id:'manage',enabled:state.can_move,visible:!!this.worktree && state.git,...(event?.id === 'manage' ? {reply_to:event.sequence} : {})},
      {id:'changes',enabled:true,visible:!!this.worktree && state.surface === 'composer' && !!this.counts,value:this.counts?.partial ? this.text.tool_partial : null,
        segments:this.counts ? [{label:{label:`+${this.counts.added}`},tone:'success'},{label:{label:`-${this.counts.removed}`},tone:'danger'}] : [],
        ...(event?.id === 'changes' ? {reply_to:event.sequence} : {})}
    ]);
  }
  render() { return dialogs(this); }
}
