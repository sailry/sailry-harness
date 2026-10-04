// Preserve the captured Node and revision checks from Sailry 5d0ac251.
import {View} from 'gpui-kit';
import {context} from 'sailry';
import {readMediaSettings, listMediaModels, prepareMediaSettings,
  completeRequest, forgetRequest, nextChange} from 'sailry/sdk';
import {nextControlEvent, toast, dismissToast} from 'sailry/ui';
import {messages} from './locales.js';
import {render} from './view.js';

const notice = 'media-save';
const same = (left, right) => left?.provider === right?.provider && left?.model === right?.model;

export default class Settings extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.saved = {revision:0,bindings:{}}; this.models = [];
    this.ready = false; this.connected = true; this.pending = false;
    this.loading = false; this.refresh = false; this.error = null;
    this.choices = new Map(); this.choiceIds = new Map();
    this.load(cx);
    cx.spawn(async cx => {
      let cursor;
      try { while (true) {
        const event = await nextChange(cursor), first = cursor === undefined;
        cursor = event.cursor; this.connected = event.connected;
        if (event.connected && !first) this.load(cx);
        cx.notify();
      } } catch (_) { /* Closing settings releases the captured Node subscription. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextControlEvent();
        this.select(event.id,event.value,cx);
      } } catch (_) { /* Closing settings releases control events. */ }
    });
  }

  busy() { return this.pending || !this.connected || !this.ready; }

  label(kind) {
    if (!kind) return this.text.media_unavailable;
    const selected = this.saved.bindings[kind];
    if (!selected) return this.text.media_unconfigured;
    const provider = this.models.find(model => model.provider === selected.provider);
    return `${provider?.provider_name ?? this.text.media_model_unavailable} / ${selected.model}`;
  }

  items(menu, kind) {
    const selected = this.saved.bindings[kind];
    return [null,...this.models.filter(model => model.kinds.includes(kind))].map(model => {
      const binding = model ? {provider:model.provider,model:model.model} : null;
      const choice = {menu,kind,revision:this.saved.revision,binding};
      const key = JSON.stringify(choice);
      let id = this.choiceIds.get(key);
      if (!id) {
        id = `choice-${this.choices.size}`;
        this.choiceIds.set(key,id); this.choices.set(id,choice);
      }
      return {id,label:model ? `${model.provider_name} / ${model.model}` : this.text.media_unconfigured,
        checked:same(binding,selected),enabled:!this.busy()};
    });
  }

  accept(settings) {
    if (settings.revision >= this.saved.revision) this.saved = settings;
  }

  fail(key) {
    this.error = key;
    toast({id:notice,message:this.text[key],kind:'error'});
  }

  load(cx) {
    if (!this.connected) return;
    if (this.loading || this.pending) { this.refresh = true; return; }
    this.loading = true;
    cx.spawn(async cx => {
      try {
        const [settings,models] = await Promise.all([readMediaSettings(),listMediaModels()]);
        this.accept(settings); this.models = models; this.ready = true;
      } catch (_) {
        this.ready = false;
        this.fail('media_model_unavailable');
      } finally {
        this.loading = false;
        const refresh = this.refresh; this.refresh = false;
        if (refresh) this.load(cx);
        cx.notify();
      }
    });
  }

  select(menu, id, cx) {
    const choice = this.choices.get(id);
    if (!choice || choice.menu !== menu || this.busy()) return;
    if (choice.revision !== this.saved.revision) { this.fail('media_conflict'); cx.notify(); return; }
    const settings = {revision:choice.revision,bindings:{...this.saved.bindings}};
    if (choice.binding) settings.bindings[choice.kind] = {...choice.binding};
    else delete settings.bindings[choice.kind];
    this.pending = true; this.error = null; dismissToast(notice); cx.notify();
    cx.spawn(async cx => {
      let request;
      try {
        request = prepareMediaSettings(settings);
        const result = await completeRequest(request);
        if (result.Err) {
          const code = result.Err.code;
          this.fail(code === 'revision_conflict' ? 'media_conflict'
            : ['invalid_request','not_configured'].includes(code) ? 'media_model_unavailable'
            : ['unavailable','outcome_unknown'].includes(code) ? 'media_unknown' : 'media_failed');
        } else if (result.Ok?.kind === 'media_settings') this.accept(result.Ok.data);
        else this.fail('media_unknown');
      } catch (_) { this.fail(request ? 'media_unknown' : 'media_failed'); }
      finally {
        // Reconcile by reading the Node; an uncertain save is never replayed.
        if (request) forgetRequest(request);
        this.pending = false; this.refresh = false; this.load(cx); cx.notify();
      }
    });
  }

  render() { return render(this); }
}
