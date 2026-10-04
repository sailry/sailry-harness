// Curated memory management retains the original captured Node and revision semantics.
import {View} from 'gpui-kit';
import {context} from 'sailry';
import {newId as newMemoryId,readProjectCatalog,
  completeRequest,forgetRequest,nextChange} from 'sailry/sdk';
import {createText,readText,setText,releaseText,nextTextEvent} from 'sailry/forms';
import {modal_closed,toast,nextControlEvent} from 'sailry/ui';
import {messages} from './locales.js';
import {render} from './view.js';
import {bytes} from '../host/policy.js';
import {readMemorySettings,prepareMemorySettings,settingsOutput} from '../host/settings.js';
import {listMemories,browseMemories,reviewMemories} from '../host/retrieval.js';
import {readMemory} from '../host/storage.js';
import {prepareMemory,prepareRemoveMemory,prepareMergeMemories,memoryOutput} from '../host/mutations.js';

const clone = value => JSON.parse(JSON.stringify(value));
const same = (left,right) => JSON.stringify(left) === JSON.stringify(right);
const uncertain = code => ['outcome_unknown','unavailable'].includes(code);
export const errorKey = code => code === 'revision_conflict' || code === 'not_found' ? 'memory_conflict'
  : uncertain(code) ? 'memory_unknown' : code === 'invalid_request' ? 'memory_invalid'
  : code === 'busy' ? 'memory_capacity' : code === 'conflict' ? 'memory_duplicate' : 'memory_failed';

