import { View, div } from "gpui-kit";
import { IconButton, ResourceTree, nextControlEvent, nextTreeEvent } from "sailry/ui";
import { DocumentSurface, readDocuments, nextDocumentChange, openDocument, documentAction, saveDocument, closeDocument, refreshDocuments } from "sailry/documents";
import { listDirectory, prepareFileAction, completeRequest, forgetRequest } from "sailry/sdk";

export default class Example extends View {
  init(_props, cx) {
    this.state = readDocuments(); this.current = null; this.items = []; this.status = "starting";
    const accept = value => { this.state = value; this.current = value.reveal?.document ?? value.documents[0]?.id ?? null; };
    accept(this.state);
    const list = async () => { const directory = await listDirectory(""); this.items = directory.entries.filter(entry => entry.kind === "file").map(entry => ({id:entry.name,label:entry.name})); this.status = "ready"; };
    cx.spawn(async cx => { await list(); cx.notify(); });
    cx.spawn(async cx => { while (true) { accept(await nextDocumentChange(this.state.cursor)); cx.notify(); } });
    cx.spawn(async cx => { while (true) { const event = await nextTreeEvent(); if (event.kind === "open") { await openDocument(event.id); accept(readDocuments()); cx.notify(); } } });
    cx.spawn(async cx => { while (true) {
      const event = await nextControlEvent();
      cx.spawn(async cx => {
      try {
        if (event.id === "document-focus") await documentAction(this.current,{kind:"focus"});
        if (event.id === "document-save") await saveDocument(this.current);
        if (event.id === "document-discard") await closeDocument(this.current,{discard:true});
        if (event.id === "document-close") await closeDocument(this.current,{confirm:true});
        if (event.id === "document-refresh") refreshDocuments();
        if (event.id === "document-rename") {
          const request = prepareFileAction({kind:"rename",from:"notes.txt",to:"renamed.txt"});
          try { const result = await completeRequest(request); if (result.Err) throw new Error(result.Err.message); }
          finally { forgetRequest(request); }
          await list(); this.status = "renamed";
        }
      } catch (_error) { this.status = "blocked"; }
      cx.notify();
      });
    } });
  }
  render() {
    const document = this.state.documents.find(document=>document.id===this.current);
    return div().h_flex().size_full()
      .child(div().w_48().h_full().child(ResourceTree.new("fixture-tree",{items:this.items,selected:[],current:null})))
      .child(div().v_flex().flex_1().min_w_0().min_h_0()
        .child(div().id(`document-status-${this.status}`).child(this.status))
        .child(div().h_flex().children([
          IconButton.new("document-focus",{icon:"file",label:"Focus document"}),
          IconButton.new("document-rename",{icon:"file",label:"Rename document"}),
          IconButton.new("document-save",{icon:"file",label:"Save document"}),
          IconButton.new("document-discard",{icon:"file",label:"Discard document"}),
          IconButton.new("document-close",{icon:"file",label:"Close document"}),
          IconButton.new("document-refresh",{icon:"file",label:"Refresh documents"})]))
        .child(document ? div().id(`document-${document.dirty ? "dirty":"clean"}`).flex_1().min_h_0().child(DocumentSurface.new("fixture-document",{document:document.id})) : div().child("No document")));
  }
}
