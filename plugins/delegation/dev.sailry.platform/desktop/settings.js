// Captured role drafts and durable retry receipts preserve Sailry ef5e82ea.
import {View} from 'gpui-kit';
import {context} from 'sailry';
import {listRoles,listModels,newRoleId,prepareRole,prepareRemoveRole,completeRequest,
  forgetRequest,nextChange,faultCode} from 'sailry/sdk';
import {createText,readText,setText,releaseText,nextTextEvent} from 'sailry/forms';
import {nextControlEvent,nextMenuEvent,modal_closed,toast,dismissToast} from 'sailry/ui';
import * as draft from './draft.js';
import {messages} from './locales.js';
import {render} from './view.js';

export default class Settings extends View {
  init(_props,cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.roles = []; this.models = []; this.ready = false; this.connected = true;
    this.loading = false; this.refresh = false; this.error = null; this.dialog = null; this.serial = 0;
    this.load(cx);
    cx.spawn(async cx => {
      let cursor;
      try { while (true) {
        const event = await nextChange(cursor), first = cursor === undefined;
        cursor = event.cursor; this.connected = event.connected;
        if (event.connected && !first) this.load(cx);
        cx.notify();
      } } catch (_) { /* The captured page owns this subscription. */ }
    });
    cx.spawn(async cx => {
      try { while (true) this.control(await nextControlEvent(),cx); }
      catch (_) { /* The page owns its controls. */ }
    });
    cx.spawn(async cx => {
      try { while (true) this.menu(await nextMenuEvent(),cx); }
      catch (_) { /* The page owns its menus. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const id = await modal_closed();
        if (this.dialog?.token === id) this.close(cx);
      } } catch (_) { /* The page owns its modal. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextTextEvent(), owner = this.dialog;
        if (owner?.kind === 'edit' && !owner.request && Object.values(owner.fields).includes(event.id)
          && event.kind === 'change') { owner.error = null; cx.notify(); }
      } } catch (_) { /* Native fields are released with the draft. */ }
    });
  }

  load(cx) {
    if (!this.connected) return;
    if (this.loading) { this.refresh = true; return; }
    this.loading = true;
    cx.spawn(async cx => {
      try {
        const [roles,models] = await Promise.all([listRoles(),listModels()]);
        this.roles = roles; this.models = draft.catalog(models); this.ready = true; this.error = null;
      } catch (_) { this.error = 'role_failed'; toast({id:'role-catalog',message:this.text[this.error],kind:'error'}); }
      finally {
        this.loading = false;
        const refresh = this.refresh; this.refresh = false;
        if (refresh) this.load(cx);
        cx.notify();
      }
    });
  }

  edit(profile,cx) {
    if (!this.connected || !this.ready) return;
    this.close(cx);
    const owner = draft.create(profile,newRoleId(),this.models), value = owner.original;
    owner.kind = 'edit'; owner.token = `role-dialog-${++this.serial}`;
    owner.fields = {
      key:createText(value.key,{label:this.text.role_id,placeholder:this.text.form_role_id_hint}),
      name:createText(value.name,{label:this.text.settings_name,placeholder:this.text.form_name_hint}),
      turns:createText(value.max_turns === null ? '' : String(value.max_turns),
        {label:this.text.role_max_turns,placeholder:this.text.form_turns_hint}),
      instructions:createText(value.instructions,{multiline:true,rows:[3,5],label:this.text.role_instructions,
        placeholder:this.text.form_role_instructions_hint})
    };
    this.dialog = owner; cx.notify();
  }

  remove(profile,cx) {
    if (!this.connected || !this.ready) return;
    this.close(cx);
    this.dialog = {kind:'remove',token:`role-dialog-${++this.serial}`,original:JSON.parse(JSON.stringify(profile)),
      request:null,pending:false,error:null,closed:false};
    cx.notify();
  }

  close(cx) {
    const owner = this.dialog;
    if (owner) {
      owner.closed = true;
      for (const handle of Object.values(owner.fields ?? {})) releaseText(handle);
      dismissToast(owner.token);
    }
    this.dialog = null; cx.notify();
  }

  control(event,cx) {
    const owner = this.dialog;
    if (event.id === 'role' && owner?.kind === 'edit' && !owner.request) {
      owner.appearance = event.value; owner.error = null; cx.notify();
    } else if (event.id.startsWith('role-edit-')) {
      const role = this.roles.find(role => `role-edit-${role.id}` === event.id);
      if (role) this.edit(role,cx);
    } else if (event.id.startsWith('role-delete-')) {
      const role = this.roles.find(role => `role-delete-${role.id}` === event.id);
      if (role) this.remove(role,cx);
    }
  }

  menu(event,cx) {
    const owner = this.dialog;
    if (owner?.kind !== 'edit' || owner.request || !event.id.startsWith(`${owner.token}:`)) return;
    const value = event.id.slice(owner.token.length+1);
    if (event.menu === 'role-source') draft.source(owner,value === 'fixed');
    else if (event.menu === 'role-model') {
      const model = owner.models.find(model => model.id === value);
      if (model) draft.selectModel(owner,model);
    } else if (event.menu === 'role-effort' && owner.model) {
      const effort = draft.efforts(owner).find(effort => JSON.stringify(effort) === value);
      if (effort !== undefined) { owner.model.effort = effort; owner.error = null; }
    } else if (event.menu === 'role-presets' && ['review','research','implement'].includes(value)) {
      setText(owner.fields.instructions,this.text[`role_prompt_${value}`]); owner.error = null;
    }
    cx.notify();
  }

  fail(owner,key) {
    owner.error = key;
    if (!owner.closed) toast({id:owner.token,message:this.text[key],kind:'error'});
  }

  save(cx) {
    const owner = this.dialog;
    if (!owner || owner.pending || !this.connected) return;
    if (!owner.request) {
      try {
        owner.request = owner.kind === 'remove'
          ? prepareRemoveRole(owner.original.id,owner.original.revision)
          : prepareRole(draft.profile(owner,Object.fromEntries(Object.entries(owner.fields)
            .map(([key,handle]) => [key,readText(handle)]))),owner.original.revision);
      } catch (error) {
        const key = error.message === 'role_invalid' ? 'role_invalid' : draft.errorKey(faultCode(error.message ?? String(error)));
        this.fail(owner,key); cx.notify(); return;
      }
    }
    owner.pending = true; owner.error = null; dismissToast(owner.token); cx.notify();
    cx.spawn(async cx => {
      try {
        const result = await completeRequest(owner.request);
        if (result.Ok?.kind === (owner.kind === 'edit' ? 'role' : 'roles')) {
          forgetRequest(owner.request); owner.request = null;
          if (this.dialog === owner) this.close(cx);
          this.load(cx);
        } else if (result.Err) {
          this.fail(owner,draft.errorKey(result.Err.code));
          if (!draft.uncertain(result.Err.code)) { forgetRequest(owner.request); owner.request = null; }
        } else this.fail(owner,'role_unknown');
      } catch (_) { this.fail(owner,'role_unknown'); }
      finally { owner.pending = false; cx.notify(); }
    });
  }

  render() { return render(this); }
}
