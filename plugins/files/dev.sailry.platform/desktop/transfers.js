// Clipboard selection and transfer dialogs retain the original Files workflow.
import {captureClipboard, readClipboard, preparePaste, prepareMove, selectUpload, startDownload,
  readTransfers, nextTransferChange, transferAction} from 'sailry/file-transfers';
import {join} from './explorer.js';

export class Transfers {
  constructor(report,changed = () => {},text = {}) {
    this.report = report;
    this.text = text;
    this.changed = changed;
    this.state = readTransfers();
    this.clipboard = readClipboard();
    this.current = null; this.queue = null; this.busy = false; this.advancing = null; this.capturing = null;
  }

  value() { return this.state.transfers.find(value => value.id === this.current); }

  accept(state,cx) {
    if (BigInt(state.cursor) < BigInt(this.state.cursor)) return;
    for(const value of state.transfers) {
      const previous=this.state.transfers.find(entry=>entry.id===value.id);
      if(previous?.stage===value.stage&&JSON.stringify(previous.error)===JSON.stringify(value.error))continue;
      if(['failed','uncertain','done','cancelled'].includes(value.stage)) {
        const key=status(value,this.text);if(key)this.report(key,['done','cancelled'].includes(value.stage)?'info':'error');
      }
    }
    this.state = state; this.changed(this.value()); cx?.notify();
  }

  refresh(cx) { this.accept(readTransfers(),cx); }

  observe(cx) {
    cx.spawn(async cx => {
      let cursor = this.state.cursor;
      try { while (true) {
        const state = await nextTransferChange(cursor); cursor = state.cursor;
        this.accept(state,cx);
        if (this.value()?.stage === 'done' && this.queue?.paths.length) await this.next(cx);
      } } catch (_) { /* The core retains admitted transfers after this view closes. */ }
    });
  }

  async capture(paths,cut,cx) {
    const pending = captureClipboard(paths,cut);
    this.capturing = pending;
    try { this.clipboard = await pending; cx.notify(); }
    finally { if (this.capturing === pending) this.capturing = null; }
  }

  canPaste() { this.clipboard = readClipboard(); return !!this.clipboard?.available; }

  async paste(directory,cx) {
    if (this.capturing) await this.capturing;
    if (!this.canPaste()) { this.report('files_paste_unavailable'); return; }
    this.queue = {clipboard:this.clipboard.id,paths:[...this.clipboard.paths],directory};
    await this.next(cx);
  }

  async next(cx) {
    if (this.advancing) return this.advancing;
    const queue = this.queue, entry = queue?.paths.shift();
    if (!entry) return;
    this.advancing = (async () => {
      const transfer = await preparePaste(queue.clipboard,entry.path,join(queue.directory,entry.path.split('/').at(-1)));
      if (this.queue !== queue) return;
      this.current = transfer.id; this.refresh(cx);
    })();
    try { await this.advancing; }
    finally { this.advancing = null; }
  }

  async upload(directory,cx) {
    const transfer = await selectUpload(directory);
    if (!transfer) return;
    this.queue = null; this.current = transfer.id; this.refresh(cx);
    if (transfer.can_start) await this.action('start',undefined,cx);
  }

  async move(source,path,cx) {
    const transfer = await prepareMove(source,path);
    this.queue = null; this.current = transfer.id; this.refresh(cx);
  }

  async download(path,cx) {
    const transfer = await startDownload(path);
    if (!transfer) return;
    this.queue = null; this.current = transfer.id; this.refresh(cx);
  }

  async action(kind,path,cx) {
    const id = this.current;
    if (!id || this.busy) return;
    this.busy = true; cx.notify();
    try {
      await transferAction(id,{kind,...(path === undefined ? {} : {path})});
      this.refresh(cx);
      if (this.current === id && this.value()?.stage === 'done' && this.queue?.paths.length) await this.next(cx);
    } finally { this.busy = false; cx.notify(); }
  }

  async close(cx) {
    const value = this.value();
    if (!value) return;
    this.queue = null;
    await transferAction(value.id,{kind:value.can_cancel ? 'cancel' : 'dismiss'});
    if (this.current === value.id) this.current = null;
    this.refresh(cx);
  }
}

export function status(value,text = {}) {
  const prefix = value.kind === 'upload' ? 'files_upload' : value.kind === 'download' ? 'files_download' : 'files_paste';
  if (value.kind === 'move' && value.stage === 'recycling') return 'files_move_recycling';
  if (value.kind === 'move' && value.published && ['failed','uncertain'].includes(value.stage)) return 'files_move_partial';
  if (value.error?.message && Object.hasOwn(text,value.error.message)) return value.error.message;
  if (value.stage === 'uncertain') return value.kind === 'move' ? 'files_move_unknown' : `${prefix}_unknown`;
  if (value.stage === 'exists') return value.kind === 'upload' ? 'files_upload_exists' : 'files_paste_conflict';
  if (value.stage === 'transferring') return value.kind === 'upload' ? 'files_upload_sending'
    : value.kind === 'download' ? 'files_download_receiving' : 'files_paste_copying';
  if (value.stage === 'ready') return null;
  if (value.stage === 'done' && ['copy','move'].includes(value.kind)) return null;
  if (value.stage === 'failed') {
    if (value.error?.code === 'busy') return 'files_operation_busy';
    if (['conflict','revision_conflict'].includes(value.error?.code)) return `${prefix}_conflict`;
    if (value.error?.code === 'permission_denied') return value.kind === 'download' ? 'files_download_destination' : `${prefix}_denied`;
  }
  return `${prefix}_${value.stage}`;
}
