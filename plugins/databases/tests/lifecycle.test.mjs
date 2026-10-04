import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

const settle=async()=>{for(let index=0;index<12;index++)await Promise.resolve();};

test('database startup and cached entries coalesce refreshes within host capacity',async()=>{
  const calls=[],held=[];let active=0,maximum=0,revision=0;
  const read=async(kind)=>{
    active++;maximum=Math.max(maximum,active);calls.push(kind);
    try {
      if(active>4)throw new Error('plugin request capacity exhausted');
      await new Promise(resolve=>held.push(resolve));
      if(kind==='databases')return [{id:'saved',revision:++revision}];
      if(kind==='projects')return {projects:[{id:`project-${revision}`}]};
      return [];
    }finally{active--;}
  };
  const nextChange=()=>{},globals={context:()=>'{"locale":"en"}',messages:()=>({db_sql:'SQL'}),createText:()=>({draft:'retained'}),
    listDatabases:()=>read('databases'),listSsh:()=>read('ssh'),readProjectCatalog:()=>read('projects'),nextChange};
  for(const name of ['nextTableEvent','nextTreeEvent','nextContextMenuEvent','header_action','nextNavigationTabEvent','nextPickerEvent','nextControlEvent','nextTextEvent','modal_closed','nextAssistantTools'])globals[name]=()=>{};
  const setup=await fixture('databases',globals),owner=setup.owner;let change;
  owner.watch=(read,handle)=>{if(read===nextChange)change=handle;};
  setup.live.focus_handle=()=>({});owner.init({},setup.live);
  const result={rows:[['retained']]},origin={},logs=[{id:'retained'}],loads=[];
  const catalog={loading:new Set(),expanded:new Set(['main']),load:async(database,cx)=>loads.push([database,cx])};
  Object.assign(owner,{opened:'saved',catalog,result,origin,logs,selectedLog:'retained'});
  change({connected:true,entry:'0'},setup.live);
  for(let entry=1;entry<=8;entry++)change({connected:true,entry:String(entry)},setup.live);
  await settle();
  assert.equal(calls.filter(kind=>kind==='databases').length,1,'one initial inventory read must own the overlapping entries');
  assert.deepEqual(setup.errors,[]);
  for(let turn=0;held.length&&turn<32;turn++){held.splice(0).forEach(resolve=>resolve());await settle();}
  await Promise.all(setup.tasks);
  assert.ok(maximum<=2,`refresh used ${maximum} concurrent host requests`);
  assert.deepEqual(calls,['databases','ssh','projects','databases','ssh','projects']);
  assert.equal(owner.profiles[0].revision,2);assert.equal(owner.projects[0].id,'project-2');
  assert.deepEqual(loads,[[null,setup.live],['main',setup.live]]);
  assert.equal(owner.catalog,catalog);assert.equal(owner.result,result);assert.equal(owner.origin,origin);assert.equal(owner.logs,logs);
  assert.equal(owner.sql.draft,'retained');assert.equal(owner.refreshing,null);assert.deepEqual(setup.errors,[]);assert.deepEqual(setup.notices,[]);
});

for(const failedRead of ['databases','ssh','projects','catalog','expanded']) {
  test(`database ${failedRead} refresh failure keeps unmet entry demand for the next ordinary refresh`,async()=>{
    const calls=[],loads=[];let failed=false,reject;
    const pending=new Promise((_,fail)=>{reject=fail;});
    const read=async(kind)=>{calls.push(kind);if(kind===failedRead&&!failed){failed=true;await pending;}};
    const setup=await fixture('databases',{listDatabases:async()=>{await read('databases');return [{id:'saved'}];},
      listSsh:async()=>{await read('ssh');return [];},readProjectCatalog:async()=>{await read('projects');return {projects:[{id:'project'}]};}}),owner=setup.owner;
    const catalog={loading:new Set(),expanded:new Set(['main']),load:async(database,cx)=>{loads.push([database,cx]);await read(database===null?'catalog':'expanded');}};
    Object.assign(owner,{profiles:[{id:'saved'}],ssh:[],projects:[],opened:'saved',catalog});
    const first=owner.refresh(setup.live,true,true),queued=owner.refresh(setup.live);
    assert.equal(first,queued);await settle();reject(new Error('Fixture read failed'));
    await assert.rejects(first,/Fixture read failed/);assert.equal(owner.refreshing,null);
    assert.equal(owner.refreshProjects,['databases','ssh','projects'].includes(failedRead));assert.equal(owner.refreshCatalog,true);
    const previous=calls.length;await settle();assert.equal(calls.length,previous,'a failed refresh must not retry by itself');
    await owner.refresh(setup.live);assert.equal(owner.projects[0].id,'project');assert.equal(owner.refreshing,null);
    assert.equal(calls.filter(kind=>kind==='projects').length,failedRead==='projects'?2:1);
    assert.deepEqual(loads.slice(-2),[[null,setup.live],['main',setup.live]]);
    assert.equal(owner.refreshProjects,false);assert.equal(owner.refreshCatalog,false);
  });
}

