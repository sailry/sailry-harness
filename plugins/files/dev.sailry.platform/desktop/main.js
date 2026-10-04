// Page policy is package-owned; document and transfer state stays in core.
import {View, div} from 'gpui-kit';
import {context, next_change} from 'sailry';
import {createText, readText, releaseText, setText, focusText, nextTextEvent} from 'sailry/forms';
import {Workspace, workspaceMode, FilePreview, fileResource, inspectFilePath, openFileImage, toast, Modal, ActionScope, closePanel,
  nextControlEvent, nextNavigationTabEvent, nextMenuEvent, nextTreeEvent,
  nextContextMenuEvent, modal_closed, canInsertFileReferences} from 'sailry/ui';
import {openSystem} from 'sailry/file-transfers';
import {faultCode} from 'sailry/sdk';
import {refreshDocuments} from 'sailry/documents';
import {Documents} from './documents.js';
import {Explorer, parent, join, within} from './explorer.js';
import {Search} from './search.js';
import {Mutation} from './mutations.js';
import {Transfers} from './transfers.js';
import {Actions} from './actions.js';
import {messages} from './locales.js';
import * as documentView from './document-view.js';
import {render as explorerView} from './explorer-view.js';
import * as dialogs from './dialogs.js';

export default class Files extends View {
  init(_props,cx) {
    this.resource = fileResource();
    if (this.resource) return;
    this.mode = workspaceMode();
    this.text = messages(JSON.parse(context()).locale);
    this.documents = new Documents(error => this.documentError(error));
    this.explorer = new Explorer(key => this.report(key));
    this.transfers = new Transfers((key,kind) => this.report(key,kind),value => this.transferDraft(value),this.text);
    this.actions = new Actions(this);
    this.search = null; this.editing = null; this.dialogId = 0; this.canInsert = false;
    this.transferInput = null; this.transferId = null;
    this.documents.observe(cx); this.transfers.observe(cx);
    this.run(async cx => { this.canInsert = await canInsertFileReferences(); await this.explorer.refresh(cx); },cx);
    this.watch(nextControlEvent,(event,cx) => this.control(event.id,cx),cx);
    this.watch(nextNavigationTabEvent,(event,cx) => {
      if (event.bar !== 'file-tabs') return;
      this.run(cx => event.kind === 'close' ? this.documents.close(event.id,cx) : this.documents.select(event.id,cx),cx);
    },cx);
    this.watch(nextMenuEvent,(event,cx) => this.run(cx => this.actions.invoke(event.id,cx),cx),cx);
    this.watch(nextContextMenuEvent,(event,cx) => this.run(cx => this.actions.invoke(event.id,cx),cx),cx);
    this.watch(nextTreeEvent,(event,cx) => this.run(cx => this.tree(event,cx),cx),cx);
    this.watch(nextTextEvent,(event,cx) => this.textEvent(event,cx),cx);
    this.watch(modal_closed,(id,cx) => {
      if (id === `files-entry-${this.editing?.id}`) this.closeEdit(cx);
      if (id === `files-transfer-${this.transfers.current}`) this.run(cx => this.transfers.close(cx),cx);
    },cx);
    cx.spawn(async cx => {
      let seen = '';
      try { while (true) {
        const change = await next_change(seen); seen = change;
        const state = JSON.parse(change);
        this.explorer.connected = state.connected;
        if (state.connected) { refreshDocuments(); await this.explorer.refresh(cx); }
        this.canInsert = await canInsertFileReferences(); this.actions.prepareTree(); cx.notify();
      } } catch (_) { /* Releasing the view stops observation, not Node work. */ }
    });
  }

  watch(read,handle,cx) {
    cx.spawn(async cx => {
      try { while (true) { handle(await read(),cx); cx.notify(); } }
      catch (_) { /* Releasing the view closes its event stream. */ }
    });
  }

