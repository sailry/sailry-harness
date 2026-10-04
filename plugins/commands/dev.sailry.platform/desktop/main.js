import {View} from 'gpui-kit';
import {context} from 'sailry';
import {listTerminals, listTerminalTools, prepareTerminal, prepareOpenTerminal, prepareCloseTerminal,
  completeRequest, forgetRequest, nextChange} from 'sailry/sdk';
import {nextMenuEvent, pane, openPane, updatePane, closePane, nextPaneEvent,toast} from 'sailry/ui';
import {messages} from './locales.js';
import {render} from './view.js';
import {title, directory} from './labels.js';

export default class Commands extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.resource = pane()?.resource;
    this.items = []; this.tools = []; this.selected = this.resource?.id ?? null; this.reveal = null;
    this.loading = false; this.dirty = false; this.busy = false; this.pending = null; this.error = null;
    this.openAttempts = new Map(); this.openRetry = null;
    if (this.resource) cx.spawn(async cx => {
      try { while (true) {
        const event = await nextPaneEvent();
        if (event.kind === 'close') this.perform('close', this.resource.id, cx);
      } } catch (_) { /* View release ends pane observation. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextMenuEvent();
        if (event.menu === 'terminal-tools') this.perform('create', event.id, cx);
      } } catch (_) { /* View release ends menu observation. */ }
    });
    cx.spawn(async cx => {
      let cursor = '';
      try { while (true) {
        const change = await nextChange(cursor); cursor = change.cursor;
        if (change.connected) this.refresh(cx);
      } } catch (_) { /* The captured view has been released. */ }
    });
  }
  refresh(cx) {
    if (this.loading) { this.dirty = true; return; }
    this.loading = true;
    cx.spawn(async cx => {
      try { do {
        this.dirty = false;
        const selected = this.selected;
        const [items, tools] = await Promise.all([listTerminals(), listTerminalTools()]);
        this.items = items.filter(item => item.status.kind !== 'closed'); this.tools = tools;
        if (this.resource) {
          const index = this.items.findIndex(item => item.id === this.resource.id);
          if (index < 0) { closePane(); return; }
          updatePane({title:this.label(this.items[index], index)});
        }
        if (selected === this.selected && !this.items.some(item => item.id === selected))
          this.selected = this.items[0]?.id ?? null;
        if (this.reveal) {
          const index = this.items.findIndex(item => item.id === this.reveal);
          if (index >= 0 && openPane({resource:{kind:'terminal',id:this.reveal},title:this.label(this.items[index], index)}))
            this.reveal = null;
        }
        if (this.error === 'loadFailed') this.error = null;
        cx.notify();
      } while (this.dirty); }
      catch (_) { this.fail('loadFailed'); }
      finally { this.loading = false; this.openSelected(cx); cx.notify(); }
    });
  }
  fail(key) { if(this.error!==key)toast({id:'terminal-error',message:this.text[key],kind:'error'});this.error=key; }
  select(index, cx) {
    this.selected = this.items[index]?.id ?? null;
    if (!this.pending && this.error === 'failed' && this.openRetry && this.selected !== this.openRetry) {
      this.error = null; this.openRetry = null;
    }
    this.openSelected(cx); cx.notify();
  }
  openSelected(cx) {
    const item = this.items.find(item => item.id === this.selected);
    if (this.busy || this.pending || !item || item.status.kind !== 'stopped' || item.tool || item.ssh) return;
    if (this.error) {
      const failed = this.openAttempts.get(this.openRetry);
      if (this.error !== 'failed' || !this.openRetry
        || (this.openRetry === item.id && failed?.revision === item.revision)) return;
      this.error = null; this.openRetry = null;
    }
    const attempted = this.openAttempts.get(item.id);
    if (attempted && attempted.revision === item.revision) {
      if (attempted.failed) { this.error = 'failed'; this.openRetry = item.id; cx.notify(); }
      return;
    }
    this.perform('open', item.id, cx);
  }
  openFailed(target) {
    this.openRetry = target;
    const attempted = this.openAttempts.get(target);
    if (attempted) attempted.failed = true;
  }
  retry(cx) {
    if (this.pending) this.perform(null, null, cx);
    else if (this.openRetry) this.perform('open', this.openRetry, cx);
    else this.refresh(cx);
  }
  perform(action, target, cx) {
    if (this.busy || (this.pending ? action !== null : action === null)) return;
    this.busy = true; this.error = null; this.openRetry = null; cx.notify();
    if (action === 'open') this.openAttempts.set(target, {
      revision:this.items.find(item => item.id === target)?.revision, failed:false,
    });
    if (this.resource) updatePane({busy:true});
    cx.spawn(async cx => {
      let closed = false;
      try {
        if (!this.pending) {
          const id = action === 'close' ? prepareCloseTerminal(target)
            : action === 'open' ? prepareOpenTerminal(target) : prepareTerminal(target);
          this.pending = {id, action, target};
        }
        const result = await completeRequest(this.pending.id);
        const done = this.pending.action, opened = this.pending.target;
        if (result.Err?.code === 'outcome_unknown') { this.fail('unknown'); return; }
        if (result.Err) {
          this.fail('failed');
          if (done === 'open') this.openFailed(opened);
        }
        else if (done !== 'close') {
          if (done !== 'open' || this.selected === opened) this.selected = result.Ok.data.id;
          if (!this.resource && done !== 'open') this.reveal = this.selected;
        }
        forgetRequest(this.pending.id); this.pending = null;
        if (!result.Err && done === 'close' && this.resource) { closed = true; return; }
        this.refresh(cx);
      } catch (_) {
        this.fail(this.pending ? 'unknown' : 'failed');
        if (!this.pending && action === 'open') this.openFailed(target);
      }
      finally {
        this.busy = false;
        try { if (this.resource) updatePane({busy:!!this.pending}); } catch (_) { /* The renderer may have been released. */ }
        this.openSelected(cx); cx.notify();
        if (closed) closePane();
      }
    });
  }
  label(item, index) {
    return title(item.title) ?? this.text.tools[item.tool] ?? directory(item.directory)
      ?? `${this.text.terminal} ${index + 1}`;
  }
  render(cx) { return render(this); }
}