for(const operation of ['save','remove']) {
  test(`database ${operation} waits for inventory requested during an earlier refresh`,async()=>{
    const held=[];let reads=0,completed=false;
    const profile={id:'saved',revision:1,name:'Saved'},editor={inputs:['Name','/fixture/data.sqlite3'],secret:'secret'};
    const setup=await fixture('databases',{listDatabases:async()=>{const read=++reads;await new Promise(resolve=>held.push(resolve));return operation==='remove'&&read===2?[]:[{...profile,revision:read}];},
      listSsh:async()=>[],readText:value=>value,prepare:()=>profile,prepareDatabase:()=> 'saved-request',describeSecret:()=>({filled:false}),
      releaseText(){},releaseSecret(){},format:()=>'',Request:class {
        constructor(){this.done=false;}
        async start(){this.done=true;this.output={kind:'database_outcome',data:{kind:operation==='save'?'saved':'removed'}};}
        release(){}
      }}),owner=setup.owner;
    Object.assign(owner,{connected:true,pending:null,profiles:[profile],ssh:[],projects:[],opened:'saved',catalog:null,editor,logs:[],dialog:0,epoch:0});
    const initial=owner.refresh(setup.live);
    if(operation==='save')owner.save(false,setup.event);else {owner.remove(profile,setup.event);owner.confirm(setup.live);}
    setup.close();const finished=Promise.all(setup.tasks).then(()=>{completed=true;});await settle();
    assert.equal(reads,1);assert.equal(completed,false);assert.equal(owner.editor,editor);assert.deepEqual(setup.notices,[]);
    held.shift()();await settle();assert.equal(reads,2);assert.equal(completed,false);assert.deepEqual(setup.notices,[]);
    held.shift()();await Promise.all([initial,finished]);
    assert.equal(completed,true);assert.equal(owner.refreshing,null);assert.deepEqual(setup.errors,[]);
    if(operation==='save'){assert.equal(owner.profiles[0].revision,2);assert.equal(owner.editor,null);assert.equal(setup.notices[0].message,'db_saved');}
    else {assert.equal(owner.profiles.length,0);assert.equal(owner.opened,null);assert.equal(owner.editor,editor);assert.deepEqual(setup.notices,[]);}
  });
}

