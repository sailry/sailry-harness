import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const root = new URL('../',import.meta.url);
const stateSource = (await readFile(new URL('dev.sailry.platform/desktop/state.js',root),'utf8')).replace(/^export /gm,'');
const localeSource = (await readFile(new URL('dev.sailry.platform/desktop/locales.js',root),'utf8')).replace(/^export /gm,'');
const {State,sections,sessionStatus,messages} = vm.runInNewContext(`${localeSource}\n${stateSource}\n({State,sections,sessionStatus,messages})`);
const text = messages('en');
const plain = value => JSON.parse(JSON.stringify(value));
const session = (id,lane = 'running',project = 'project') => ({id,project,worktree:'tree',archived:false,delegation:null,
  activity:{title:id,queued:0,waiting:null,run:{turn:`turn-${id}`,status:lane === 'waiting' ? 'running' : lane}}});
const catalog = sessions => ({node:'captured',cursor:'1',projects:[{id:'project',name:'Project',appearance:{icon:'code',color:'blue'}}],
  worktrees:[{id:'tree',project:'project',path:'/checkout'}],sessions,terminals:[],unread_terminals:[],hosts:[],
  session_lanes:Object.fromEntries(sessions.map(value => [value.id,value.id])),terminal_lanes:{}});

test('the workbench groups canonical lanes and omits archived and delegated sessions', () => {
  const data = catalog(['waiting','running','failed','idle'].map(lane => session(lane,lane)));
  data.sessions.push({...session('archived'),archived:true},{...session('child'),delegation:{session:'parent'}});
  data.session_lanes.archived = 'running';data.session_lanes.child = 'running';
  const result = sections(data,new Map(),text);
  assert.deepEqual(plain(Object.fromEntries(Object.entries(result).map(([lane,groups]) => [lane,groups.flatMap(group => group.cards.map(card => card.id))]))),
    {waiting:['waiting'],running:['running'],completed:['failed'],idle:['idle']});
  assert.equal(result.completed[0].cards[0].lane,'failed');
  assert.equal(result.running[0].appearance.icon,'code');
});

test('completed terminal grouping uses the shared controller unread state and preserves actual failures', () => {
  const data = catalog([]);
  data.terminals = [{id:'completed',worktree:'tree',title:null},{id:'failed',worktree:'tree',title:'CLI'},
    {id:'ssh',worktree:null,title:'SSH'},{id:'closed',worktree:'tree',title:'Closed'}];
  data.terminal_lanes = {completed:'completed',failed:'failed',ssh:'running'};
  data.unread_terminals = ['failed'];
  const result = sections(data,new Map(),text);
  assert.equal(result.completed[0].cards[0].id,'failed');
  assert.equal(result.completed[0].cards[0].content,text.activity_failed);
  assert.equal(result.idle[0].cards[0].id,'completed');
  assert.equal(result.idle[0].cards[0].title,text.terminal);
  assert.equal(result.running.length,0);
});

test('preview text must match the current turn and failed or waiting cards retain their status', () => {
  const waiting = session('waiting','waiting'), failed = session('failed','failed'), stale = session('running');
  waiting.activity.waiting = 'approval';
  const result = sections(catalog([waiting,failed,stale]),new Map([
    ['waiting',{turn:'turn-waiting',text:'Approve this command'}],['failed',{turn:'turn-failed',text:'HTTP 402'}],
    ['running',{turn:'previous',text:'Old content'}],
  ]),text);
  assert.equal(result.waiting[0].cards[0].content,`${text.activity_approval} · Approve this command`);
  assert.equal(result.completed[0].cards[0].content,`${text.activity_failed} · HTTP 402`);
  assert.equal(result.running[0].cards[0].content,text.activity_running);
});

test('project groups remain ordered and retain standalone identities and all overflow cards', () => {
  const data = catalog(Array.from({length:8},(_,index) => session(`card-${index}`)));
  data.projects.push({id:'another',name:'Another project with a long display name',appearance:{icon:'folder',color:'none'}});
  data.worktrees.push({id:'other-tree',project:'another',path:'C:\\checkout\\branch'},
    {id:'scratch',project:null,path:'/scratch'});
  data.sessions.push({...session('other'),project:'another',worktree:'other-tree'},
    {...session('standalone'),project:null,worktree:'scratch'},
    {...session('removed-tree'),worktree:'missing'});
  data.session_lanes = Object.fromEntries(data.sessions.map(value => [value.id,'running']));
  const running = sections(data,new Map(),text).running;
  assert.deepEqual(plain(running.map(group => group.id)),[null,'another','project']);
  assert.equal(running[0].name,text.activity_no_project);assert.equal(running[0].cards[0].path,'');
  assert.equal(running[1].cards[0].path,'C:\\checkout\\branch');
  assert.deepEqual(plain(running[2].cards.map(card => card.id)),Array.from({length:8},(_,index) => `card-${7-index}`));
  assert.ok(running.every(group => group.cards.every(card => card.id !== 'removed-tree')));
});

test('status text retains queue, approval, input and interruption distinctions', () => {
  const card = session('state');
  const cases = {queued:'turn_queued',stopping:'chat_stopping',completed:'activity_completed',
    failed:'activity_failed',interrupted:'chat_interrupted',cancelled:'turn_cancelled'};
  for (const [status,key] of Object.entries(cases)) {
    card.activity.run.status = status;assert.equal(sessionStatus(card,text),text[key]);
  }
  card.activity.run.status = 'running';
  for (const [waiting,key] of [[null,'activity_running'],['approval','activity_approval'],['input','activity_input']]) {
    card.activity.waiting = waiting;assert.equal(sessionStatus(card,text),text[key]);
  }
  card.activity.queued = 2;
  assert.equal(sessionStatus(card,text),`${text.activity_input} · ${text.turn_queued} 2`);
  card.activity.run.status = 'completed';assert.equal(sessionStatus(card,text),`${text.turn_queued} 2`);
  card.activity.run = null;card.activity.queued = 0;assert.equal(sessionStatus(card,text),text.activity_idle);
});

