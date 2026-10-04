// Menu snapshots keep the original file targets when selection changes.
import {writeClipboard, insertFileReferences} from 'sailry/ui';
import {openSystem} from 'sailry/file-transfers';

export class Actions {
  constructor(view) { this.view = view; this.entries = new Map(); this.ids = new Map(); this.treeMenus = new Map(); }

  item(action,key,target,enabled = true,icon) {
    const value = {action,target}, signature = JSON.stringify(value);
    let id = this.ids.get(signature);
    if (!id) { id = `file-action-${this.entries.size}`; this.ids.set(signature,id); this.entries.set(id,value); }
    return {id,label:this.view.text[key],enabled,...(icon ? {icon} : {})};
  }

  separator() { return {id:'separator',label:'',enabled:false,separator:true}; }

  create(directory) {
    const target = {directory,paths:[]}, enabled = this.view.explorer.connected;
    const items = [this.item('new_file','files_new_file',target,enabled,'file'),
      this.item('new_directory','files_new_directory',target,enabled,'folder'),
      this.item('upload','files_upload',target,enabled,'arrow-up'),this.separator(),
      this.item('paste','files_paste',target,enabled && this.view.transfers.canPaste())];
    const operations = this.view.documents.state.operations ?? [];
    const transfers = this.view.transfers.state.transfers.filter(value => !['done','cancelled'].includes(value.stage));
    if (operations.length || transfers.length) items.push(this.separator());
    for (const operation of operations) items.push({...this.item('recover','files_check_result',{operation},!operation.running),
      label:`${this.view.text.files_check_result} · ${operation.action.path ?? operation.action.to}`});
    for (const transfer of transfers) items.push({...this.item('show_transfer','files_check_result',{transfer:transfer.id}),
      label:`${this.view.text.files_check_result} · ${transfer.path}`});
    return items;
  }

  prepareTree() {
    const paths = this.view.explorer.paths(), selected = new Set(paths), menus = new Map();
    const prepared = new Map();
    // Prepare captured targets in event work, not the Kit frame's 50 ms render scope.
    for (const path of this.view.explorer.visible()) {
      const key = selected.has(path) ? `selection:${this.view.explorer.directory(path)}` : `path:${path}`;
      if (!menus.has(key)) menus.set(key,this.tree(path,paths));
      prepared.set(path,menus.get(key));
    }
    this.treeMenus = prepared;
  }

  treeMenu() { return path => this.treeMenus.get(path) ?? []; }

  tree(path, selection) {
    const {explorer,transfers} = this.view;
    const paths = explorer.targets(path,selection), directory = explorer.directory(path);
    const target = {paths,directory}, enabled = explorer.connected;
    const items = [];
    if (paths.length) items.push(this.item('open_system','files_open_system',target,enabled),this.separator(),
      this.item('copy','files_copy',target,enabled),this.item('cut','files_cut',target,enabled));
    items.push(this.item('paste','files_paste',target,enabled && transfers.canPaste()));
    if (paths.length) {
      items.push(this.item('copy_path','files_copy_path',target),
        this.item('insert','files_insert_conversation',{...target,
          files:paths.map(path => ({path,directory:explorer.entry(path)?.kind === 'directory'}))},this.view.canInsert),
        this.separator());
      if (paths.length === 1) {
        items.push(this.item('rename','files_rename',target,enabled));
        if (explorer.entry(paths[0])?.kind === 'file') items.push(this.item('download','files_download',target,enabled));
      }
      items.push(this.item('trash','files_trash',target,enabled),this.separator());
    }
    return [...items,this.item('new_file','files_new_file',target,enabled),
      this.item('new_directory','files_new_directory',target,enabled)];
  }

  path(document) {
    const target = {document:document.id,paths:[document.path]};
    return [this.item('open_system','files_open_system',target,this.view.explorer.connected),
      this.item('copy_path','files_copy_path',target)];
  }

  async invoke(id,cx) {
    const entry = this.entries.get(id);
    if (!entry) return;
    const {action,target} = entry, {view} = this;
    if (target.document && !view.documents.items().some(document => document.id === target.document)) return;
    switch (action) {
      case 'recover': return view.recover(target.operation,cx);
      case 'show_transfer': this.view.transfers.current = target.transfer; this.view.transfers.refresh(cx); return;
      case 'new_file': return view.edit('create',[target.directory],false,cx);
      case 'new_directory': return view.edit('create',[target.directory],true,cx);
      case 'rename': return view.edit('rename',target.paths,false,cx);
      case 'trash': return view.edit('trash',target.paths,false,cx);
      case 'copy': case 'cut': return view.transfers.capture(target.paths,action === 'cut',cx);
      case 'paste': return view.transfers.paste(target.directory,cx);
      case 'upload': return view.transfers.upload(target.directory,cx);
      case 'download': return view.transfers.download(target.paths[0],cx);
      case 'open_system': for (const path of target.paths) await openSystem(path); return;
      case 'copy_path': writeClipboard(target.paths.join('\n')); return;
      case 'insert': await insertFileReferences(target.files); return;
    }
  }

  command(action,cx) {
    const {explorer} = this.view;
    const paths = explorer.targets(explorer.current ?? '');
    const directory = explorer.directory();
    if (action === 'select_all') { explorer.all(); cx.notify(); return; }
    if (action !== 'paste' && !paths.length) return;
    if (action === 'rename' && paths.length !== 1) return;
    const name = action === 'delete' ? 'trash' : action;
    const item = this.item(name,`files_${name}`,{paths,directory});
    return this.invoke(item.id,cx);
  }
}