async function fixture(packageName,globals={}) {
  const notices=[];
  const source=(await readFile(new URL(`../../${packageName}/dev.sailry.platform/desktop/main.js`,import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class').replace(/^export /gm,'');
  const Controller=vm.runInNewContext(`${source}\n${packageName==='ssh'?'Ssh':'Databases'}`,{View:class{},toast:value=>notices.push(value),...globals});
  const owner=new Controller(),tasks=[],errors=[];
  let valid=true,notifications=0;
  const live={notify(){notifications++;},spawn:body=>start(body)};
  const start=body=>{const task=body(live);tasks.push(task);return task;};
  const event={notify(){assert.equal(valid,true,'event context was retained');},spawn:start};
  owner.text=new Proxy({},{get:(_,key)=>key});
  const report=owner.report.bind(owner);
  owner.report=error=>{errors.push(error);if(globals.feedback)report(error);};
  return {owner,event,live,errors,notices,notifications:()=>notifications,close:()=>{valid=false;},tasks};
}

for(const packageName of ['databases','ssh']) {
  test(`${packageName} failed operations toast once without discarding drafts or previous results`,async()=>{
    const error={code:'invalid_request',message:'Invalid fixture connection'},editor={inputs:['Retained name','Retained address']},result={rows:[['retained']]},origin={},profiles=[{id:'profile',revision:4}];
    const setup=await fixture(packageName,{feedback:true,failure:()=>packageName==='ssh'?'ssh_failed':'db_failed',Request:class {
      constructor(command){this.command=command;this.id='request';this.done=false;}
      async start(){this.error=error;this.done=true;}
      release(){}
    }}),owner=setup.owner,output={kind:'completed',stdout:'Previous output',stderr:'',exit_code:0};
    Object.assign(owner,{connected:true,pending:null,editor,result,origin,profiles,logs:[],page:2,epoch:3,opened:'profile',selected:'profile',commandOpen:true,outcome:output});
    await owner.execute({kind:packageName==='ssh'?'check_ssh':'check_database'},setup.live);
    assert.equal(owner.editor,editor);assert.equal(owner.result,result);assert.equal(owner.origin,origin);assert.equal(owner.profiles,profiles);
    assert.equal(owner.page,2);assert.equal(owner.epoch,3);assert.equal(owner.pending,null);
    if(packageName==='ssh')assert.equal(owner.outcome,output);
    assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].kind,'error');assert.match(setup.notices[0].message,/Invalid fixture connection/);
    await owner.finish(setup.live);assert.equal(setup.notices.length,1,'completed failures are not reported twice');
  });

  test(`${packageName} connection test success uses a toast and keeps the editor draft`,async()=>{
    const setup=await fixture(packageName,{Request:class {
      constructor(){this.done=false;}
      async start(){this.output={kind:packageName==='ssh'?'ssh_outcome':'database_outcome',data:{kind:'connected'}};this.done=true;}
      release(){}
    }}),owner=setup.owner,editor={inputs:['Retained name']};
    Object.assign(owner,{connected:true,pending:null,editor,profiles:[],logs:[]});
    await owner.execute({kind:packageName==='ssh'?'check_ssh':'check_database'},setup.live);
    assert.equal(owner.editor,editor);assert.equal(owner.pending,null);assert.equal(setup.notices.length,1);
    assert.equal(setup.notices[0].kind,'info');assert.equal(setup.notices[0].message,packageName==='ssh'?'ssh_connected':'db_connected');
  });

  test(`${packageName} uncertain operations toast while retaining their original request`,async()=>{
    const setup=await fixture(packageName,{feedback:true,failure:()=>packageName==='ssh'?'ssh_unknown':'db_unknown',Request:class {
      constructor(){this.id='original-request';this.done=false;}
      async start(){this.unknown=true;}
    }}),owner=setup.owner;
    Object.assign(owner,{connected:true,pending:null,profiles:[],logs:[]});
    await owner.execute({kind:packageName==='ssh'?'check_ssh':'check_database'},setup.live);
    assert.equal(owner.pending.request.id,'original-request');assert.equal(owner.pending.request.unknown,true);
    assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].kind,'error');assert.equal(setup.notices[0].message,packageName==='ssh'?'ssh_unknown':'db_unknown');
  });

  test(`${packageName} actions receive the live spawned context`,async()=>{
    const state=await fixture(packageName);let accepted;
    state.owner.run(async cx=>{accepted=cx;await Promise.resolve();cx.notify();},state.event);
    state.close();await Promise.all(state.tasks);
    assert.equal(accepted,state.live);assert.deepEqual(state.errors,[]);
    assert.equal(state.notifications(),2);
  });

  test(`${packageName} event streams receive the live context after initialization`,async()=>{
    const state=await fixture(packageName);let resolve,accepted;
    const pending=new Promise(done=>{resolve=done;});let reads=0;
    state.owner.watch(()=>reads++===0?pending:new Promise(()=>{}),(event,cx)=>{accepted=cx;cx.notify();},state.event);
    state.close();resolve({kind:'changed'});await settle();
    assert.equal(accepted,state.live);assert.equal(state.notifications(),2);
  });

  test(`${packageName} saved confirmations receive the confirming task context`,async()=>{
    const state=await fixture(packageName,{format:()=>'',messages:()=>({})});
    const profile={id:'captured-profile',revision:7,name:'Connection'};let accepted;
    Object.assign(state.owner,{text:{},pending:null,connected:true,dialog:0});
    state.owner.execute=async(command,cx)=>{await Promise.resolve();accepted=cx;cx.notify();};
    state.owner.remove(profile,state.event);state.close();
    state.owner.confirm(state.live);await Promise.all(state.tasks);
    assert.equal(accepted,state.live);assert.deepEqual(state.errors,[]);
  });
}