  run(action,cx) {
    cx.spawn(async cx => {
      try { await action(cx); }
      catch (error) {
        let fault = error;
        try { fault = JSON.parse(error.message); } catch (_) { /* Plain runtime failures carry no structured Fault. */ }
        this.report(this.text[fault?.message] ? fault.message : faultCode(error.message ?? String(error)) === 'busy' ? 'files_operation_busy' : 'files_open_failed');
      }
      finally { this.actions.prepareTree(); cx.notify(); }
    });
  }

  report(key,kind='error') { toast({id:`files-${kind}`,message:this.text[key] ?? this.text.files_read_failed,kind}); }

  documentError(error) {
    this.report(error.code === 'outcome_unknown' ? 'files_outcome_unknown'
      : ['conflict','revision_conflict'].includes(error.code) ? 'files_save_conflict'
      : error.code === 'invalid_request' ? 'files_read_failed' : 'files_save_failed');
  }

  pathItems(document) { return this.actions.path(document); }

  async open(path,line,cx) {
    const type = inspectFilePath(path);
    if (type.image) openFileImage(path);
    else if (type.external) await openSystem(path);
    else {
      try { await this.documents.open(path,line,cx); }
      catch (error) {
        if (faultCode(error.message ?? String(error)) !== 'invalid_request') throw error;
        await openSystem(path);
      }
    }
  }

  async tree(event,cx) {
    if (event.tree !== 'files-tree') return;
    const {id,kind} = event;
    if (kind === 'menu') return this.actions.invoke(event.action,cx);
    if (kind === 'command') return this.actions.command(event.action,cx);
    if (kind === 'select') { this.explorer.select(id,event); cx.notify(); return; }
    if (kind === 'context') { this.explorer.context(id); cx.notify(); return; }
    if (kind === 'expand') return this.explorer.expand(id,cx);
    if (kind === 'collapse') return this.explorer.collapse(id,cx);
    if (kind === 'open') {
      if (id.endsWith('\0more')) return this.explorer.load(id.slice(0,-5),true,cx);
      if (id.includes('\0')) return;
      if (this.explorer.entry(id)?.kind === 'directory') return;
      return this.open(id,undefined,cx);
    }
    if (kind === 'drop') {
      const source = event.source, directory = this.explorer.directory(id);
      if (!source || parent(source) === directory || within(directory,source)) return;
      return this.transfers.move(source,join(directory,source.split('/').at(-1)),cx);
    }
  }

  control(id,cx) {
    if (id === 'files-close-current') {
      const current = this.documents.current();
      if (current) this.run(cx => this.documents.close(current.id,cx),cx);
      else closePanel();
      return;
    }
    if (id === 'file-search-toggle') {
      if (this.search) { this.search.close(cx); this.search = null; }
      else this.search = new Search(this.text,key => this.report(key));
      cx.notify(); return;
    }
    if (id === 'file-search-run' && this.search) {
      if (this.search.running) this.search.reset(cx);
      else this.run(cx => this.search.run(cx),cx);
      return;
    }
    if (id === 'file-search-case' && this.search) { this.search.toggle('case_sensitive',cx); return; }
    if (id === 'file-search-regex' && this.search) { this.search.toggle('regex',cx); return; }
    if (id === 'files_save') this.run(cx => this.documents.save(cx),cx);
    else if (['files_undo','files_redo','files_copy','files_cut','files_paste','files_find'].includes(id)) {
      this.run(cx => this.documents.action(id.slice(6),cx),cx);
    }
  }

  textEvent(event,cx) {
    if (this.search && [this.search.query,this.search.filter].includes(event.id)) {
      if (event.kind === 'change') this.search.reset(cx);
      if (event.kind === 'enter') this.run(cx => this.search.run(cx),cx);
    }
    if (event.kind === 'enter' && event.id === this.editing?.input) this.submit(cx);
    if (event.kind === 'enter' && event.id === this.transferInput && this.transfers.value()?.stage === 'ready') this.transfer('start',cx);
  }

