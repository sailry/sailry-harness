import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {State,query,choices} from '../dev.sailry.platform/desktop/state.js';
import {messages} from '../dev.sailry.platform/desktop/locales.js';
import {compact,ranking} from '../dev.sailry.platform/desktop/format.js';
import {requestRows} from '../dev.sailry.platform/desktop/requests.js';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/settings.js', import.meta.url), 'utf8'))
  .replace(/^import .*;\n/gm, '').replace('export default class Usage', 'class Usage');
const plain = value => JSON.parse(JSON.stringify(value));
const node = '01'.repeat(32), remote = '02'.repeat(32);

function fixture() {
  const tasks = [], events = [], calls = [], notices=[],cx = {spawn:task => tasks.push(task), notify() {}};
  const {Usage} = vm.runInNewContext(`${source}\n({Usage})`, {
    View:class {}, context:() => JSON.stringify({node,locale:'en'}), State, query, choices, messages,
    nextTableEvent:async () => { throw new Error('closed'); },
    header_action:async () => { if (!events.length) throw new Error('closed'); return events.shift(); },
    selectHost:() => assert.fail('Usage filters must not navigate'),
    watchUsage:(query,all,target=node) => calls.push(['watch',plain(query),all,target]),
    nextUsageChange:async () => { throw new Error('closed'); },
    refreshUsage:() => calls.push(['refresh']),
    toast:value=>notices.push(value),
  });
  const view = new Usage();
  view.init({},cx);
  const header = tasks.at(-1);
  return {view,cx,calls,tasks,events,notices,header:() => header(cx),
    select:(field,value,generation=view.state.generation) => events.push(JSON.stringify({id:`usage-filter-${field}`,value:`${generation}:${value}`}))};
}

test('declares one independent Host-owned workspace page', async () => {
  const manifest = JSON.parse(await readFile(new URL('../plugin.json', import.meta.url), 'utf8'));
  const extension = manifest.extensions['dev.sailry.platform'];
  assert.equal(extension.settings_page, undefined);
  assert.equal(extension.scope ?? 'host', 'host');
  assert.equal(extension.desktop.navigation_options.surface, 'workspace');
  assert.equal(extension.desktop.navigation_options.target ?? 'node', 'node');
  assert.deepEqual(extension.actions, ['usage.read','conversation.read']);
  assert.equal(extension.desktop.ui_shared, true);
  assert.equal(extension.api_version, 'v1');
});

test('uses the bound Shell host and rejects page-level host changes', async () => {
  const setup = fixture(), header = setup.view.header();
  assert.equal(header.title, 'Usage');
  assert.deepEqual(plain(header.filters.map(filter => filter.id)), ['usage-filter-range','usage-filter-project','usage-filter-provider','usage-filter-model']);
  setup.select('host',remote);
  setup.select('host','all');
  setup.select('provider','selected');
  setup.select('provider','stale',1);
  setup.select('project','project-a',2);
  await setup.header();
  assert.deepEqual(setup.calls.filter(call => call[0] === 'watch').map(call => call.slice(2)), [[false,node],[false,node],[false,node]]);
  assert.equal(setup.view.state.node, node);
  assert.deepEqual(setup.view.state.query.providers, []);
  assert.deepEqual(setup.view.state.query.projects, ['project-a']);
});

test('failed observers retain honest feedback and retry the current query', async () => {
  const setup = fixture();
  await setup.tasks[0](setup.cx);
  assert.equal(setup.view.error, 'usage_read_failed');
  assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].kind,'error');
  setup.events.push('usage-refresh');
  await setup.header();
  assert.equal(setup.view.error, null);
  assert.equal(setup.view.state.generation, 2);
  assert.equal(setup.calls.filter(call => call[0] === 'watch').length, 2);
  assert.equal(setup.calls.some(call => call[0] === 'refresh'), false);
});