test('database editor work keeps the spawned context through request completion',async()=>{
  let prepared=0;
  const state=await fixture('databases',{
    readText:value=>value,prepare:()=>({id:'captured-profile'}),
    prepareDatabase:()=>{prepared++;return 'captured-request';},describeSecret:()=>({filled:false}),
    releaseText(){},releaseSecret(){},
    Request:class {
      constructor(_,id){this.id=id;this.done=false;}
      async start(cx){await Promise.resolve();cx.notify();this.done=true;this.output={kind:'database_outcome',data:{kind:'connected'}};}
      release(){}
    },
  });
  Object.assign(state.owner,{connected:true,pending:null,editor:{inputs:['Name','/fixture/data.sqlite3'],secret:'secret'},ssh:[],logs:[]});
  let refreshed;
  state.owner.refresh=async cx=>{refreshed=cx;};
  state.owner.save(true,state.event);state.close();await Promise.all(state.tasks);
  assert.equal(prepared,1);assert.equal(refreshed,state.live);assert.deepEqual(JSON.parse(JSON.stringify(state.notices)),[{id:'database-success',kind:'info',message:'db_connected'}]);
  assert.ok(state.owner.editor,'connection tests retain their draft');
  assert.equal(state.owner.pending,null);assert.deepEqual(state.errors,[]);
});

for(const packageName of ['databases','ssh']) {
  test(`${packageName} native New actions open a draft only while unlocked`,async()=>{
    const setup=await fixture(packageName),owner=setup.owner,calls=[];
    Object.assign(owner,{connected:true,pending:null});owner.edit=(profile,cx)=>calls.push([profile,cx]);
    const id=packageName==='ssh'?'ssh-new':'db-new';
    owner.control({id},setup.live);assert.deepEqual(calls,[[null,setup.live]]);
    owner.connected=false;owner.control({id},setup.live);
    owner.connected=true;owner.pending={request:{}};owner.control({id},setup.live);assert.equal(calls.length,1);
  });

  test(`${packageName} empty connections menu retains the native picker`,async()=>{
    const setup=await fixture(packageName),owner=setup.owner;
    Object.assign(owner,{profiles:[],connected:true,pending:null,picker:false,pickerProfile:null,pickerRevision:0});
    owner.edit=()=>assert.fail('the menu must not bypass the native picker');
    owner.connections(setup.event);
    assert.equal(owner.picker,true);assert.equal(owner.pickerProfile,null);assert.equal(owner.pickerRevision,1);
  });

  test(`${packageName} native saved-card events resolve only live unlocked profiles`,async()=>{
    const setup=await fixture(packageName),owner=setup.owner,profile={id:'saved',revision:4},calls=[];
    Object.assign(owner,{profiles:[profile],connected:true,pending:null});
    owner.open=(value,cx)=>calls.push([value,cx]);
    const prefix=packageName==='ssh'?'ssh':'db';
    owner.control({id:`${prefix}-open-saved`},setup.event);await Promise.all(setup.tasks);
    assert.deepEqual(calls,[[profile,setup.live]]);
    owner.profiles=[];owner.control({id:`${prefix}-open-saved`},setup.live);
    owner.profiles=[profile];owner.connected=false;owner.control({id:`${prefix}-open-saved`},setup.live);
    assert.equal(calls.length,1);
  });

  test(`${packageName} command picker navigates before dispatch and ignores stale declarations`,async()=>{
    const setup=await fixture(packageName),owner=setup.owner,profiles=[{id:'first',revision:2},{id:'second',revision:7}],calls=[];
    const prefix=packageName==='ssh'?'ssh':'db';
    Object.assign(owner,{profiles,connected:true,pending:null,picker:false,pickerProfile:null,pickerRevision:0,dialog:0});
    owner.execute=(value,cx)=>calls.push([value,cx]);
    owner.connections(setup.event);
    owner.pick({picker:`${prefix}-picker-1`,kind:'select',value:{kind:'profile',id:'second'}},setup.event);
    assert.equal(owner.pickerProfile,'second');assert.equal(owner.pickerRevision,2);assert.equal(calls.length,0);
    owner.pick({picker:`${prefix}-picker-1`,kind:'select',value:{kind:'action',action:'check'}},setup.event);
    assert.equal(calls.length,0);
    owner.pick({picker:`${prefix}-picker-2`,kind:'select',value:{kind:'action',action:'check'}},setup.event);
    await Promise.all(setup.tasks);
    assert.equal(owner.picker,false);
    assert.deepEqual(JSON.parse(JSON.stringify(calls[0][0])),{kind:packageName==='ssh'?'check_ssh':'check_database',data:{profile:'second',expected_revision:7}});
    assert.equal(calls[0][1],setup.live);
    owner.connections(setup.live);
    owner.pick({picker:`${prefix}-picker-2`,kind:'close'},setup.live);
    assert.equal(owner.picker,true,'closing a released picker cannot close the new declaration');
    owner.pick({picker:`${prefix}-picker-3`,kind:'select',value:{kind:'profile',id:'removed'}},setup.live);
    assert.equal(owner.pickerProfile,null);
    owner.pick({picker:`${prefix}-picker-3`,kind:'close'},setup.live);assert.equal(owner.picker,false);
  });
}