  edit(kind,paths,directory,cx) {
    this.closeEdit(cx);
    const input = kind === 'trash' ? null : createText(kind === 'rename' ? paths[0].split('/').at(-1) : '',
      {label:this.text.files_entry_name,placeholder:this.text.form_filename_hint});
    this.editing = {id:++this.dialogId,operation:new Mutation(kind,paths,directory),input};
    if (input) focusText(input);
    cx.notify();
  }

  closeEdit(cx) {
    if (this.editing) {
      this.editing.operation.closed = true;
      if (this.editing.input) releaseText(this.editing.input);
    }
    this.editing = null; cx.notify();
  }

  recover(entry,cx) {
    const current = (this.documents.state.operations ?? []).find(value => value.id === entry.id);
    if (!current || current.running) return;
    const {action} = current;
    const kind = action.kind === 'rename' ? 'rename' : action.kind === 'trash' ? 'trash' : 'create';
    const paths = kind === 'rename' ? [action.from] : kind === 'trash' ? [action.path] : [parent(action.path)];
    this.edit(kind,paths,action.kind === 'create_directory',cx);
    if (this.editing.input) setText(this.editing.input,(action.to ?? action.path).split('/').at(-1));
    this.editing.operation.pending = {id:current.id,actions:[action]};
    this.editing.operation.error = kind === 'rename' ? 'files_rename_unknown'
      : kind === 'trash' ? 'files_trash_unknown' : 'files_create_unknown';
    this.report(this.editing.operation.error);
    cx.notify();
  }

  submit(cx) {
    const editing = this.editing;
    if (!editing) return;
    const name = editing.input ? readText(editing.input) : '';
    this.run(async cx => {
      await editing.operation.submit(name,cx);
      if (this.editing !== editing) return;
      if (editing.operation.done) {
        const operation = editing.operation;
        this.closeEdit(cx); await this.explorer.refresh(cx);
        if (operation.kind === 'create' && !operation.directory) await this.open(operation.completed[0].path,undefined,cx);
      } else if (editing.operation.error) this.report(editing.operation.error);
    },cx);
  }

  transferDraft(value) {
    if ((value?.id ?? null) === this.transferId) return;
    if (this.transferInput) releaseText(this.transferInput);
    this.transferId = value?.id ?? null;
    this.transferInput = value ? createText(value.path.split('/').at(-1),{label:this.text.files_entry_name,placeholder:this.text.form_filename_hint}) : null;
    if (value?.stage === 'ready' && this.transferInput) focusText(this.transferInput);
  }

  transfer(kind,cx) {
    const value = this.transfers.value();
    if (!value) return;
    const path = value.stage === 'ready' && this.transferInput
      ? join(parent(value.path),readText(this.transferInput)) : undefined;
    this.run(cx => this.transfers.action(kind,path,cx),cx);
  }

  render() {
    if (this.resource) return FilePreview.new('file-preview');
    return ActionScope.new('files-actions',{close:'files-close-current'}).children([
      div().id('files-panel').v_flex().size_full().min_h_0().min_w_0()
      .child(this.mode === 'embedded' && !this.documents.current() ? explorerView(this)
        : div().v_flex().size_full().min_h_0().min_w_0()
          .children(this.mode === 'main' ? [documentView.header(this)] : [])
          .child(div().flex_1().min_h_0().min_w_0().child(Workspace.new('files-workspace',{navigation:'none',default_details_width:320,min_details_width:260,details_label:this.text.file_tree})
            .children([this.mode === 'embedded'
              ? div().v_flex().size_full().min_h_0().min_w_0().child(documentView.header(this))
                .child(div().flex_1().min_h_0().min_w_0().child(documentView.content(this)))
              : documentView.content(this),explorerView(this)]))))
      .child(this.editing ? dialogs.mutation(this)
        : this.transfers.value() ? dialogs.transfer(this) : Modal.new('files-dialog',{open:false}))]);
  }
}
