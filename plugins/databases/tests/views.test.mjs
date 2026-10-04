import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

const plain=value=>JSON.parse(JSON.stringify(value));

async function fixture(packageName) {
  let phase='render';
  const buttons=new Map(),nodes=new Map();
  const buttonMethods=new Set(['label','tooltip','loading','size','disabled','outline','primary','secondary','danger','success','warning','ghost','link','compact',
    'w_full','max_w_96']);
  function element(type='div',id=null,props={}) {
    const value={type,selector:id,nodes:[],style:{},props};
    let proxy;
    proxy=new Proxy(value,{get(target,key) {
      if(key in target)return target[key];
      if(key==='id')return id=>{target.selector=id;nodes.set(id,proxy);return proxy;};
      if(key==='child')return child=>{target.nodes.push(child);return proxy;};
      if(key==='children')return children=>{target.nodes.push(...children);return proxy;};
      if(['on_click','on_change','on_key_down'].includes(key))return callback=>{
        assert.equal(phase,'render','callbacks require an open snapshot generation');
        target[key]=callback;if(key==='on_click')buttons.set(target.selector,target);return proxy;
      };
      if(type==='Button')assert.ok(buttonMethods.has(key),`Button.${String(key)} is not exported by the pinned script API`);
      return (...args)=>{target.style[key]=args;return proxy;};
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const native=type=>({new:(id,props)=>element(type,id,props)});
  const source=(await readFile(new URL(`../../${packageName}/dev.sailry.platform/desktop/view.js`,import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace(/^export /gm,'');
  const names=['Button','Icon','Pagination','Tab','TabBar','Resizable','ResizablePanel','DropdownMenu','VForm','Field','Progress'];
  const constructors=Object.fromEntries(names.map(name=>[name,function(id){return element(name,id);}]))
  const globals=Object.fromEntries(['Workspace','PanelHeader','Header','EmptyState','Loading','InputSurface','Picker','SelectableRow','IconButton','SelectField','Tooltip','ResourceTree','DataTable','Modal','NativeContextMenu','Conversation','Terminal','TextField','ActionScope'].map(name=>[name,native(name)]));
  const functions=vm.runInNewContext(`${source}\n({picker,landing,render${packageName==='databases'?',log,results,sqlEditor,editorPanel':''}})`,{
    div:()=>element(),...constructors,...globals,
    theme:()=>({colors:{muted_foreground:'muted',foreground:'foreground',border:'border'}}),
    window:{viewport_size:()=>({width:1440,height:940})},
    command:(kind,data)=>({kind,data}),cell:value=>String(value.value),readText:value=>value,
    parent:path=>path.split('/').slice(0,-1).join('/')||'/',failure:()=>'',format:(text,values)=>`${text}:${JSON.stringify(values)}`,
    editor:()=>element('editor',`${packageName==='databases'?'db':'ssh'}-editor`),
    editorFooter:()=>element('editorFooter',`${packageName==='databases'?'db':'ssh'}-editor-footer`),
  });
  const cx={notify(){},stop_propagation(){}};
  return {functions,buttons,nodes,cx,layout(){phase='event';},click(id){const button=buttons.get(id);assert.ok(button,`missing callable ${id}`);button.on_click({},cx);}};
}

function view(packageName,profiles=[]) {
  const owner={text:new Proxy({},{get:(_,key)=>key}),profiles,locked:()=>false,dialog:0,
    picker:false,pickerProfile:null,pickerRevision:0,pending:null,error:null,status:null,editor:null,confirmation:null,
    terminals:[],terminal:null,files:()=>null,commandOpen:false,transfer:null,fileEdit:null,outcome:null,
    opened:null,catalog:{loading:new Set(),names:[],failed:new Set(),items:()=>[],database:null,current:null},
    result:null,page:0,epoch:0,tab:0,logs:[],sql:'',logFocus:{},rowMenu:()=>[],
  };
  owner.profile=()=>profiles.find(profile=>profile.id===owner.opened);
  owner.activeProfile=()=>profiles.find(profile=>profile.id===owner.terminals.find(info=>info.id===owner.terminal)?.ssh);
  return owner;
}

const profiles=[{id:'first',revision:2,name:'First',username:'one',host:'host',port:22,connection:{kind:'sqlite',path:'/fixture/first.sqlite3'}},
  {id:'second',revision:7,name:'Second',username:'two',host:'host',port:2222,connection:{kind:'network',engine:'postgres',username:'two',host:'host',port:5432}}];

for(const packageName of ['databases','ssh']) {
  const prefix=packageName==='databases'?'db':'ssh';
  test(`${packageName} operation outcomes never add feedback to landing or editor bodies`,async()=>{
    for(const editing of [false,true]){
      const setup=await fixture(packageName),owner=view(packageName,profiles);
      owner.error={message:'Raw failure must stay in toast'};owner.status='db_connected';owner.outcome={kind:'connected'};
      if(editing)owner.editor={original:{revision:0}};
      const root=setup.functions.render(owner),strings=[];
      const visit=node=>{if(typeof node==='string')strings.push(node);else for(const child of node?.nodes??[])visit(child);};visit(root);
      assert.equal(setup.nodes.has(`${prefix}-error`),false);assert.equal(setup.nodes.has(`${prefix}-connected`),false);assert.equal(setup.nodes.has('db-status'),false);
      assert.equal(strings.includes('Raw failure must stay in toast'),false);assert.equal(strings.includes('db_connected'),false);assert.equal(strings.includes('ssh_connected'),false);
      assert.ok(setup.nodes.has(`${prefix}_workspace_empty`),'landing content survives the failure');
      if(editing)assert.equal(setup.nodes.get(`${prefix}-dialog-0`).nodes[0].nodes.length,1,'the draft body contains only its editor');
    }
  });
  test(`${packageName} landing uses a native empty state with only new and open actions`,async()=>{
    const setup=await fixture(packageName),owner=view(packageName,profiles);
    setup.functions.render(owner);setup.layout();
    assert.ok(setup.nodes.has(`${prefix}_workspace_empty`));
    assert.equal(setup.nodes.has(`${prefix}-connections-close`),false);
    assert.equal(setup.buttons.size,0,'landing actions use native control events');
    const add=setup.nodes.get(`${prefix}-new`);
    assert.equal(add.type,'IconButton');
    assert.deepEqual(plain(add.props),{icon:'plus',label:`${prefix}_new`,show_label:true,full_width:true,variant:'primary',size:'medium',disabled:false});
    const empty=setup.nodes.get(`${prefix}_workspace_empty`),wrapper=empty.nodes.find(node=>node.nodes?.[0]===add);
    assert.deepEqual(plain(wrapper.style.w_full),[]);assert.deepEqual(plain(wrapper.style.max_w_96),[]);
    assert.deepEqual(plain(setup.nodes.get(`${prefix}-open-second`).props),{variant:'card',label:'Second',disabled:false,selected:false});
    const workspace=setup.nodes.get(packageName==='ssh'?'ssh-workspace':'database-workspace');
    assert.deepEqual(plain(workspace.props),{has_details:false,details_label:packageName==='ssh'?'ssh_assistant':'connection_assistant',min_navigation_width:200,min_details_width:320,navigation:'none'});
    assert.equal(workspace.nodes.length,3);
    assert.equal(setup.nodes.get(`${prefix}-picker-0`).props.open,false);
    assert.equal(setup.nodes.get(`${prefix}-dialog-0`).props.open,false);
    const header=JSON.parse(setup.nodes.get(packageName==='ssh'?'ssh-header':'database-header').props.content);
    assert.equal(header.title,packageName==='ssh'?'ssh':'database');
    assert.equal(header.actions[0].id,`${prefix}-connections-menu`);
  });

  test(`${packageName} picker uses searchable native Command rows and a separate dialog lifecycle`,async()=>{
    const setup=await fixture(packageName),owner=view(packageName,profiles);
    owner.picker=true;owner.pickerRevision=4;
    setup.functions.render(owner);
    let picker=setup.nodes.get(`${prefix}-picker-4`);
    assert.equal(picker.props.mode,'command');assert.equal(picker.props.title,'connection_search');assert.equal(picker.props.empty,'connection_search_empty');
    assert.deepEqual(plain(picker.props.items.map(item=>item.id)),['connection-choice-0','connection-choice-1','connection-add']);
    assert.equal(picker.props.items[1].submenu,true);
    assert.deepEqual(plain(picker.props.items[1].value),{kind:'profile',id:'second'});
    assert.equal(setup.nodes.get(`${prefix}-dialog-0`).props.open,false,'native Picker must not be nested in Modal');
    owner.pickerProfile='second';owner.pickerRevision++;
    setup.functions.picker(owner);picker=setup.nodes.get(`${prefix}-picker-5`);
    assert.deepEqual(plain(picker.props.items.map(item=>item.id)),['connection-action-open','connection-action-edit','connection-action-check',...(packageName==='ssh'?['connection-action-transfer','connection-action-run']:[]),'connection-action-remove','connection-back']);
  });

  test(`${packageName} editor uses native titled form body and footer slots`,async()=>{
    for(const revision of [0,3]) {
      const setup=await fixture(packageName),owner=view(packageName,profiles);
      owner.editor={original:{revision}};owner.dialog=7;
      setup.functions.render(owner);
      const modal=setup.nodes.get(`${prefix}-dialog-7`);
      assert.deepEqual(plain(modal.props),{open:true,form:true,title:`${prefix}_${revision?'edit':'new'}`,width:560});
      assert.equal(modal.nodes.length,2);
      assert.equal(modal.nodes[0].nodes[0].selector,`${prefix}-editor`);
      assert.equal(modal.nodes[1].selector,`${prefix}-editor-footer`);
      assert.equal('w' in modal.nodes[0].style,false,'native form owns the width');
      assert.equal('pt_4' in modal.nodes[0].style,false,'native form owns body top padding');
    }
  });
}

test('database catalog failure keeps its tree and header refresh without a duplicate retry bar',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);
  owner.opened='first';owner.catalog.failed.add('');
  setup.functions.render(owner);
  assert.ok(setup.nodes.has('database-tree'));
  assert.ok(setup.nodes.has('db-refresh-catalog'));
  assert.equal(setup.nodes.has('db-catalog-retry'),false);
});

test('SSH transfer, trust, and command dialogs use native titled form slots',async()=>{
  for(const kind of ['transfer','trust','command']) {
    const setup=await fixture('ssh'),owner=view('ssh',profiles);
    owner.dialog=5;
    if(kind==='transfer') {
      owner.transfer={worktree:'tree',inputs:['source','destination']};
      owner.worktrees=[{id:'tree',project:'project',path:'/fixture/tree'}];owner.projects=[{id:'project',name:'Fixture'}];
    } else if(kind==='trust')owner.outcome={kind:'host_key_required',changed:true,key:{algorithm:'fixture',fingerprint:'Fixture fingerprint'}};
    else {owner.commandOpen=true;owner.command='pwd';}
    setup.functions.render(owner);
    const modal=setup.nodes.get('ssh-dialog-5');
    assert.deepEqual(plain(modal.props),{open:true,form:true,title:{transfer:'ssh_transfer',trust:'ssh_trust_title',command:'ssh_run'}[kind],width:560});
    assert.equal(modal.nodes.length,2);
    assert.equal('w' in modal.nodes[0].style,false);
    assert.equal('max_h' in modal.nodes[0].style,false,'native form owns scrolling and maximum height');
    assert.equal('pt_4' in modal.nodes[0].style,false);
    assert.ok(setup.nodes.has({transfer:'ssh-transfer-editor',trust:'ssh-host-key',command:'ssh-command'}[kind]));
    assert.ok(setup.buttons.has({transfer:'ssh-upload',trust:'ssh-trust',command:'ssh-run'}[kind]));
  }
});

test('SSH Transfer worktree selects preserve selection and pending guards',async()=>{
  for(const state of ['ready','pending','empty']) {
    const setup=await fixture('ssh'),owner=view('ssh',profiles);
    owner.dialog=6;owner.transfer={worktree:'tree',inputs:['source','destination']};owner.projects=[{id:'project',name:'Fixture'}];
    owner.worktrees=state==='empty'?[]:[{id:'tree',project:'project',path:'/fixture/tree'}];owner.locked=()=>state==='pending';
    setup.functions.render(owner);
    const menu=setup.nodes.get('ssh-worktree-6');assert.equal(menu.type,'SelectField');
    assert.deepEqual(plain(menu.props),{label:'ssh_worktree',placeholder:'ssh_no_project',selected:'tree',disabled:state!=='ready',
      items:state==='empty'?[]:[{id:'tree',label:'Fixture · /fixture/tree'}]});
  }
});

test('SSH keeps a terminal after profile removal while unmounting its assistant and file navigation',async()=>{
  const setup=await fixture('ssh'),owner=view('ssh',profiles);
  owner.terminals=[{id:'retained',ssh:'removed',status:{kind:'running'}}];owner.terminal='retained';
  setup.functions.render(owner);
  assert.ok(setup.nodes.has('ssh-terminal-retained'));
  assert.equal(setup.nodes.get('ssh-workspace').props.has_details,false);
  assert.equal(setup.nodes.get('ssh-workspace').props.navigation,'none');
  assert.equal(setup.nodes.has('ssh-assistant-removed'),false);
});

test('SSH assistant is bound to the active existing profile, not a selected saved connection',async()=>{
  const setup=await fixture('ssh'),owner=view('ssh',profiles);
  owner.selected='first';owner.terminals=[{id:'active',ssh:'second'}];owner.terminal='active';
  setup.functions.render(owner);
  const assistant=setup.nodes.get('ssh-assistant-second');
  assert.deepEqual(plain(assistant.props),{assistant:'ssh',resource:{kind:'ssh',id:'second'},heading:'ssh_assistant'});
  assert.equal(setup.nodes.get('ssh-workspace').props.has_details,true);
  assert.equal(setup.nodes.get('ssh-workspace').props.navigation,'resource');
});

test('SSH Files gives its native context and tree a flex column that fills navigation height',async()=>{
  const setup=await fixture('ssh'),owner=view('ssh',profiles);
  owner.terminals=[{id:'active',ssh:'first'}];owner.terminal='active';owner.jobs=[];owner.fileMenu=()=>[];
  owner.files=()=>({input:'path-input',path:'/',pending:false,profile:profiles[0],entries:[{name:'fixture.txt',kind:'file'}],selected:[],current:null,items:()=>[]});
  setup.functions.render(owner);
  const panel=setup.nodes.get('ssh-files-panel'),body=panel.nodes.find(node=>node.nodes?.[0]?.selector==='ssh-files-root');
  assert.deepEqual(plain(setup.nodes.get('ssh-files-header').props),{size:'row',padding:8,draggable:false,bordered:true});
  assert.ok(body);
  for(const method of ['v_flex','flex_1','min_h_0','min_w_0','p_2'])assert.deepEqual(plain(body.style[method]),[]);
  assert.equal(body.nodes[0].nodes[0].selector,'ssh-files');
  assert.equal(setup.nodes.get('ssh-workspace').nodes[2],panel);
});

test('empty SSH directories fill their viewport without cards',async()=>{
  for(const state of ['empty','pending','failed','unloaded']) {
    const setup=await fixture('ssh'),owner=view('ssh',profiles);
    owner.terminals=[{id:'active',ssh:'first'}];owner.terminal='active';owner.jobs=[];
    owner.fileMenu=()=>[{id:'refresh',label:'Refresh',enabled:true}];
    owner.files=()=>({input:'retained-path',path:'/',pending:state==='pending',error:state==='failed'?'ssh_files_failed':null,
      revision:state==='unloaded'?0:1,profile:profiles[0],entries:[],selected:[],current:null,items:()=>[]});
    setup.functions.render(owner);
    const panel=setup.nodes.get('ssh-files-panel'),context=setup.nodes.get('ssh-files-root');
    const body=panel.nodes.find(node=>node.nodes?.[0]===context);
    assert.ok(body);
    assert.deepEqual(plain(context.props.items),[{id:'refresh',label:'Refresh',enabled:true}]);
    assert.equal(setup.nodes.get('ssh-workspace').nodes[2],panel);
    if(state==='empty') {
      const empty=setup.nodes.get('ssh-files-empty');
      assert.deepEqual(plain(empty.props),{variant:'list',fill_height:true,icon:'folder',label:'files_directory_empty'});
      assert.equal(context.nodes[0],empty);
      assert.equal(setup.nodes.has('ssh-files'),false);
      assert.equal(panel.nodes.includes(empty),false,'the empty state belongs inside the viewport');
    } else {
      assert.equal(setup.nodes.has('ssh-files-empty'),false);
      assert.equal(context.nodes[0].selector,'ssh-files');
    }
  }
});

test('database catalog and assistant use native resource slots, with an empty striped result',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);owner.opened='first';
  setup.functions.render(owner);
  assert.equal(setup.nodes.get('database-workspace').props.navigation,'resource');
  assert.equal(setup.nodes.get('database-workspace').nodes[2].selector,'db-catalog');
  assert.deepEqual(plain(setup.nodes.get('database-catalog-header').props),{size:'row',draggable:false,bordered:true});
  assert.equal(setup.nodes.get('database-tree').props.variant,'branch');
  assert.equal(setup.nodes.get('database-tree').props.current,null);
  assert.deepEqual(plain(setup.nodes.get('database-assistant-first').props),{assistant:'database',resource:{kind:'database',id:'first'},heading:'connection_assistant'});
  assert.equal(setup.nodes.has('db_data_empty'),false);
  assert.deepEqual(plain(setup.nodes.get('database-results').props),{revision:'0',columns:[],rows:[],page:0,menu:[],stripe:true,empty_stripes:true,header:false});
});

test('database catalog renders the retained UI selection through the native tree',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);owner.opened='first';owner.catalog.current='table:0:1';
  setup.functions.render(owner);
  assert.equal(setup.nodes.get('database-tree').props.current,'table:0:1');
});

test('an open database uses one native closeable header tab, disabled while pending',async()=>{
  for(const pending of [null,{request:{unknown:false}},{request:{unknown:true}}]) {
    const setup=await fixture('databases'),owner=view('databases',profiles);owner.opened='first';owner.pending=pending;
    setup.functions.render(owner);
    const header=JSON.parse(setup.nodes.get('database-header').props.content);
    assert.equal(header.title,'database');
    assert.deepEqual(header.tabs,{id:'db-tabs',selected:'first',max_width:180,close_label:'close',
      items:[{id:'first',label:'First',icon:'hard-drive',closable:true,close_disabled:!!pending}]});
  }
});

test('database result pagination uses Kit page numbers',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);
  owner.result={columns:['value'],rows:Array.from({length:123},(_,value)=>[{kind:'text',value}]),truncated:false};
  owner.page=1;setup.functions.results(owner);
  const pages=setup.nodes.get('db-pages');assert.equal(pages.type,'Pagination');
  assert.deepEqual(plain(pages.style.current_page),[2]);assert.deepEqual(plain(pages.style.total_pages),[3]);assert.deepEqual(plain(pages.style.visible_pages),[3]);
  pages.on_change(3,setup.cx);assert.equal(owner.page,2);
  assert.equal(setup.nodes.get('database-results').props.stripe,true);
  assert.equal(setup.nodes.get('database-results').props.empty_stripes,true);
  assert.equal(setup.nodes.get('database-results').props.header,true);
});