test('native icon actions retain the captured browser, catalog, and request',async()=>{
  const ssh=await fixture('ssh',{parent:()=>'/parent'}),loaded=[];
  const files={pending:false,path:'/parent/child',load:(path,cx)=>loaded.push([path,cx])};
  ssh.owner.files=()=>files;
  ssh.owner.control({id:'ssh-files-parent'},ssh.event);await Promise.all(ssh.tasks);
  assert.deepEqual(loaded,[['/parent',ssh.live]]);
  files.pending=true;ssh.owner.control({id:'ssh-files-parent'},ssh.live);assert.equal(loaded.length,1);
  const database=await fixture('databases'),calls=[],owner=database.owner;
  const catalog={tables:new Map([['old',[]]]),names:['old'],expanded:new Set(['old']),current:'db:0',loading:new Set(),load:(name,cx,reset)=>calls.push(['catalog',name,cx,reset])};
  owner.catalog=catalog;owner.profile=()=>({id:'saved'});
  owner.runSql=cx=>calls.push(['run',cx]);owner.check=cx=>calls.push(['check',cx]);
  owner.control({id:'db-refresh-catalog'},database.event);owner.control({id:'db-run'},database.event);
  const request={unknown:false,cancel:()=>calls.push(['cancel',request])};owner.pending={request};
  owner.control({id:'db-stop'},database.event);owner.control({id:'db-inspect'},database.event);
  request.unknown=true;owner.control({id:'db-stop'},database.event);owner.control({id:'db-inspect'},database.event);
  await Promise.all(database.tasks);
  assert.deepEqual(calls,[['catalog',null,database.live,true],['run',database.event],['cancel',request],['check',database.live]]);
  assert.equal(catalog.tables.size,1);assert.equal(catalog.names.length,1);assert.equal(catalog.expanded.size,1);
  assert.equal(catalog.current,'db:0');
});

test('database catalog selection survives explicit fold events and rejects other trees',async()=>{
  const setup=await fixture('databases'),owner=setup.owner,loads=[];
  const catalog={current:null,expanded:new Set(),target:id=>['db:0','table:0:0'].includes(id)?{database:'main'}:null,load:async database=>loads.push(database)};
  owner.catalog=catalog;
  await owner.tree({tree:'other',kind:'select',id:'db:0'},setup.live);
  await owner.tree({tree:'database-tree',kind:'select',id:'missing'},setup.live);assert.equal(catalog.current,null);
  await owner.tree({tree:'database-tree',kind:'select',id:'db:0'},setup.live);
  await owner.tree({tree:'database-tree',kind:'expand',id:'db:0'},setup.live);assert.equal(catalog.current,'db:0');assert.equal(catalog.expanded.has('main'),true);
  await owner.tree({tree:'database-tree',kind:'collapse',id:'db:0'},setup.live);assert.equal(catalog.current,'db:0');assert.equal(catalog.expanded.has('main'),false);
  assert.deepEqual(loads,['main']);
});

