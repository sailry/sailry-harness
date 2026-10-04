import {View, div} from 'gpui-kit';
import {context} from 'sailry';
import * as sdk from 'sailry/sdk';
import {createText, readText, setText, nextTextEvent} from 'sailry/forms';
import {Workspace, workspaceMode, ActionScope, registerShortcuts, closePanel, toast, nextControlEvent, nextMenuEvent,
  nextNavigationTabEvent, nextTreeEvent, nextContextMenuEvent, nextPickerEvent, modal_closed,
  canInvokeContribution, readResourceTarget, nextResourceTarget, nextWindowActivation, focusSurface, collapseOpen} from 'sailry/ui';
import {Repository} from './repository.js';
import {Actions} from './actions.js';
import {Menus} from './menus.js';
import {checkbox} from './changes.js';
import {messages} from './locales.js';
import * as views from './view.js';

export default class Git extends View {
  init(_props,cx) {
    this.text = messages(JSON.parse(context()).locale);
    cx.spawn(async cx => { this.notify = () => cx.notify(); });
    this.contentFocus = cx.focus_handle();
    this.shortcutContext = this.overlay ? null : registerShortcuts([
      {keystroke:'secondary-enter',action:'git-commit-action'},
      {keystroke:'secondary-shift-enter',action:'git-amend-action'},
      {keystroke:'secondary-enter',action:'git-commit-action',context:'Input'},
      {keystroke:'secondary-shift-enter',action:'git-amend-action',context:'Input'},
      {keystroke:'ctrl-g ctrl-g',action:'git-fetch-action'},
      {keystroke:'ctrl-g down',action:'git-pull-action'},
      {keystroke:'ctrl-g shift-down',action:'git-pull-rebase-action'},
      {keystroke:'secondary-shift-k',action:'git-push-action'},
    ]);
    this.mode = workspaceMode(); this.worktrees = false; this.worktreesPending = false;
    this.message = createText('',{multiline:true,label:this.text.git_commit_message,placeholder:this.text.git_commit_message});
    this.repo = new Repository(sdk,() => this.notify(),key => this.report(key));
    this.actions = new Actions(this); this.menus = new Menus(this);
    this.expanded = new Map(); this.nodes = new Map();
    this.watch(nextMenuEvent,(event,cx) => this.run(() => this.actions.invoke(event.value),cx),cx);
    this.watch(nextContextMenuEvent,(event,cx) => this.run(() => this.actions.invoke(event.value),cx),cx);
    this.watch(nextControlEvent,(event,cx) => this.control(event.id,cx,event.value),cx);
    this.watch(nextTreeEvent,(event,cx) => this.tree(event,cx),cx);
    this.watch(nextNavigationTabEvent,(event,cx) => {
      if (event.bar !== 'git-tabs') return;
      if (event.kind === 'close') this.closeDocument(event.id);
      else if (this.repo.documents.has(event.id)) { this.repo.selected = event.id; this.focusDocument(); }
      cx.notify();
    },cx);
    this.watch(nextPickerEvent,(event,cx) => {
      if (event.picker !== `git-picker-${this.actions.picker?.id}`) return;
      this.actions.picker = null;
      if (event.kind === 'select') this.run(() => this.actions.invoke(event.value),cx);
      cx.notify();
    },cx);
    this.watch(modal_closed,id => { if (id === `git-dialog-${this.actions.dialog?.id}`) this.actions.close(); },cx);
    this.watch(nextTextEvent,(event,cx) => {
      if (event.id === this.message && event.kind === 'change') this.repo.message = readText(this.message);
      if (event.kind === 'enter' && [this.actions.dialog?.input,this.actions.dialog?.extra].includes(event.id)) this.run(() => this.actions.submit(),cx);
      cx.notify();
    },cx);
    cx.spawn(async cx => {
      let cursor = '';
      try { while (true) {
        const change = await sdk.nextChange(cursor); cursor = change.cursor;
        this.repo.connected = change.connected;
        this.refreshWorktrees(cx);
        if (change.connected) await this.invalidate();
        cx.notify();
      } } catch (_) { /* Releasing a renderer stops observation, not admitted Git work. */ }
    });
    cx.spawn(async cx => {
      let cursor = '';
      try { while (true) {
        const state = await nextWindowActivation(cursor); const changed = cursor !== ''; cursor = state.cursor;
        if (changed && state.active) await this.invalidate();
        cx.notify();
      } } catch (_) { /* Activation ends with this controller. */ }
    });
    if (!this.overlay) cx.spawn(async cx => {
      let target = readResourceTarget();
      try { while (target) {
        if (target.value) await this.repo.open(target.value.path ? {kind:'file',path:target.value.path,scope:'all'} : {kind:'changes'});
        target = await nextResourceTarget(target.cursor); cx.notify();
      } } catch (_) { /* Resource routing retains the captured checkout. */ }
    });
  }
  refreshWorktrees(cx) {
    if (this.worktreesPending || this.repo.closed) return;
    this.worktreesPending = true;
    cx.spawn(async cx => {
      let available = false;
      try { available = await canInvokeContribution('worktrees'); }
      catch (_) { /* Optional contributions do not block repository observation. */ }
      finally { this.worktreesPending = false; }
      if (!this.repo.closed) { this.worktrees = available; cx.notify(); }
    });
  }
  async invalidate() { await this.repo.refresh(); }
  watch(read,handle,cx) {
    cx.spawn(async cx => { try { while (true) { handle(await read(),cx); cx.notify(); } } catch (_) { /* Event streams close with this captured controller. */ } });
  }
  run(action,cx) {
    cx.spawn(async cx => {
      try { await action(cx); }
      catch (_) { this.report('git_action_failed'); }
      finally { cx.notify(); }
    });
  }
  report(key) { toast({id:'git-error',message:this.text[key] ?? this.text.git_action_failed,kind:'error'}); }
  focusDocument() {
    const document = this.repo.documents.get(this.repo.selected);
    if (!document) return;
    const request = document.request;
    if (request.kind === 'output') focusSurface(`git-output-${document.id}`);
    else if (['file','stash'].includes(request.kind)) {
      if (document.files?.[0] && !document.files[0].binary) focusSurface(`git-diff-${document.id}`);
      else this.contentFocus.focus();
    }
    else {
      const file = document.files?.find(file => !file.binary && file.text
        && (collapseOpen(`diff-file-${document.id}/${file.path}`) ?? views.defaultOpen));
      if (file) focusSurface(`git-diff-${document.id}/${file.path}`);
      else this.contentFocus.focus();
    }
  }
  closeDocument(id) {
    const active = this.repo.selected === id;
    this.repo.close(id);
    if (active) this.focusDocument();
  }
  amend(cx) { this.repo.toggleAmend(); setText(this.message,this.repo.message); cx.notify(); }
  control(id,cx,value) {
    if (id === 'git-refresh') this.run(() => this.repo.refresh(true),cx);
    else if (id === 'git-initialize') this.run(() => this.repo.initialize(),cx);
    else if (id === 'git-branch-menu' && this.repo.branches) this.actions.choose('git_checkout',this.menus.checkout());
    else if (id === 'git-expand-commit') this.repo.commitOpen = !this.repo.commitOpen;
    else if (id === 'git-views' && ['changes','history'].includes(value) && this.repo.view !== value) {
      this.repo.view = value;
      if (value === 'history') this.run(() => this.repo.loadHistory(),cx);
    }
    else if (id === 'git-open-changes') this.run(() => this.repo.open({kind:'changes'}),cx);
    else if (id === 'git-close-current') { if (this.repo.selected) this.closeDocument(this.repo.selected); else closePanel(); }
    else if (id === 'git-commit-action') this.run(() => this.actions.commit({}),cx);
    else if (id === 'git-amend-action') this.amend(cx);
    else if (id.startsWith('git-commit-')) this.run(() => this.actions.invoke({kind:'open',request:{kind:'commit',commit:id.slice(11)}}),cx);
    else if (id.startsWith('git-diff-undo-')) this.actions.confirm({kind:'confirm',title:'turn_changes_undo',target:id.slice(14),action:{discard_all:{paths:[id.slice(14)]}}});
    else if (id === 'git-fetch-action') this.run(() => this.repo.action({fetch:{remote:null,prune:true,all:true}}),cx);
    else if (id === 'git-pull-action' || id === 'git-pull-rebase-action') this.run(() => this.repo.action({pull:{remote:null,branch:null,rebase:id === 'git-pull-rebase-action'}}),cx);
    else if (id === 'git-push-action') this.run(() => this.actions.invoke(this.menus.push().value),cx);
    cx.notify();
  }
  tree(event,cx) {
    if (event.tree !== 'git-tree') return;
    if (event.kind === 'menu') { this.run(() => this.actions.invoke(event.value),cx); return; }
    const node = this.nodes.get(event.id);
    if (!node) return;
    if (event.kind === 'expand' || event.kind === 'collapse') this.expanded.set(event.id,event.kind === 'expand');
    else if (event.kind === 'check') {
      const staged = node.scope === 'staged', selected = checkbox(node.entries);
      const operation = staged ? 'unstage' : selected.operation;
      const paths = staged ? node.entries.filter(entry => !entry.conflicted && entry.staged !== null).map(entry => entry.path) : selected.paths;
      this.run(() => this.repo.index(paths,operation),cx);
    } else if (event.kind === 'select' || event.kind === 'open') {
      if (node.row.entry) this.run(() => this.repo.open({kind:'file',path:node.row.entry.path,scope:node.scope}),cx);
    }
    cx.notify();
  }
  release() { this.repo.stop(); }
  render() {
    return ActionScope.new('git-actions-scope',{close:'git-close-current'}).child(
      ['git-commit-action','git-amend-action','git-fetch-action','git-pull-action','git-pull-rebase-action','git-push-action']
        .reduce((scope,id) => scope.on_action(id,(_event,cx) => this.control(id,cx)),div().v_flex().size_full().min_w_0().min_h_0().key_context(this.shortcutContext))
        .children([
      div().v_flex().size_full().min_h_0().min_w_0().child(this.mode === 'embedded' && !this.repo.selected ? views.details(this)
        : div().v_flex().size_full().min_h_0().min_w_0().children(this.mode === 'main' ? [views.header(this)] : [])
          .child(div().flex_1().min_h_0().min_w_0().child(Workspace.new('git-workspace',{navigation:'none',default_details_width:320,min_details_width:260})
            .children([this.mode === 'embedded'
              ? div().v_flex().size_full().min_h_0().min_w_0().child(views.header(this))
                .child(div().flex_1().min_h_0().min_w_0().child(views.content(this)))
              : views.content(this),views.details(this)])))),views.dialogs(this)]));
  }
}