test('database zero-row results preserve real columns without no-data text',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);
  owner.result={columns:['id','value'],rows:[],truncated:false};
  setup.functions.results(owner);
  const table=setup.nodes.get('database-results');
  assert.deepEqual(plain(table.props.columns),['id','value']);
  assert.deepEqual(plain(table.props.rows),[]);
  assert.equal(table.props.header,true);
  assert.equal(table.props.empty_stripes,true);
  assert.equal('empty' in table.props,false);
  assert.equal(setup.nodes.has('db_data_empty'),false);
});

test('database bottom tabs use the square native tab variant',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);owner.opened='first';
  const panel=setup.functions.editorPanel(owner),tabs=panel.nodes[0];
  assert.deepEqual(plain(tabs.style.variant),['tab']);
  assert.deepEqual(plain(tabs.style.rounded_none),[]);
  tabs.on_change(1,setup.cx);assert.equal(owner.tab,1);
});

test('database SQL actions use accessible native icon buttons',async()=>{
  for(const state of ['ready','running','unknown']) {
    const setup=await fixture('databases'),owner=view('databases',profiles);owner.opened='first';owner.sql='SELECT 1';
    if(state!=='ready')owner.pending={request:{unknown:state==='unknown'}};
    setup.functions.sqlEditor(owner);
    const id=state==='ready'?'db-run':state==='unknown'?'db-inspect':'db-stop';
    assert.deepEqual(plain(setup.nodes.get(id).props),{icon:state==='ready'?'play':state==='unknown'?'rotate-cw':'square',
      label:state==='ready'?'db_run':state==='unknown'?'db_inspect':'db_stop',variant:'primary',circular:true,size:'medium',disabled:false});
    assert.equal(setup.buttons.size,0);
  }
});