test('editor native controls reject stale dialog generations and pending key selection',async()=>{
  let picking=false;
  const ssh=await fixture('ssh',{describeSecret:()=>({picking})});
  Object.assign(ssh.owner,{connected:true,pending:null,dialog:3,editor:{key:'key',authentication:'password'}});
  ssh.owner.control({id:'ssh-authentication-2',value:'private_key'},ssh.live);assert.equal(ssh.owner.editor.authentication,'password');
  picking=true;ssh.owner.control({id:'ssh-authentication-3',value:'private_key'},ssh.live);assert.equal(ssh.owner.editor.authentication,'password');
  picking=false;ssh.owner.control({id:'ssh-authentication-3',value:'private_key'},ssh.live);assert.equal(ssh.owner.editor.authentication,'private_key');
  const database=await fixture('databases',{readText:value=>value,setText(){},switchEngine:(editor,engine)=>{editor.engine=engine;}});
  Object.assign(database.owner,{connected:true,pending:null,dialog:4,ssh:[{id:'saved'}],editor:{engine:'sqlite',inputs:[],method:'tcp',tls:'disable',ssh:null}});
  database.owner.control({id:'db-engine-3',value:'mysql'},database.live);assert.equal(database.owner.editor.engine,'sqlite');
  database.owner.control({id:'db-engine-4',value:'mysql'},database.live);assert.equal(database.owner.editor.engine,'mysql');
  database.owner.control({id:'db-tls-4',value:'require'},database.live);assert.equal(database.owner.editor.tls,'require');
  database.owner.control({id:'db-ssh-4',value:'missing'},database.live);assert.equal(database.owner.editor.ssh,null);
  database.owner.control({id:'db-ssh-4',value:'saved'},database.live);assert.equal(database.owner.editor.ssh,'saved');
});

test('key selection blocks duplicate prompts and save while retaining the captured draft',async()=>{
  let resolve,picking=false,opened=0;
  const pending=new Promise(done=>{resolve=done;});
  const setup=await fixture('ssh',{describeSecret:()=>({picking}),chooseSecretFile:()=>{opened++;picking=true;return pending;}});
  const editor={key:'captured-key'};
  Object.assign(setup.owner,{connected:true,pending:null,editor,text:{ssh_key_choose:'Choose key'}});
  setup.owner.chooseKey(setup.event);assert.equal(opened,1);
  setup.owner.chooseKey(setup.event);setup.owner.save(setup.event);assert.equal(opened,1);assert.equal(setup.tasks.length,1);
  const next={key:'next-key'};setup.owner.editor=next;picking=false;resolve();await Promise.all(setup.tasks);
  assert.equal(setup.owner.editor,next);assert.deepEqual(setup.errors,[]);
});

test('database history double-click and native menu remain bound to the displayed profile',async()=>{
  const copied=[],opened=[];
  const setup=await fixture('databases',{writeClipboard:text=>copied.push(text)}),owner=setup.owner;
  Object.assign(owner,{opened:'displayed',tab:0,logs:[{id:'older',sql:'SELECT older',database:'one'},{id:'newer',sql:'SELECT newer',database:'two'}],selectedLog:null,logFocus:{focus(){}}});
  owner.openSql=(sql,database,cx)=>opened.push([sql,database,cx]);
  owner.control({id:`db-log:${JSON.stringify({profile:'displayed',id:'newer'})}`,click_count:2},setup.live);
  assert.equal(owner.selectedLog,'newer');assert.deepEqual(opened,[['SELECT newer','two',setup.live]]);
  await owner.menu(`log:${JSON.stringify({profile:'displayed',id:'older',edit:false})}`,setup.live);assert.deepEqual(copied,['SELECT older']);
  owner.opened='other';await owner.menu(`log:${JSON.stringify({profile:'displayed',id:'older',edit:true})}`,setup.live);
  assert.equal(opened.length,1);
});

