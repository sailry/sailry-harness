// Tab and command policy from Sailry 116aab0f's Files page and DocumentTabs.
// Native document handles retain text, undo, drafts and pending publications.
import {readDocuments, nextDocumentChange, openDocument, documentAction,
  saveDocument, closeDocument} from 'sailry/documents';

export class Documents {
  constructor(report) {
    this.report = report;
    this.state = {cursor:'0',documents:[],reveal:null};
    this.order = [];
    this.selected = null;
    this.selection = 0;
    this.revealed = '0';
    this.errors = new Map();
    this.accept(readDocuments());
  }

  items() {
    const documents = new Map(this.state.documents.map(document => [document.id,document]));
    return this.order.map(id => documents.get(id)).filter(Boolean);
  }

  current() { return this.state.documents.find(document => document.id === this.selected); }

  accept(state, cx) {
    if (BigInt(state.cursor) < BigInt(this.state.cursor)) return false;
    this.state = state;
    const selectedIndex = this.order.indexOf(this.selected);
    const present = new Set(state.documents.map(document => document.id));
    this.order = this.order.filter(id => present.has(id));
    for (const document of state.documents) {
      if (!this.order.includes(document.id)) this.order.push(document.id);
      const signature = document.error ? JSON.stringify(document.error) : null;
      if (signature && this.errors.get(document.id) !== signature) this.report(document.error,document);
      if (signature) this.errors.set(document.id,signature);
      else this.errors.delete(document.id);
    }
    for (const id of this.errors.keys()) if (!present.has(id)) this.errors.delete(id);
    if (state.reveal && present.has(state.reveal.document) &&
        BigInt(state.reveal.sequence) > BigInt(this.revealed)) {
      this.revealed = state.reveal.sequence;
      this.selected = state.reveal.document;
      ++this.selection;
    }
    if (!present.has(this.selected)) this.selected = this.order[Math.min(Math.max(selectedIndex,0),this.order.length - 1)] ?? null;
    cx?.notify();
    return true;
  }

  observe(cx) {
    cx.spawn(async cx => {
      let cursor = this.state.cursor;
      try { while (true) {
        const state = await nextDocumentChange(cursor);
        cursor = state.cursor;
        this.accept(state,cx);
      } } catch (_) { /* Closing this view releases its observation, not its drafts. */ }
    });
  }

  refresh(cx) { return this.accept(readDocuments(),cx); }

  async open(path, line, cx) {
    await openDocument(path,line === undefined ? {} : {line});
    this.refresh(cx);
  }

  async select(id, cx) {
    if (!this.order.includes(id)) return;
    this.selected = id;
    ++this.selection;
    cx.notify();
    await documentAction(id,{kind:'focus'});
    this.refresh(cx);
  }

  async action(kind, cx) {
    const id = this.selected;
    if (!id) return;
    await documentAction(id,{kind});
    this.refresh(cx);
  }

  async save(cx) {
    const document = this.current();
    if (!document?.can_save) return;
    await saveDocument(document.id);
    this.refresh(cx);
  }

  async close(id, cx) {
    if (!this.order.includes(id)) return;
    await this.remove(id,{confirm:true},cx);
  }

  async remove(id, options, cx) {
    const active = this.selected === id, selection = this.selection;
    const closed = await closeDocument(id,options);
    this.refresh(cx);
    // Snapshots may select the successor before the close reply. Only an explicit
    // selection or reveal supersedes this focus handoff, not that reconciliation.
    if (closed && active && selection === this.selection && this.selected) {
      await documentAction(this.selected,{kind:'focus'});
    }
    return closed;
  }
}