test('capped database history has native selectable rows and captured copy/open menus',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);owner.opened='first';
  owner.logs=Array.from({length:100},(_,index)=>({id:`entry-${index}`,sql:`SELECT '${index} 中文 🙂'`,database:`db-${index}`,timestamp:index,elapsed:0.25,result:{rows:{columns:['value'],rows:[],affected_rows:0}}}));
  setup.functions.log(owner);setup.layout();assert.equal(setup.buttons.size,0,'history actions belong in the OS-native menu');
  const row=setup.nodes.get('db-history-entry-99');
  assert.equal(row.type,'NativeContextMenu');assert.deepEqual(plain(row.props.items.map(item=>JSON.parse(item.id.slice(4)))),[
    {profile:'first',id:'entry-99',edit:false},{profile:'first',id:'entry-99',edit:true},
  ]);
  assert.equal(setup.nodes.get(`db-log:${JSON.stringify({profile:'first',id:'entry-99'})}`).props.variant,'list');
  for(let index=0;index<100;index++) {
    const entry=`entry-${99-index}`,wrapper=setup.nodes.get(`db-history-row-${entry}`);
    assert.deepEqual(plain(wrapper.style.h_8),[]);
    assert.deepEqual(plain(wrapper.style.flex_none),[]);
    assert.equal(wrapper.nodes[0],setup.nodes.get(`db-history-${entry}`));
    assert.equal(setup.nodes.get(`db-log:${JSON.stringify({profile:'first',id:entry})}`).props.stripe,index%2===1);
  }
  assert.deepEqual(plain(setup.nodes.get('database-history-grid').props),{revision:'0',columns:[],rows:[],stripe:true,empty_stripes:true,header:false});
  assert.equal(setup.nodes.get('database-history-rows').nodes.length,100);
});

test('empty database history has viewport stripes and no artificial log entries',async()=>{
  const setup=await fixture('databases'),owner=view('databases',profiles);
  setup.functions.log(owner);
  assert.deepEqual(plain(setup.nodes.get('database-history-grid').props.rows),[]);
  assert.equal(setup.nodes.get('database-history-rows').nodes.length,0);
  assert.equal(owner.logs.length,0);
});