test('card metadata uses execution timestamps and the conversation phase', () => {
  const card = session('running');
  const data = catalog([card]);
  data.session_lanes.running = 'running';
  const item = () => sections(data,new Map(),text).running[0].cards[0];
  assert.equal(item().time,null);assert.equal(item().phase,'turn_tools_running');
  card.activity.run.started_ms = 1234;
  assert.equal(item().time,1234);
  card.activity.waiting = 'approval';
  assert.equal(item().phase,'turn_waiting');
  card.activity.run.status = 'stopping';
  assert.equal(item().phase,'turn_waiting');
  card.activity.run.finished_ms = 5678;
  card.activity.run.status = 'completed';
  assert.equal(item().time,5678);assert.equal(item().phase,null);
});

test('a refreshed catalog preserves literal preview content', async () => {
  const card = session('running');card.activity.title = '  Review\nworkspace\rchanges  ';
  const content = 'Reviewing\n\n**Result**\n| File | State |\n| --- | --- |\n| example.rs | changed |\n继续检查项目';
  let cursor = 1;
  const state = new State(async () => ({...catalog([card]),cursor:String(cursor)}),
    async () => [{session:card.id,turn:card.activity.run.turn,text:content}],() => {});
  await state.refresh();await state.previewTask;
  cursor += 1;await state.refresh();await state.previewTask;
  const item = sections(state.catalog,state.previews,text).running[0].cards[0];
  assert.equal(item.title,'Review workspace changes');assert.equal(item.content,content);
});

test('failed refresh reports once and retains the current board',async()=>{
  let fail=false;const notices=[];
  const state=new State(async()=>{if(fail)throw new Error('Catalog unavailable');return catalog([session('running')]);},async()=>[],()=>{},()=>notices.push(text.activity_read_failed));
  await state.refresh();const previous=state.catalog;
  fail=true;await state.refresh();assert.equal(notices.length,1);assert.equal(state.error,true);
  assert.equal(state.catalog,previous);
  fail=false;await state.refresh();assert.equal(state.error,false);assert.equal(notices.length,1);
});

test('preview reads use bounded batches and stop after the active catalog changes', async () => {
  const cards = Array.from({length:18},(_,index) => session(`card-${index}`));
  cards.push({...session('archived'),archived:true},{...session('child'),delegation:{session:'parent'}});
  const reads = [];
  const state = new State(async () => catalog(cards),async ids => {reads.push(plain(ids));return [];},() => {});
  await state.refresh();await state.previewTask;
  assert.deepEqual(reads.map(batch => batch.length),[8,8,2]);
  assert.ok(reads.flat().every(id => !['archived','child'].includes(id)));
  let finish;
  state.readPreviews = ids => {reads.push(plain(ids));return new Promise(resolve => {finish = resolve;});};
  await state.refresh();const pending = state.previewTask;state.generation += 1;
  finish([{session:cards[0].id,turn:cards[0].activity.run.turn,text:'Late'}]);await pending;
  assert.equal(state.previews.has(cards[0].id),false);assert.equal(reads.length,4);
});

test('catalog updates are not blocked by optional previews and discard late preview generations', async () => {
  let cursor = 1, finishPreview;
  const state = new State(async () => ({...catalog([session('running')]),cursor:String(cursor)}),
    () => new Promise(resolve => {finishPreview = resolve;}),() => {});
  await state.refresh();
  const oldPreview = finishPreview;
  cursor = 2;await state.refresh();
  oldPreview([{session:'running',turn:'turn-running',text:'Late'}]);
  await Promise.resolve();
  assert.equal(state.catalog.cursor,'2');assert.equal(state.previews.has('running'),false);
  finishPreview([{session:'running',turn:'turn-running',text:'Current'}]);
  await state.previewTask;
  assert.equal(state.previews.get('running').text,'Current');
});

test('hiding invalidates previews and showing the cached page resumes them', async () => {
  let finish;
  const state = new State(async () => catalog([session('running')]),
    () => new Promise(resolve => {finish = resolve;}),() => {});
  await state.refresh();state.visible = true;
  const pending = state.previewTask, old = finish;
  state.hosts({hosts:[],unread_terminals:[],visible:false});
  old([{session:'running',turn:'turn-running',text:'Hidden'}]);await pending;
  assert.equal(state.previews.has('running'),false);
  state.hosts({hosts:[],unread_terminals:[],visible:true});await state.pending;
  finish([{session:'running',turn:'turn-running',text:'Visible'}]);await state.previewTask;
  assert.equal(state.previews.get('running').text,'Visible');
});

test('the shipped page has ordinary navigation and lists every imported package file', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json',root),'utf8'));
  const desktop = manifest.extensions['dev.sailry.platform'].desktop;
  assert.equal(desktop.navigation_options.target,'node');assert.equal(desktop.navigation_options.order,100);
  assert.equal(desktop.navigation_options.pinned,true);
  for (const name of desktop.resources) {
    const source = await readFile(new URL(name,root),'utf8');
    for (const [,path] of source.matchAll(/from ['"]\.\/([^'"]+)['"]/g)) {
      assert.ok(desktop.resources.includes(`dev.sailry.platform/desktop/${path}`),`${name} imports unlisted ${path}`);
    }
  }
});