test('database tab close rejects stale or pending events and preserves local drafts and history',async()=>{
  const setup=await fixture('databases'),owner=setup.owner,logs=[{id:'log',sql:'SELECT 1'}],sql={draft:'SELECT retained'};
  Object.assign(owner,{profiles:[{id:'active'}],opened:'active',catalog:{},result:{rows:[]},origin:{},status:'db_ready',pending:null,epoch:4,page:2,
    logs,selectedLog:'log',sql,connected:false});
  owner.execute=()=>assert.fail('closing a tab must not issue a Node command');
  for(const event of [{bar:'other-tabs',id:'active',kind:'close'},{bar:'db-tabs',id:'stale',kind:'close'},{bar:'db-tabs',id:'active',kind:'select'}])owner.navigation(event,setup.live);
  assert.equal(owner.opened,'active');
  for(const unknown of [false,true]) {
    owner.pending={request:{unknown}};owner.navigation({bar:'db-tabs',id:'active',kind:'close'},setup.live);
    assert.equal(owner.opened,'active');
  }
  owner.pending=null;owner.navigation({bar:'db-tabs',id:'active',kind:'close'},setup.live);
  for(const key of ['opened','catalog','result','origin','status'])assert.equal(owner[key],null);
  assert.equal(owner.epoch,5);assert.equal(owner.page,0);assert.equal(owner.logs,logs);assert.equal(owner.selectedLog,'log');assert.equal(owner.sql,sql);
  owner.navigation({bar:'db-tabs',id:'active',kind:'close'},setup.live);assert.equal(owner.epoch,5);
});

test('opening another database clears resource state while reopening the same one retains it',async()=>{
  const loaded=[],created=[];
  const setup=await fixture('databases',{Catalog:class {
    constructor(profile){this.profile=profile;created.push(this);}
    load(database,cx){loaded.push([this.profile.id,database,cx]);}
  }}),owner=setup.owner,first={id:'first',revision:2},second={id:'second',revision:3};
  const catalog={profile:first,load:(database,cx)=>loaded.push(['first',database,cx])},logs=[{id:'retained'}],result={},origin={},sql={draft:'SELECT retained'};
  Object.assign(owner,{profiles:[first,second],opened:'first',connected:true,pending:null,picker:true,catalog,logs,selectedLog:'retained',result,origin,status:'db_ready',error:{message:'retained'},epoch:1,page:2,sql});
  await owner.open(first,setup.live);
  assert.equal(owner.catalog,catalog);assert.equal(owner.logs,logs);assert.equal(owner.selectedLog,'retained');assert.equal(owner.result,result);assert.equal(owner.origin,origin);
  assert.equal(owner.status,'db_ready');assert.equal(owner.error.message,'retained');assert.equal(owner.epoch,1);assert.equal(owner.page,2);
  await owner.open(second,setup.live);
  assert.equal(owner.opened,'second');assert.equal(owner.catalog,created[0]);assert.equal(owner.logs.length,0);assert.equal(owner.selectedLog,null);
  for(const key of ['result','origin','status','error'])assert.equal(owner[key],null);
  assert.equal(owner.epoch,2);assert.equal(owner.page,0);assert.equal(owner.sql,sql);assert.equal(owner.picker,false);
  assert.deepEqual(loaded,[['first',null,setup.live],['second',null,setup.live]]);
});

