import {View, div} from 'gpui-kit';
import {context} from 'sailry';
import {listTerminals, listTerminalTools, prepareTerminal, completeRequest, forgetRequest,
  publishContributions, nextContributionEvent, nextChange} from 'sailry/sdk';
import {openPane, publishResourceTitles, toast, dismissToast, nextToastEvent} from 'sailry/ui';
import {messages} from './locales.js';
import {title, directory} from './labels.js';

const notification = 'terminal-launch';

export default class Project extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.tools = []; this.connected = false; this.loading = false; this.busy = false; this.ready = false;
    this.pending = null; this.error = null; this.loadError = null; this.reveal = null;
    this.refreshing = false; this.dirty = false;
    this.load(cx);
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextContributionEvent();
        if (event.handler === 'create' && !this.pending) this.start(null, event, cx);
        else if (event.value === 'refresh') this.load(cx, event);
        else if (event.value === 'retry' && this.pending) this.start(null, event, cx);
        else if (event.handler === 'cli' && !this.pending) this.start(event.value, event, cx);
        else this.publish(event);
      } } catch (_) { /* Releasing the captured project stops contribution events. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextToastEvent();
        if (event.id !== notification) continue;
        if (event.action === 'retry' && this.pending) this.start(null, null, cx);
        if (event.action === 'refresh' && !this.pending) this.load(cx);
      } } catch (_) { /* Releasing the captured project stops toast events. */ }
    });
    cx.spawn(async cx => {
      let cursor = '';
      try { while (true) {
        const change = await nextChange(cursor); cursor = change.cursor;
        this.connected = change.connected;
        this.publish();
        if (change.connected) this.refresh(cx);
      } } catch (_) { /* Releasing the captured project stops Node observation. */ }
    });
  }

  publish(event) {
    if (!this.ready) return;
    const reply = id => event?.id === id ? {reply_to:event.sequence} : {};
    const recovery = this.pending ? 'retry' : 'refresh';
    const choices = [];
    choices.push(...this.tools.map(entry => ({id:entry.tool,
      label:{label:this.text.tools[entry.tool]}, icon:'square-terminal', group:{label:this.text.cli},
      enabled:entry.available && !this.busy && !this.pending})));
    choices.push({id:recovery, label:{label:this.text[recovery]}, icon:'rotate-cw',
      group:{label:this.text[recovery]}, enabled:!this.busy && !this.loading});
    const enabled = this.connected && !this.busy;
    publishContributions([
      ...['terminal-new', 'terminal-menu-new'].map(id => ({id,
        enabled:enabled && !this.pending, ...reply(id)})),
      ...['terminal-cli', 'terminal-menu-cli'].map(id => ({id,
        enabled:enabled && !this.loading, choices, ...reply(id)})),
    ]);
  }

  load(cx, event) {
    if (this.loading || this.busy || this.pending) { this.publish(event); return; }
    this.loading = true; this.error = null; this.loadError = null; this.publish(event);
    if (this.ready) dismissToast(notification);
    cx.spawn(async () => {
      try { this.tools = await listTerminalTools(); }
      catch (_) { this.loadError = 'toolsFailed';toast({id:notification,message:this.text[this.loadError],kind:'error'}); }
      finally { this.loading = false; this.ready = true; this.publish(); }
    });
  }

  start(tool, event, cx) {
    if (this.busy || (tool !== null && this.loading) || (!this.pending && tool !== null &&
      !this.tools.some(entry => entry.tool === tool && entry.available))) {
      this.publish(event); return;
    }
    this.busy = true; this.error = null; this.publish(event);
    dismissToast(notification);
    cx.spawn(async cx => {
      try {
        if (!this.pending) this.pending = prepareTerminal(tool);
        const result = await completeRequest(this.pending);
        if (result.Err) this.error = result.Err.code === 'not_configured' ? 'unavailable' : 'launchFailed';
        else this.reveal = result.Ok.data.id;
        forgetRequest(this.pending); this.pending = null;
      } catch (_) { this.error = this.pending ? 'unknown' : 'launchFailed'; }
      finally {
        this.busy = false;
        this.publish();
        if (this.error) {
          const action = this.pending ? 'retry' : 'refresh';
          toast({id:notification, message:this.text[this.error], kind:'error',
            action:{id:action, label:this.text[action]}});
        } else this.refresh(cx);
      }
    });
  }

  refresh(cx) {
    if (this.refreshing) { this.dirty = true; return; }
    this.refreshing = true;
    cx.spawn(async () => {
      try { do {
        this.dirty = false;
        const id = this.reveal;
        const items = (await listTerminals()).filter(item => item.status.kind !== 'closed');
        const titles = items.map((item, index) => ({resource:{kind:'terminal', id:item.id},
          title:title(item.title) ?? this.text.tools[item.tool] ?? directory(item.directory)
            ?? `${this.text.terminal} ${index + 1}`}));
        publishResourceTitles(titles);
        const entry = titles.find(entry => entry.resource.id === id);
        if (entry && this.reveal === id && openPane(entry)) this.reveal = null;
      } while (this.dirty); }
      catch (_) { /* A later Node snapshot retries foreground navigation, never process creation. */ }
      finally { this.refreshing = false; }
    });
  }

  render() { return div(); }
}
