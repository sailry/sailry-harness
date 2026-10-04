import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {lanes,labels,sections,clock} from '../dev.sailry.platform/desktop/state.js';
import {messages} from '../dev.sailry.platform/desktop/locales.js';

const source=(await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const plain=value=>JSON.parse(JSON.stringify(value));

function fixture() {
  const nodes=new Map();
  function element(kind='div',id=null,props={}) {
    const value={kind,selector:id,props,items:[],contentSlot:null,style:{}};
    const proxy=new Proxy(value,{get(target,key) {
      if(key==='id')return id=>{target.selector=id;nodes.set(id,proxy);return proxy;};
      if(key==='child')return child=>{target.items.push(child);return proxy;};
      if(key==='content')return child=>{target.contentSlot=child;return proxy;};
      if(key==='children')return children=>{target.items.push(...children);return proxy;};
      if(key in target)return target[key];
      return (...args)=>{target.style[key]=args;return proxy;};
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const adapters=Object.fromEntries(['EmptyState','Appearance','CardButton','ClampedText','HoverSwap','ThinkingIcon','ScrollRegion','Collapse','CollapseSlot']
    .map(kind=>[kind,{new:(id,props)=>element(kind,id,props)}]));
  const {render}=vm.runInNewContext(`${source}\n({render})`,{
    div:()=>element(),Icon:function(id){return element('Icon',id);},
    ...adapters,lanes,labels,sections,clock,pageInsets:()=>({rail_width:56}),
    theme:()=>({colors:{muted_foreground:'muted',border:'border'}}),window:{rem_size:()=>16,viewport_size:()=>({width:1440})},
  });
  const view={text:messages('en'),board:{connected:true,error:false,previews:new Map(),
    catalog:{node:'captured',projects:[{id:'project',name:'Project'}],worktrees:[{id:'tree',project:'project',path:'/fixture'}],
      sessions:[],terminals:[],unread_terminals:[],session_lanes:{},terminal_lanes:{}}}};
  return {view,nodes,render(){nodes.clear();return render(view,{theme:()=>({radius:{lg:11}})});}};
}

test('empty lanes fill their viewport without cards',()=>{
  const setup=fixture();setup.render();
  for(const lane of lanes) {
    const key=labels[lane],items=setup.nodes.get(`${key}-items`),empty=setup.nodes.get(`${key}-empty`);
    assert.deepEqual(plain(empty.props),{variant:'list',fill_height:true,vertical_align:'start',icon:'inbox',label:setup.view.text.activity_board_empty});
    assert.equal(items.items[0],empty);
    assert.deepEqual(plain(items.style.overflow_y_scrollbar),[]);
    const heading=setup.nodes.get(`${key}-heading`);
    assert.deepEqual(plain(heading.style.rounded),[11]);
    assert.deepEqual(plain(heading.style.border_1),[]);
    assert.deepEqual(plain(heading.style.border_color),['border']);
    assert.equal(heading.style.rounded_full,undefined);
  }
  assert.equal([...setup.nodes.values()].filter(node=>node.kind==='EmptyState').length,4);
});

test('populated lanes retain their cards and host disclosures without empty content',()=>{
  const setup=fixture(),catalog=setup.view.board.catalog;
  catalog.sessions=[{id:'retained',project:'project',worktree:'tree',archived:false,delegation:null,
    activity:{title:'Retained task',queued:0,waiting:null,run:{turn:'turn',status:'running'}}}];
  catalog.session_lanes.retained='running';setup.render();
  const key=labels.running,group=`activity-project-${key}-project`;
  assert.equal(setup.nodes.has(`${key}-empty`),false);
  assert.ok(setup.nodes.has('activity-retained'));
  assert.deepEqual(plain(setup.view.targets.get('activity-retained')),{kind:'session',id:'retained'});
  assert.equal(setup.view.targets.has(group),false);
  assert.equal(setup.nodes.get(`captured-${group}`).kind,'Collapse');
  assert.equal([...setup.nodes.values()].filter(node=>node.kind==='EmptyState').length,3);
});

test('read failures keep all lanes without a retry bar',()=>{
  const setup=fixture();setup.view.board.error=true;setup.render();
  assert.equal(setup.nodes.has('activity-retry'),false);
  for(const lane of lanes)assert.ok(setup.nodes.has(`${labels[lane]}-items`));
});

function containing(root,child) {
  if(!root?.items)return;
  if(root.items.includes(child))return root;
  for(const nested of [...root.items,...(root.contentSlot ? [root.contentSlot] : [])]) {
    const parent=containing(nested,child);
    if(parent)return parent;
  }
}

test('project headings and content use host slots with a natural scrolling ceiling',()=>{
  const setup=fixture(),catalog=setup.view.board.catalog;
  catalog.sessions=Array.from({length:7},(_,index)=>({id:`retained-${index}`,project:'project',worktree:'tree',
    archived:false,delegation:null,activity:{title:'Retained task',queued:0,waiting:null,run:null}}));
  catalog.session_lanes=Object.fromEntries(catalog.sessions.map(session=>[session.id,'idle']));
  {
    const root=setup.render(),selector=`activity-project-${labels.idle}-project`;
    const header=setup.nodes.get(selector),group=containing(root,header),reveal=group.items[1];
    assert.equal(group.items[0],header);
    assert.equal(group.kind,'Collapse');
    assert.deepEqual(plain(group.props),{label:'Project',default_open:true,variant:'plain'});
    assert.equal(header.kind,'CollapseSlot');assert.equal(header.props.variant,'header');
    assert.equal(reveal.kind,'CollapseSlot');assert.equal(reveal.props.variant,'content');
    const viewport=setup.nodes.get(`${selector}-viewport`),area=viewport.items[0];
    assert.equal(containing(reveal,viewport).kind,'div');
    assert.equal(area.kind,'ScrollRegion');
    assert.deepEqual(plain(area.props),{max_height:520});
    assert.equal(area.selector,'captured-activity-project-activity_idle-project-cards');
    assert.equal(area.items.length,7);
    const card=setup.nodes.get('activity-retained-0'),message=setup.nodes.get('activity-retained-0-content');
    assert.equal(card.kind,'CardButton');
    assert.equal(card.props.fill_height,undefined);
    assert.equal(message.props.lines,2);
    assert.deepEqual(plain(setup.nodes.get('activity-retained-0-title').style.truncate),[]);
    assert.ok(setup.nodes.has('activity-retained-0-location'));
    assert.equal(setup.view.targets.has(selector),false);
    assert.deepEqual(plain(setup.view.targets.get('activity-retained-0')),{kind:'session',id:'retained-0'});
  }
});

test('native project scroll identity includes the captured node and lane',()=>{
  const setup=fixture(),catalog=setup.view.board.catalog;
  catalog.sessions=[{id:'retained',project:'project',worktree:'tree',archived:false,delegation:null,
    activity:{title:'Retained task',queued:0,waiting:null,run:null}}];
  catalog.session_lanes.retained='idle';setup.render();
  const key='captured-activity-project-activity_idle-project-cards';
  setup.render();assert.equal(setup.nodes.get(key).kind,'ScrollRegion');
  const sessions=catalog.sessions;
  catalog.sessions=[];setup.render();
  assert.equal(setup.nodes.has(key),false);
  catalog.sessions=sessions;setup.render();
  assert.equal(setup.nodes.get(key).kind,'ScrollRegion');
  catalog.session_lanes.retained='running';setup.render();
  assert.equal(setup.nodes.has(key),false);
  assert.equal(setup.nodes.get('captured-activity-project-activity_board_running-project-cards').kind,'ScrollRegion');
  catalog.session_lanes.retained='idle';
  catalog.node='replacement';setup.render();
  assert.equal(setup.nodes.has(key),false);
  assert.equal(setup.nodes.get('replacement-activity-project-activity_idle-project-cards').kind,'ScrollRegion');
  catalog.node='captured';setup.render();assert.equal(setup.nodes.get(key).kind,'ScrollRegion');
});

test('compact cards show muted project metadata and execution time on hover',()=>{
  const setup=fixture(),catalog=setup.view.board.catalog;
  const time=new Date(2026,9,3,20,30).getTime();
  catalog.sessions=[{id:'retained',project:'project',worktree:'tree',archived:false,delegation:null,
    activity:{title:'Retained task',queued:0,waiting:null,run:{turn:'turn',status:'running',started_ms:time}}}];
  catalog.session_lanes.retained='running';setup.render();
  const card=setup.nodes.get('activity-retained'),body=card.items[0],header=body.items[0];
  assert.equal(body.items.length,2);
  assert.equal(header.selector,'activity-retained-header');
  assert.deepEqual(plain(header.style.w_full),[]);
  assert.deepEqual(plain(header.style.justify_between),[]);
  const title=setup.nodes.get('activity-retained-title');
  assert.deepEqual(plain(title.style.flex_1),[]);
  assert.deepEqual(plain(title.style.min_w_0),[]);
  assert.deepEqual(plain(title.style.truncate),[]);
  assert.equal(header.items[0].kind,'ThinkingIcon');
  assert.equal(header.items[0].props.phase,'turn_tools_running');
  const metadata=setup.nodes.get('activity-retained-metadata');
  assert.equal(metadata.kind,'HoverSwap');assert.equal(metadata.props.group,'activity-retained');
  assert.equal(header.items[2],metadata);
  assert.deepEqual(plain(setup.nodes.get('activity-retained-location').style.opacity),[0.7]);
  assert.deepEqual(plain(setup.nodes.get('activity-retained-location').style.justify_end),[]);
  assert.deepEqual(plain(setup.nodes.get('activity-retained-appearance').props.value).color,'none');
  assert.equal(setup.nodes.get('activity-retained-time').items[1],'20:30');
  assert.deepEqual(plain(setup.nodes.get('activity-retained-time').style.justify_end),[]);
  assert.equal(setup.nodes.has('activity-retained-worktree-icon'),false);
  assert.equal(setup.nodes.get('activity-retained-content').props.lines,2);
  catalog.sessions[0].activity.run=null;setup.render();
  assert.equal(setup.nodes.has('activity-retained-loading'),false);
  assert.equal(setup.nodes.has('activity-retained-time'),false);
  assert.equal(setup.nodes.get('activity-retained-metadata').items.length,1);
});
