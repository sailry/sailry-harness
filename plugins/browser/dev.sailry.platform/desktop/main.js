import {View} from 'gpui-kit';
import {context} from 'sailry';
import {readBrowser, nextBrowserChange, browserAction, nextNavigationTabEvent,
  nextControlEvent, openSettings, toast} from 'sailry/ui';
import {createText, readText, setText, focusText, isTextFocused, nextTextEvent} from 'sailry/forms';
import {messages, errorKey} from './locales.js';
import {destination} from './address.js';
import {render} from './view.js';

export default class Browser extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.state = readBrowser();
    this.reported = new Map();
    this.address = createText(this.selected()?.url ?? '', {
      placeholder:this.text.browser_address, label:this.text.browser_address
    });
    this.accept(this.state, false, cx);
    cx.spawn(async cx => {
      let cursor = this.state.cursor;
      try { while (true) {
        const state = await nextBrowserChange(cursor); cursor = state.cursor;
        this.accept(state, false, cx);
      } } catch (_) { /* Releasing the captured resource ends observation. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextNavigationTabEvent();
        if (event.bar === 'browser-tabs') this.perform(event.kind, {id:Number(event.id)}, cx);
      } } catch (_) { /* The tab strip has been released. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextTextEvent();
        if (event.id === this.address && event.kind === 'enter') this.navigate(cx);
      } } catch (_) { /* The address field has been released. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const {id} = await nextControlEvent();
        if (id === 'browser-new-tab') this.perform('add', {}, cx);
        else if (id === 'browser-go') this.navigate(cx);
        else if (id === 'browser-settings') openSettings();
        else if (id === 'browser_back') this.perform('back', {}, cx);
        else if (id === 'browser_forward') this.perform('forward', {}, cx);
        else if (id === 'browser_reload') this.perform(this.selected()?.loading ? 'stop' : 'reload', {}, cx);
      } } catch (_) { /* The native controls have been released. */ }
    });
  }
  selected() { return this.state.tabs.find(tab => tab.id === this.state.selected); }
  accept(state, force, cx) {
    if (BigInt(state.cursor) < BigInt(this.state.cursor)) return false;
    const changed = state.selected !== this.state.selected;
    const previousUrl = this.selected()?.url ?? '';
    this.state = state;
    const url = this.selected()?.url ?? '';
    if (force || changed || (url !== previousUrl && !isTextFocused(this.address))) setText(this.address, url);
    for (const tab of state.tabs) {
      if (tab.error && this.reported.get(tab.id) !== tab.error) this.report(tab.error);
      this.reported.set(tab.id, tab.error);
    }
    for (const id of this.reported.keys()) {
      if (!state.tabs.some(tab => tab.id === id)) this.reported.delete(id);
    }
    cx.notify();
    return true;
  }
  report(error) {
    toast({id:'browser-navigation',kind:'error',message:this.text[errorKey(error)]});
  }
  navigate(cx) {
    const url = destination(readText(this.address));
    if (url === null) this.report('browser_invalid_address');
    else this.perform('navigate', {url}, cx);
  }
  perform(kind, args, cx) {
    cx.spawn(async cx => {
      try {
        const state = await browserAction({kind,...args});
        const tab = state.tabs.find(tab => tab.id === state.selected);
        const force = ['add','select'].includes(kind) || (kind === 'navigate' && !tab?.error);
        const accepted = this.accept(state, force, cx);
        if (accepted && kind === 'add') { focusText(this.address); cx.notify(); }
      } catch (error) { this.report(error); }
    });
  }
  render() { return render(this); }
}