test('page title is published only through the shared Header', async () => {
  const viewSource = (await readFile(new URL('../dev.sailry.platform/desktop/view.js', import.meta.url), 'utf8'))
    .replace(/^import .*;\n/gm, '').replace(/^export /gm, '');
  const nodes = new Map();
  function element(kind='div',id=null,props={}) {
    const value = {kind,selector:id,props,items:[]};
    const proxy = new Proxy(value,{get(target,key) {
      if(key==='id')return id => {target.selector=id;nodes.set(id,proxy);return proxy;};
      if(key==='child')return child => {target.items.push(child);return proxy;};
      if(key==='children')return children => {target.items.push(...children);return proxy;};
      if(key in target)return target[key];
      return () => proxy;
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const {render} = vm.runInNewContext(`${viewSource}\n({render})`, {
    div:() => element(), Progress:function(id) {return element('Progress',id);},
    Header:{new:(id,props) => element('Header',id,props)},
    theme:() => ({colors:{muted_foreground:'muted'}}),
  });
  const {view} = fixture();
  render(view);
  assert.equal(nodes.get('usage-page').items[0].selector, 'usage-header');
  assert.equal(JSON.parse(nodes.get('usage-header').props.content).title, 'Usage');
  assert.equal(nodes.has('settings-header'), false);
  assert.equal(nodes.has('settings-heading'), false);
  assert.ok(nodes.has('usage-content'));
});

test('empty model rankings reuse their card and populated rankings retain native bars',async()=>{
  const viewSource=(await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
    .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
  const nodes=new Map();
  function element(kind='div',id=null,props={}) {
    const value={kind,selector:id,props,items:[]};
    const proxy=new Proxy(value,{get(target,key) {
      if(key==='id')return id=>{target.selector=id;nodes.set(id,proxy);return proxy;};
      if(key==='child')return child=>{target.items.push(child);return proxy;};
      if(key==='children')return children=>{target.items.push(...children);return proxy;};
      if(key in target)return target[key];
      return ()=>proxy;
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const adapters=Object.fromEntries(['EmptyState','SettingsGroup','ProgressBar']
    .map(kind=>[kind,{new:(id,props)=>element(kind,id,props)}]));
  const {card,ranks}=vm.runInNewContext(`${viewSource}\n({card,ranks})`,{
    div:()=>element(),theme:()=>({colors:{muted_foreground:'muted'}}),...adapters,compact,ranking,
  });
  const view={text:messages('en'),state:{all:false}},data={node,groups:[]};
  card('usage-ranking-card',ranks(view,data));
  const empty=nodes.get('usage-ranking-empty');
  assert.deepEqual(plain(empty.props),{variant:'list',icon:'chart-pie',label:view.text.usage_empty});
  const group=nodes.get('usage-ranking-card-box');
  assert.equal(group.kind,'SettingsGroup');
  assert.deepEqual(plain(group.props),{heading:false});
  assert.equal(group.items[0].items[0].selector,'usage-ranking');
  assert.equal(group.items[0].items[0].items[1],empty);
  data.groups=[{key:{kind:'model',data:{model:'retained-model',provider:'retained-provider'}},
    metrics:{tokens:{input:'2',output:'3'}}}];
  nodes.clear();card('usage-ranking-card',ranks(view,data));
  assert.equal(nodes.has('usage-ranking-empty'),false);
  assert.ok(nodes.has('usage-rank-0'));
  assert.equal(nodes.get('usage-rank-bar-0').props.value,100);
  assert.ok(nodes.get('usage-rank-bar-0').props.label.includes('retained-model'));
});

test('request content starts with the native six-column two-line table',async()=>{
  const viewSource=(await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
    .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
  const nodes=new Map();
  function element(kind='div',id=null,props={}) {
    const value={kind,selector:id,props,items:[]};
    const proxy=new Proxy(value,{get(target,key) {
      if(key==='id')return id=>{target.selector=id;nodes.set(id,proxy);return proxy;};
      if(key==='h')return height=>{target.height=height;return proxy;};
      if(key==='child')return child=>{target.items.push(child);return proxy;};
      if(key==='children')return children=>{target.items.push(...children);return proxy;};
      if(key in target)return target[key];
      return ()=>proxy;
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const {requests}=vm.runInNewContext(`${viewSource}\n({requests})`,{
    div:()=>element(),DataTable:{new:(id,props)=>element('DataTable',id,props)},requestRows,
  });
  const state=new State(node),request={position:{node,timestamp_ms:'1',sequence:'1'},model:'Model',provider_name:'Provider',scope_name:'Project',
    tokens:{input:'1200',output:'340',cached_input:'100',reasoning:'20'},first_token_us:'1250',elapsed_us:'1000000',usd_micros:'123'};
  const content=requests({state,text:messages('en')},{requests:{items:[request],has_more:false}}),table=nodes.get('usage-requests');
  assert.equal(content.items[0].selector,'usage-table');assert.equal(nodes.get('usage-table').height,96);
  assert.equal(table.kind,'DataTable');assert.equal(table.props.row_height,48);
  assert.equal(table.props.stripe,true);
  assert.deepEqual(plain(table.props.alignments),['start','start','end','end','end','end']);
  assert.deepEqual(plain(table.props.columns),['Model / Provider','Project / Time','Input / Output','Cached / Reasoning','First token / Duration','Cost']);
  assert.equal(table.props.cells[0][0].secondary.text,'Provider');assert.equal(table.props.cells[0][1].primary.text,'Project');
  assert.equal(table.props.cells[0][2].primary.tone,'chart_2');assert.equal(table.props.cells[0][2].secondary.tone,'chart_4');
  assert.equal(table.props.rows[0][5],'0.000123');
  requests({state,text:messages('en')},{requests:{items:Array.from({length:20},(_,index)=>({...request,position:{...request.position,sequence:String(index)}})),has_more:false}});
  assert.equal(nodes.get('usage-table').height,48*21);
  requests({state,text:messages('en')},{requests:{items:[],has_more:false}});
  assert.equal(nodes.get('usage-table').height,144);
});
