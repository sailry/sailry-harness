import {View} from 'gpui-kit';
import {context} from 'sailry';
import {readComputerPermissions, requestComputerPermission, nextChange} from 'sailry/sdk';
import {nextWindowActivation, nextControlEvent,toast} from 'sailry/ui';
import {messages} from './locales.js';
import {render} from './view.js';

export default class Settings extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.permissions = null; this.pending = false; this.refresh = false;
    this.error = null; this.connected = true;
    this.read(null,cx);
    cx.spawn(async cx => {
      let cursor;
      try { while (true) {
        const event = await nextWindowActivation(cursor);
        if (cursor !== undefined && event.active) this.read(null,cx);
        cursor = event.cursor;
      } } catch (_) { /* Closing settings releases activation observation. */ }
    });
    cx.spawn(async cx => {
      let cursor;
      try { while (true) {
        const event = await nextChange(cursor); cursor = event.cursor;
        const restored = !this.connected && event.connected;
        this.connected = event.connected;
        if (restored) this.read(null,cx);
        cx.notify();
      } } catch (_) { /* Closing settings releases the Node subscription. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const {id} = await nextControlEvent();
        if (id === 'computer-permissions-refresh') this.read(null,cx);
      } } catch (_) { /* Closing settings releases its controls. */ }
    });
  }
  value() { return this.connected ? this.permissions : null; }
  canRequest(permission) {
    const value = this.value();
    return !!value && !this.error && value.local && value.platform === 'macos' && value[permission] === false;
  }
  status(permission) {
    if (this.pending) return 'computer_permissions_checking';
    if (!this.connected) return 'plugins_disconnected';
    const granted = this.value()?.[permission];
    return granted === true ? 'computer_permission_granted'
      : granted === false ? 'computer_permission_missing' : 'computer_permission_unknown';
  }
  read(permission, cx) {
    if (!this.connected) return;
    if (this.pending) { this.refresh ||= permission === null; return; }
    if (permission !== null && !this.canRequest(permission)) return;
    this.pending = true; this.error = null; cx.notify();
    cx.spawn(async cx => {
      try {
        this.permissions = permission === null ? await readComputerPermissions()
          : await requestComputerPermission(permission);
      } catch (_) {
        this.error = permission === null ? 'computer_permissions_failed' : 'computer_permissions_open_failed';
        toast({id:'computer-permissions-error',message:this.text[this.error],kind:'error'});
      } finally {
        this.pending = false;
        const refresh = this.refresh; this.refresh = false;
        if (refresh) this.read(null,cx);
        cx.notify();
      }
    });
  }
  render() { return render(this); }
}