test('opening SSH Run and Transfer clears stale feedback without changing the trust binding',async()=>{
  for(const action of ['run','transfer'])for(const kind of ['host_key_required','completed']) {
    const setup=await fixture('ssh',{createText:value=>({value})}),owner=setup.owner,profile={id:'chosen',revision:4},target={id:'prior',revision:2};
    Object.assign(owner,{profiles:[profile],selected:'prior',connected:true,pending:null,picker:true,pickerProfile:'chosen',pickerRevision:3,dialog:1,
      commandOpen:false,transfer:null,worktrees:[{id:'tree',project:'project'}],error:{message:'prior'},outcome:{kind},target});
    owner.execute=()=>assert.fail('opening a form must not dispatch a request');
    owner.pick({picker:'ssh-picker-3',kind:'select',value:{kind:'action',action}},setup.live);
    assert.equal(owner.outcome,null);assert.equal(owner.error,null);assert.equal(owner.target,target);assert.equal(owner.dialog,2);assert.equal(owner.picker,false);
    if(action==='run')assert.equal(owner.commandOpen,true);
    else {assert.equal(owner.transfer.profile,profile);assert.equal(owner.transfer.worktree,'tree');}
  }
  const setup=await fixture('ssh'),owner=setup.owner,outcome={kind:'host_key_required'};
  Object.assign(owner,{connected:true,pending:{request:{}},picker:true,pickerProfile:'chosen',pickerRevision:3,commandOpen:false,transfer:null,outcome,dialog:1});
  owner.pick({picker:'ssh-picker-3',kind:'select',value:{kind:'action',action:'run'}},setup.live);owner.transferDialog(setup.live);
  assert.equal(owner.outcome,outcome);assert.equal(owner.dialog,1);assert.equal(owner.commandOpen,false);assert.equal(owner.transfer,null);
});

test('SSH Run keeps its selected resource through terminal refresh and captures the live revision',async()=>{
  let selected={id:'chosen',revision:4};const active={id:'active',revision:2},terminal={id:'terminal',ssh:'active',status:{kind:'running'}};
  let profiles=[selected,active];const calls=[];
  const setup=await fixture('ssh',{listSsh:async()=>profiles,listSshTerminals:async id=>id==='active'?[terminal]:[],readText:value=>value}),owner=setup.owner;
  const browser={profile:active};
  Object.assign(owner,{profiles,selected:'chosen',connected:true,pending:null,commandOpen:true,command:'pwd',terminals:[terminal],terminal:'terminal',
    browsers:new Map([['active',browser]]),refreshRevision:0});
  owner.execute=(value,cx,after,id,target)=>calls.push([value,cx,target]);
  await owner.refresh(setup.live);
  assert.equal(owner.selected,'chosen');assert.equal(owner.terminal,'terminal');assert.equal(owner.files(),browser);
  owner.runCommand(setup.event);await Promise.all(setup.tasks);
  assert.deepEqual(JSON.parse(JSON.stringify(calls[0][0])),{kind:'run_ssh',data:{profile:'chosen',expected_revision:4,command:'pwd',timeout_ms:300000}});
  assert.equal(calls[0][1],setup.live);assert.equal(calls[0][2],selected);
  selected={id:'chosen',revision:5};profiles=[selected,active];await owner.refresh(setup.live);
  owner.runCommand(setup.live);await Promise.all(setup.tasks);assert.equal(calls[1][0].data.expected_revision,5);assert.equal(calls[1][2],selected);
  profiles=[active];await owner.refresh(setup.live);owner.runCommand(setup.live);assert.equal(calls.length,2);
  owner.profiles=[selected,active];owner.pending={request:{}};owner.runCommand(setup.live);assert.equal(calls.length,2);
});

test('SSH Transfer worktree events require the live draft generation and unlocked valid choice',async()=>{
  const setup=await fixture('ssh'),owner=setup.owner,transfer={worktree:'first'};
  Object.assign(owner,{transfer,dialog:4,connected:true,pending:null,worktrees:[{id:'first',project:'project'},{id:'second',project:'project'},{id:'unowned',project:null}]});
  owner.control({id:'ssh-worktree-3',value:'second'},setup.live);assert.equal(transfer.worktree,'first');
  owner.control({id:'ssh-worktree-4',value:'unowned'},setup.live);assert.equal(transfer.worktree,'first');
  owner.pending={request:{}};owner.control({id:'ssh-worktree-4',value:'second'},setup.live);assert.equal(transfer.worktree,'first');
  owner.pending=null;owner.control({id:'ssh-worktree-4',value:'second'},setup.live);assert.equal(transfer.worktree,'second');
  const replacement={worktree:'first'};owner.transfer=replacement;owner.dialog=5;
  owner.control({id:'ssh-worktree-4',value:'second'},setup.live);assert.equal(replacement.worktree,'first');
  owner.transfer=null;owner.control({id:'ssh-worktree-5',value:'second'},setup.live);assert.equal(owner.transfer,null);
});
