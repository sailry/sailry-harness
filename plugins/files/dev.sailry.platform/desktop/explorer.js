// Directory paging and multi-selection follow Sailry 116aab0f's file browser.
import {listDirectory, faultCode} from 'sailry/sdk';

export const parent = path => path.includes('/') ? path.slice(0,path.lastIndexOf('/')) : '';
export const join = (directory,name) => directory ? `${directory}/${name}` : name;
export const within = (path,directory) => path === directory || path.startsWith(`${directory}/`);

export class Explorer {
  constructor(report) {
    this.report = report;
    this.pages = new Map();
    this.expanded = new Set();
    this.selected = new Set();
    this.current = null;
    this.anchor = null;
    this.connected = true;
  }

  entry(path) {
    const directory = this.pages.get(parent(path));
    return directory?.index?.get(path.slice(path.lastIndexOf('/') + 1));
  }

  visible() {
    const paths = [];
    const walk = directory => {
      for (const entry of this.pages.get(directory)?.entries ?? []) {
        const path = join(directory,entry.name);
        paths.push(path);
        if (entry.kind === 'directory' && this.expanded.has(path)) walk(path);
      }
    };
    walk('');
    return paths;
  }

  paths() { const visible = new Set(this.visible()); return [...this.selected].filter(path => visible.has(path)).sort(); }

  targets(path, selected) {
    if (path === '') return [];
    if (!this.selected.has(path)) return [path];
    const paths = selected ?? this.paths();
    if (!paths.includes(path)) return [path];
    const present = new Set(paths);
    return paths.filter(path => {
      for (let directory = parent(path); directory; directory = parent(directory)) {
        if (present.has(directory)) return false;
      }
      return true;
    });
  }

  directory(path = this.current) { return !path ? '' : this.entry(path)?.kind === 'directory' ? path : parent(path); }

  select(path, {shift = false, additive = false} = {}) {
    const visible = this.visible(), index = visible.indexOf(path);
    if (index < 0) return;
    if (shift) {
      const anchor = Math.max(0,visible.indexOf(this.anchor ?? path));
      if (!additive) this.selected.clear();
      for (const item of visible.slice(Math.min(anchor,index),Math.max(anchor,index) + 1)) this.selected.add(item);
    } else if (additive) {
      if (!this.selected.delete(path)) this.selected.add(path);
      this.anchor = path;
    } else {
      this.selected = new Set([path]);
      this.anchor = path;
    }
    this.current = path;
  }

  all() { this.selected = new Set(this.visible()); this.anchor = this.paths()[0] ?? null; }

  context(path) {
    if (!this.paths().includes(path)) this.selected = new Set(path ? [path] : []);
    this.current = path || null; this.anchor = path;
  }

  async expand(path, cx) {
    this.expanded.add(path);
    cx.notify();
    if (!this.pages.get(path)?.loaded) await this.load(path,false,cx);
  }

  collapse(path, cx) { this.expanded.delete(path); cx.notify(); }

  async load(path, more, cx) {
    if (!this.connected) return;
    let page = this.pages.get(path);
    if (!page) {
      page = {entries:[],next:null,loaded:false,loading:false,refresh:false,partial:false};
      this.pages.set(path,page);
    }
    if (page.loading) { page.refresh ||= !more; return; }
    if (more && !page.next) return;
    page.loading = true; page.error = null; cx.notify();
    try {
      const extent = more ? 0 : page.entries.length;
      let result = await listDirectory(path,more ? page.next : undefined);
      const entries = more ? [...page.entries,...result.entries] : [...result.entries];
      let partial = result.unsupported_names > 0 || (result.truncated && !result.next);
      // Refresh the visible extent with fresh cursors; keep selection until all pages arrive.
      while (!more && result.next && entries.length < extent) {
        result = await listDirectory(path,result.next);
        entries.push(...result.entries);
        partial ||= result.unsupported_names > 0 || (result.truncated && !result.next);
      }
      const names = new Set();
      page.entries = entries.filter(entry => {
        if (names.has(entry.name)) return false;
        names.add(entry.name); return true;
      });
      page.index = new Map(page.entries.map(entry => [entry.name,entry]));
      page.next = result.next; page.loaded = true;
      page.partial = partial;
      page.revision = result.revision;
      const visible = new Set(this.visible());
      this.selected = new Set([...this.selected].filter(path => visible.has(path)));
      if (this.current && !visible.has(this.current)) this.current = null;
    } catch (error) {
      const code = faultCode(error.message ?? String(error));
      page.error = code === 'revision_conflict' ? 'files_directory_changed' : 'files_directory_failed';
      this.report(page.error);
      if (more && code === 'revision_conflict') page.refresh = true;
    } finally {
      page.loading = false;
      const refresh = page.refresh; page.refresh = false;
      if (refresh) await this.load(path,false,cx);
      cx.notify();
    }
  }

  async refresh(cx) {
    await this.load('',false,cx);
    await Promise.all([...this.expanded].filter(path => this.entry(path)?.kind === 'directory')
      .map(path => this.load(path,false,cx)));
  }

  items(text, menu) {
    const children = directory => {
      const page = this.pages.get(directory);
      const rows = (page?.entries ?? []).map(entry => {
        const path = join(directory,entry.name), folder = entry.kind === 'directory';
        return {id:path,label:entry.name,icon:folder ? 'folder' : 'file',
          expanded:folder && this.expanded.has(path),menu:menu(path),
          ...(folder ? {children:children(path)} : {})};
      });
      if (!rows.length && directory !== '') rows.push({id:`${directory}\0empty`,
        label:text[page?.loaded ? 'files_directory_empty' : 'files_loading'],disabled:true});
      if (page?.next) rows.push({id:`${directory}\0more`,icon:'ellipsis',
        label:text[page.loading ? 'files_loading' : 'files_load_more'],disabled:page.loading});
      return rows;
    };
    return children('');
  }
}