export default class Settings extends View {
  init(_props,cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.saved = null; this.draft = null; this.configuration = {pending:false,request:null,error:null};
    this.connected = true; this.loading = false; this.refreshQueued = false; this.generation = 0; this.error = null;
    this.entries = []; this.results = []; this.reviews = []; this.projects = [];
    this.scope = 'all'; this.view = 'active'; this.query = createText('',{placeholder:this.text.memory_search,label:this.text.memory_search});
    this.editing = null; this.removal = null; this.dialogId = 0;
    this.refresh(cx);
    cx.spawn(async cx => {
      let cursor;
      try { while (true) {
        const event = await nextChange(cursor), first = cursor === undefined; cursor = event.cursor;
        this.connected = event.connected;
        if (event.connected && !first) this.refresh(cx);
        cx.notify();
      } } catch { /* Closing settings releases its captured subscription. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextTextEvent();
        if (event.id === this.query && event.kind === 'change') this.refresh(cx);
      } } catch { /* Closing settings releases form events. */ }
    });
    cx.spawn(async cx => {
      try { while (true) { if (await modal_closed() === `memory-dialog-${this.dialogId}`) this.close(cx); } }
      catch { /* Closing settings releases modal events. */ }
    });
    cx.spawn(async cx => {
      try { while (true) this.control(await nextControlEvent(),cx); }
      catch { /* Closing settings releases native control events. */ }
    });
  }
  control(event,cx) {
    if(event.id==='memory-budget' && [2,4,8,16,32,64].includes(Number(event.value))) this.configure('context_bytes',Number(event.value)*1024,cx);
    else if(event.id==='memory-review-age' && [30,90,180,365].includes(Number(event.value))) this.configure('review_after_days',Number(event.value),cx);
    else if(event.id==='memory-scope' && ['all','global',...this.projects.map(project=>project.id)].includes(event.value)) {
      this.scope=event.value;this.refresh(cx);
    } else if(event.id===`memory-kind-${this.dialogId}` && this.editing && !this.editing.loading && !this.editing.pending && !this.editing.request && ['user','feedback','project','reference'].includes(event.value)) {
      this.editing.entry.summary.kind=event.value;cx.notify();
    }
  }
  project() { return this.scope === 'all' || this.scope === 'global' ? null : this.scope; }
  projectLabel(id) { return id === null ? this.text.memory_global : this.projects.find(project => project.id === id)?.name ?? this.text.memory_project_unavailable; }
  kindLabel(kind) { return this.text[`memory_${kind}`]; }
  busy() { return !this.connected || this.loading; }
  fail(owner,key) { owner.error=key;toast({id:'memory-error',message:this.text[key],kind:'error'}); }
  refresh(cx) {
    if (!this.connected) return;
    if (this.loading) { this.refreshQueued = true; return; }
    const generation = ++this.generation, filter = {project:this.project(),all_projects:this.scope === 'all',archived:this.view === 'archived',query:readText(this.query)};
    const review = this.view === 'review';
    this.loading = true; this.error = null; cx.notify();
    cx.spawn(async cx => {
      try {
        const [settings,entries,catalog,results] = await Promise.all([readMemorySettings(),listMemories(),readProjectCatalog(),browseMemories(filter)]);
        const reviews = review ? await reviewMemories(settings,filter) : [];
        if (generation !== this.generation) return;
        if (!this.draft || (same(this.draft,this.saved) && !this.configuration.pending && !this.configuration.request)) this.draft = clone(settings);
        this.saved = settings; this.entries = entries; this.projects = catalog.projects; this.reviews = reviews;
        this.results = review ? results.filter(entry => reviews.some(candidate => candidate.summary.id === entry.id)) : results;
      } catch (error) {
        if (generation === this.generation) this.fail(this,error.code === 'invalid_request' ? 'memory_search_too_long' : 'memory_failed');
      } finally {
        if (generation === this.generation) {
          this.loading = false;
          const refresh = this.refreshQueued; this.refreshQueued = false;
          if (refresh) this.refresh(cx);
          cx.notify();
        }
      }
    });
  }
  configure(key,value,cx) {
    if (!this.draft || this.configuration.pending || this.configuration.request || this.configuration.error || !this.connected) return;
    this.draft[key] = value;
    this.saveConfiguration(cx);
  }
  saveConfiguration(cx) {
    const owner = this.configuration;
    if (owner.pending || !this.connected) return;
    owner.pending = true; owner.error = null; cx.notify();
    cx.spawn(async cx => {
      try {
        owner.request ??= await prepareMemorySettings(clone(this.draft));
        const result = await completeRequest(owner.request);
        if (result.Err) {
          owner.error = result.Err.code === 'revision_conflict' ? 'memory_settings_conflict' : errorKey(result.Err.code);
          if (!uncertain(result.Err.code)) { forgetRequest(owner.request); owner.request = null; }
        } else if (result.Ok?.kind === 'plugin_transaction') {
          this.saved = settingsOutput(result.Ok); this.draft = clone(this.saved); forgetRequest(owner.request); owner.request = null;
        } else owner.error = 'memory_unknown';
      } catch { owner.error = owner.request ? 'memory_unknown' : 'memory_failed'; }
      finally { owner.pending = false;if(owner.error)this.fail(owner,owner.error);cx.notify(); }
    });
  }
  reloadConfiguration(cx) { this.draft = clone(this.saved); this.configuration.error = null; cx.notify(); }
  edit(summary,cx) {
    if (this.busy() || this.editing?.pending || this.removal?.pending) return;
    this.close(cx);
    const project = summary ? summary.project : this.project();
    const entry = {summary:summary ? clone(summary) : {id:newMemoryId(),project,title:'',kind:'user',revision:0,updated_at_ms:0,archived:false},body:''};
    const owner = {entry,title:createText(entry.summary.title,{label:this.text.memory_title,placeholder:this.text.memory_title_hint}),body:createText('',{label:this.text.memory_body,placeholder:this.text.memory_body_hint,multiline:true}),sources:[],loading:!!summary,pending:false,request:null,error:null};
    this.editing = owner; this.dialogId++; cx.notify();
    if (summary) this.loadEditor(owner,cx);
  }
  loadEditor(owner,cx) {
    owner.loading = true; owner.error = null;
    cx.spawn(async cx => {
      try {
        const entry = await readMemory(owner.entry.summary.id);
        if (this.editing !== owner) return;
        owner.entry = entry; setText(owner.title,entry.summary.title); setText(owner.body,entry.body); owner.loading = false;
      } catch { if (this.editing === owner) this.fail(owner,'memory_failed'); }
      finally { cx.notify(); }
    });
  }
  merge(id,cx) {
    const owner = this.editing;
    if (!owner || owner.loading || owner.pending || owner.request || owner.sources.length) return;
    owner.loading = true;
    cx.spawn(async cx => {
      try {
        const source = await readMemory(id);
        if (this.editing !== owner) return;
        if (source.summary.project !== owner.entry.summary.project || source.summary.archived) throw new Error('invalid source');
        owner.sources = [{id,revision:source.summary.revision}];
        const body = readText(owner.body); setText(owner.body,`${body}${body ? '\n\n' : ''}${source.body}`);
      } catch { if (this.editing === owner) this.fail(owner,'memory_failed'); }
      finally { if (this.editing === owner) owner.loading = false; cx.notify(); }
    });
  }
  save(cx) {
    const owner = this.editing;
    if (!owner || owner.loading || owner.pending || !this.connected) return;
    if (!owner.request) {
      const entry = clone(owner.entry); entry.summary.title = readText(owner.title).trim(); entry.body = readText(owner.body);
      if (!entry.summary.title || bytes(entry.summary.title) > 160 || /[\u0000-\u001f\u007f]/.test(entry.summary.title) || !entry.body.trim() || bytes(entry.body) > 8192 || entry.body.includes('\0')) { this.fail(owner,'memory_invalid'); cx.notify(); return; }
      const sources = clone(owner.sources);
      this.perform(owner,cx,()=>sources.length ? prepareMergeMemories(entry,sources) : prepareMemory(entry,entry.summary.revision));
      return;
    }
    this.perform(owner,cx);
  }
  remove(summary,cx) {
    if (this.busy() || this.editing?.pending || this.removal?.pending) return;
    this.close(cx); this.removal = {summary:clone(summary),request:null,pending:false,error:null}; this.dialogId++; cx.notify();
  }
  confirmRemoval(cx) {
    const owner = this.removal;
    if (!owner || owner.pending || !this.connected) return;
    this.perform(owner,cx,()=>prepareRemoveMemory(owner.summary.id,owner.summary.revision));
  }
  perform(owner,cx,prepare) {
    owner.pending = true; owner.error = null; cx.notify();
    cx.spawn(async cx => {
      try {
        if (!owner.request) owner.request = await prepare();
        const result = await completeRequest(owner.request);
        if (result.Err) {
          owner.error = errorKey(result.Err.code);
          if (!uncertain(result.Err.code)) { forgetRequest(owner.request); owner.request = null; }
        } else if (['memory_results','memory_removed'].includes(memoryOutput(result.Ok).kind)) {
          forgetRequest(owner.request); owner.request = null; owner.pending = false;
          if (this.editing === owner || this.removal === owner) this.close(cx);
          this.refresh(cx);
        } else owner.error = 'memory_unknown';
      } catch (error) { owner.error = owner.request ? 'memory_unknown' : errorKey(error.code ?? 'internal'); }
      finally { owner.pending = false;if(owner.error)this.fail(owner,owner.error);cx.notify(); }
    });
  }
  close(cx) {
    for (const owner of [this.editing,this.removal]) if (owner?.request) forgetRequest(owner.request);
    if (this.editing) { releaseText(this.editing.title); releaseText(this.editing.body); }
    this.editing = null; this.removal = null; cx.notify();
  }
  render() { return render(this); }
}
