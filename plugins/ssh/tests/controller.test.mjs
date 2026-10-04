import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

async function fixture() {
  const context=vm.createContext({});
  const opened=[],notices=[],focused=[];
  const profile={id:'saved-host',revision:3,name:'Saved host'};
  const terminal={id:'existing-terminal',ssh:profile.id,status:{kind:'running'}};
  const host={profiles:[profile],terminals:[terminal],inspect:async()=>({Ok:{kind:'terminal_snapshot',data:{info:terminal}}}),browse:async()=>({kind:'directory',path:'.',entries:[],next:null}),upload:async()=>[],download:async()=>[]};
  host.listProfiles=async()=>host.profiles;
  host.listTerminals=async id=>host.terminals.filter(value=>value.ssh===id);
  const requests=new Map(),reads=[],forgotten=[],started=[],texts=[];
  const exports={
    'gpui-kit':{View:class{}},
    sailry:{context:()=>JSON.stringify({locale:'en'}),header_action(){},focusTerminal:id=>focused.push(id)},
    'sailry/sdk':{nextChange(){},readProjectCatalog(){},prepareRequest(value){const id=`request-${reads.length}`;reads.push(value);requests.set(id,value);return id;},completeRequest:id=>host.inspect(requests.get(id)),forgetRequest:id=>forgotten.push(id),faultCode(message){try{return JSON.parse(message).code;}catch(_){return 'failed';}}},
    'sailry/connections':{listSsh:()=>host.listProfiles(),newSshId(){},prepareSsh(){},listSshTerminals:id=>host.listTerminals(id),prepareSshTerminal(){},browseSshDirectory:async input=>{opened.push(input);return host.browse(input);}},
    'sailry/forms':{createText:value=>value,readText:value=>value,releaseText:value=>texts.push(['release',value]),setText:(field,value)=>texts.push(['set',field,value]),nextTextEvent(){}},
    'sailry/credentials':{createSecret(){},releaseSecret(){},describeSecret(){},chooseSecretFile(){}},
    'sailry/ui':{nextTreeEvent(){},nextContextMenuEvent(){},nextNavigationTabEvent(){},nextPickerEvent(){},nextControlEvent(){},modal_closed(){},writeClipboard(){},toast:value=>notices.push(plain(value))},
    'sailry/ssh-transfers':{uploadDropped(){},selectUpload:(...args)=>host.upload(...args),selectDownload:(...args)=>host.download(...args),nextTransfers(){},transferAction:(...args)=>started.push(args)},
    './editor.js':{draft(){},prepare(){}},
    './requests.js':{Request:class{},failure(){}},
    './locales.js':{messages(){},format(){}},
    './view.js':{render(){}},
  };
  const cache=new Map();
  const resolve=async name=>{
    if(cache.has(name))return cache.get(name);
    const module=name==='./files.js'
      ?new vm.SourceTextModule(await readFile(new URL('../dev.sailry.platform/desktop/files.js',import.meta.url),'utf8'),{context})
      :new vm.SyntheticModule(Object.keys(exports[name]),function(){for(const [key,value] of Object.entries(exports[name]))this.setExport(key,value);},{context});
    cache.set(name,module);if(name==='./files.js')await module.link(resolve);return module;
  };
  const module=new vm.SourceTextModule(await readFile(new URL('../dev.sailry.platform/desktop/main.js',import.meta.url),'utf8'),{context});
  await module.link(resolve);await module.evaluate();
  const view=new module.namespace.default();const pending=[];
  view.run=(action,cx)=>pending.push(Promise.resolve().then(()=>action(cx)));
  Object.assign(view,{profiles:[],terminals:[],terminal:null,terminalSelection:0,terminalClose:null,selected:null,browsers:new Map(),pending:null,connected:true,refreshRevision:0,text:{ssh_files_delete_confirm:'Delete files',files_upload:'Upload',files_download:'Download'},dialog:0});
  return {view,profile,terminal,host,opened,reads,forgotten,started,texts,notices,focused,cx:{notify(){}},settle:()=>Promise.all(pending)};
}

async function browsing() {const setup=await fixture();await setup.view.refresh(setup.cx);await setup.settle();return setup;}
const plain=value=>JSON.parse(JSON.stringify(value));
function deferred(){let resolve,reject;const promise=new Promise((accept,fail)=>{resolve=accept;reject=fail;});return {promise,resolve,reject};}

test('coalesces overlapping refreshes and awaits the latest inventory',async()=>{
  const {view,cx,host}=await fixture(),first=deferred(),latest=deferred();let reads=0;
  host.listProfiles=()=>++reads===1?first.promise:latest.promise;
  const initial=view.refresh(cx),queued=Array.from({length:8},()=>view.refresh(cx));
  assert.equal(reads,1,'entry and subscription changes do not overlap reads');
  first.resolve([]);
  for(let step=0;step<20&&reads<2;step++)await Promise.resolve();
  assert.equal(reads,2,'one follow-up absorbs all queued changes');
  latest.resolve([]);await Promise.all([initial,...queued]);
  assert.equal(reads,2);assert.equal(view.refreshing,null);
  assert.deepEqual(Array.from(view.profiles),[]);
});

test('refresh failures remain visible and allow a later read',async()=>{
  const {view,cx,host}=await fixture(),first=deferred();let reads=0;
  host.listProfiles=()=>++reads===1?first.promise:Promise.resolve([]);
  const initial=view.refresh(cx),queued=view.refresh(cx);
  const results=Promise.allSettled([initial,queued]);
  first.reject(new Error('inventory unavailable'));
  for(const result of await results){assert.equal(result.status,'rejected');assert.match(result.reason.message,/inventory unavailable/);}
  assert.equal(view.refreshing,null);
  await view.refresh(cx);assert.equal(reads,2);
});

test('restoring multiple profiles stays within the host request capacity',async()=>{
  const {view,cx,host,terminal,settle}=await fixture();let active=0,maximum=0,reads=0;
  host.profiles=Array.from({length:6},(_,index)=>({id:`host-${index}`,revision:1,name:`Host ${index}`}));
  host.terminals=host.profiles.map(profile=>({...terminal,id:`terminal-${profile.id}`,ssh:profile.id}));
  view.terminals=[...host.terminals];
  host.listTerminals=async id=>{
    reads++;active++;maximum=Math.max(maximum,active);
    try{
      if(active>4)throw new Error('plugin request capacity exhausted');
      await Promise.resolve();return host.terminals.filter(info=>info.ssh===id);
    }finally{active--;}
  };
  await view.refresh(cx);await settle();
  assert.equal(reads,6);assert.equal(view.terminals.length,6);assert.ok(maximum<=4);
});

test('transfer outcomes use one toast and cancellation remains neutral',async()=>{
  const {view,cx,notices}=await fixture();view.jobs=[];
  Object.assign(view.text,{ssh_cancelled:'Cancelled',ssh_transfer_done:'Transferred',ssh_files_uncertain:'Result unconfirmed',ssh_files_action_failed:'File action failed'});
  view.transfers({transfers:[{id:'prior',stage:'done'}]},cx);assert.equal(notices.length,0);
  view.transfers({transfers:[{id:'prior',stage:'done'},{id:'job',stage:'transferring'}]},cx);
  for(const [stage,kind,message]of[['uncertain','error','Result unconfirmed'],['done','info','Transferred'],['cancelled','info','Cancelled']]){
    const value={transfers:[{id:'job',stage}]};view.transfers(value,cx);view.transfers(value,cx);
    assert.deepEqual(notices.at(-1),{id:'ssh-transfer-job',kind,message});
  }
  assert.equal(notices.length,3);assert.equal(view.jobs[0].stage,'cancelled');
});

test('restores existing terminal and its file browser without reopening a terminal',async()=>{
  const setup=await fixture();await setup.view.refresh(setup.cx);await setup.settle();
  assert.equal(setup.view.terminal,setup.terminal.id);
  assert.equal(setup.view.selected,setup.profile.id);
  assert.equal(setup.view.files().profile,setup.profile);
  assert.deepEqual(setup.opened.map(value=>[value.profile,value.expected_revision,value.path]),[[setup.profile.id,3,'.']]);
  await setup.view.refresh(setup.cx);await setup.settle();assert.equal(setup.opened.length,1);
});

test('dragging selected files moves the captured selection and preserves clipboard',async()=>{
  const {view,cx}=await fixture();const calls=[];
  const clipboard={paths:['/remote/copied'],cut:false};
  const files={path:'/remote',selected:['/remote/a','/remote/b'],clipboard,entry:path=>path==='/remote/target'?{kind:'directory'}:null};
  view.files=()=>files;view.mutate=async (_scope,actions)=>calls.push(actions);
  await view.tree({tree:'ssh-files',kind:'drop',source:'/remote/a',id:'/remote/target'},cx);
  assert.deepEqual(JSON.parse(JSON.stringify(calls)),[[
    {path:'/remote/a',action:{operation:'move',destination:'/remote/target/a'}},
    {path:'/remote/b',action:{operation:'move',destination:'/remote/target/b'}},
  ]]);
  assert.equal(files.clipboard,clipboard);
  files.selected=['/remote/a','/remote/target'];
  await view.tree({tree:'ssh-files',kind:'drop',source:'/remote/a',id:'/remote/target'},cx);
  assert.equal(calls.length,1,'a selection containing the destination is refused as a unit');
});

test('native additive and range modifiers retain the selected remote paths',async()=>{
  const setup=await fixture();await setup.view.refresh(setup.cx);await setup.settle();
  const files=setup.view.files();files.path='/remote';files.entries=['a','b','c'].map(name=>({name,kind:'file'}));
  files.select('/remote/a',{});files.select('/remote/c',{additive:true});
  assert.deepEqual(Array.from(files.selected),['/remote/a','/remote/c']);
  files.select('/remote/b',{shift:true});assert.deepEqual(Array.from(files.selected),['/remote/b','/remote/c']);
  files.select('/remote/c',{additive:true});assert.deepEqual(Array.from(files.selected),['/remote/b']);
});

test('delete confirmation refuses a changed browser or captured revision',async()=>{
  for(const change of ['browser','directory','profile','closed']){
    const {view,cx,profile,settle}=await browsing(),files=view.files(),commands=[];
    files.selected=['/remote/a'];view.execute=async value=>commands.push(value);
    await view.fileAction(view.fileId('delete','/remote/a'),cx);
    if(change==='browser')view.files=()=>({...files});
    if(change==='directory')files.revision++;
    if(change==='profile')view.profiles=[{...profile,revision:profile.revision+1}];
    if(change==='closed')files.close();
    view.confirm(cx);await settle();
    assert.equal(commands.length,0,change);
  }
});

test('delete confirmation retains original profile and selected paths',async()=>{
  const {view,cx,profile,settle}=await browsing(),files=view.files(),commands=[];
  files.selected=['/remote/a','/remote/b'];
  view.execute=async(value,_cx,_after,_id,target)=>{commands.push({value,target});return {kind:'ssh_outcome',data:{kind:'files_changed'}};};
  await view.fileAction(view.fileId('delete','/remote/a'),cx);
  files.selected.push('/remote/later');view.confirm(cx);
  await settle();
  assert.deepEqual(plain(commands),['/remote/a','/remote/b'].map(path=>({value:{kind:'modify_ssh_file',data:{profile:profile.id,expected_revision:profile.revision,path,action:{operation:'remove'}}},target:profile})));
});

test('multi-file mutation stops after awaited scope changes',async()=>{
  for(const change of ['browser','directory','profile']){
    const {view,cx,profile,opened}=await browsing(),files=view.files(),wait=deferred(),commands=[];
    view.execute=async(value,_cx,_after,_id,target)=>{commands.push({value,target});return wait.promise;};
    const result=view.mutate(view.fileScope(),['/remote/a','/remote/b'].map(path=>({path,action:{operation:'remove'}})),cx);
    assert.equal(commands.length,1);
    if(change==='browser')view.files=()=>({...files});
    if(change==='directory')files.revision++;
    if(change==='profile')view.profiles=[{...profile,revision:profile.revision+1}];
    wait.resolve({kind:'ssh_outcome',data:{kind:'files_changed'}});
    assert.equal(await result,false,change);
    assert.equal(commands.length,1,change);
    assert.equal(opened.length,1,'a stale browser is not reloaded');
    assert.equal(commands[0].target,profile);
    assert.equal(commands[0].value.data.profile,profile.id);
  }
});

test('queued file edits refuse a browser switch before dispatch',async()=>{
  const {view,cx,settle}=await browsing(),files=view.files(),commands=[];
  await view.fileAction(view.fileId('new_file',''),cx);view.fileEdit.input='created.txt';
  view.execute=async value=>commands.push(value);view.submitFile(cx);
  view.files=()=>({...files});await settle();assert.equal(commands.length,0);
});

test('native transfer pickers do not start jobs after scope changes',async()=>{
  for(const action of ['upload','download']){
    for(const change of ['browser','profile','closed']){
    const {view,cx,host,profile,started}=await browsing(),wait=deferred(),files=view.files();
    host[action]=async target=>{assert.equal(target,profile);return wait.promise;};
    const pending=view.fileAction(view.fileId(action,'/remote/a'),cx);
    if(change==='browser')view.files=()=>({...files});
    if(change==='profile')view.profiles=[{...profile,revision:profile.revision+1}];
    if(change==='closed')files.close();
    wait.resolve(['prepared-job']);await pending;
    assert.equal(started.length,0,`${action}: ${change}`);
    }
  }
});

test('native transfer pickers retain captured targets across listing refreshes',async()=>{
  for(const action of ['upload','download']){
    const {view,cx,host,profile,started}=await browsing(),wait=deferred(),files=view.files(),chosen=[];
    files.path='/remote';files.entries=[{name:'a',kind:'file'}];
    host[action]=async(target,paths)=>{chosen.push({target,paths});return wait.promise;};
    const pending=view.fileAction(view.fileId(action,'/remote/a'),cx);
    await files.load('/elsewhere',cx);
    wait.resolve(['prepared-job']);await pending;
    assert.deepEqual(plain(chosen),[{target:profile,paths:action==='upload'?'/remote':[{path:'/remote/a',directory:false}]}]);
    assert.deepEqual(started,[['prepared-job','start']],action);
  }
});

test('retains a captured running terminal after its profile is removed',async()=>{
  const {view,cx,host,terminal,reads,forgotten}=await browsing(),files=view.files();
  host.profiles=[];host.terminals=[];await view.refresh(cx);
  assert.equal(view.terminal,terminal.id);assert.equal(view.terminals[0],terminal);
  assert.equal(view.files(),null);assert.equal(files.closed,true);
  assert.deepEqual(plain(reads),[{kind:'inspect_terminal',data:{terminal:terminal.id}}]);
  assert.equal(forgotten.length,1);
});

test('retains captured exited output without discovering unowned terminals',async()=>{
  const {view,cx,host,terminal,reads}=await browsing();
  const exited={...terminal,owner:null,status:{kind:'exited',code:7}};
  host.terminals=[];host.inspect=async()=>({Ok:{kind:'terminal_snapshot',data:{info:exited}}});
  await view.refresh(cx);assert.equal(view.terminal,terminal.id);assert.equal(view.terminals[0],exited);
  assert.deepEqual(reads.map(value=>value.data.terminal),[terminal.id]);
  const other=await fixture();other.host.terminals=[];other.host.inspect=host.inspect;
  await other.view.refresh(other.cx);assert.equal(other.view.terminals.length,0);assert.equal(other.reads.length,0);
});

test('keeps captured terminals on transient read failure until closed or not found',async()=>{
  for(const [result,retained] of [[{Err:{code:'unavailable'}},true],[new Error('disconnected'),true],[new Error(JSON.stringify({code:'unavailable',message:'disconnected'})),true],[{Ok:{kind:'terminal_snapshot',data:{info:{id:'existing-terminal',ssh:'saved-host',status:{kind:'closed'}}}}},false],[{Err:{code:'not_found'}},false],[new Error(JSON.stringify({code:'not_found',message:'terminal closed'})),false]]){
    const {view,cx,host,terminal,forgotten}=await browsing();host.terminals=[];
    host.inspect=async()=>{if(result instanceof Error)throw result;return result;};
    await view.refresh(cx);
    assert.equal(view.terminal,retained?terminal.id:null);
    assert.equal(forgotten.length,1);
  }
});

test('opening a running connection preserves captured exited tabs',async()=>{
  const {view,cx,host,terminal,profile}=await browsing(),exited={...terminal,status:{kind:'exited',code:0}},running={...terminal,id:'second-terminal'};
  view.terminals=[exited];host.terminals=[running];await view.open(profile,cx);
  assert.deepEqual(Array.from(view.terminals,info=>info.id),[exited.id,running.id]);
});

test('closing a browser suppresses its late directory result',async()=>{
  const {view,cx,host,texts}=await browsing(),files=view.files(),wait=deferred();
  texts.length=0;host.browse=async()=>wait.promise;const pending=files.load('/late',cx);files.close();
  wait.resolve({kind:'directory',path:'/late',entries:[],next:null});await pending;
  assert.deepEqual(texts,[['release',files.input]]);assert.equal(files.path,'.');
});

test('active terminal closes focus each captured successor without replay',async()=>{
  const {view,cx,host,terminal,focused}=await browsing();
  const second={...terminal,id:'second-terminal'},third={...terminal,id:'third-terminal'};
  host.terminals=[terminal,second,third];await view.refresh(cx);
  assert.equal(focused.length,0,'background observations never take focus');
  view.selectTerminal(second.id,cx,true);assert.deepEqual(focused,[second.id]);
  const closed=[];
  view.execute=async value=>{
    closed.push(value.data.terminal);host.terminals=host.terminals.filter(info=>info.id!==value.data.terminal);
    view.terminals=view.terminals.map(info=>info.id===value.data.terminal?{...info,status:{kind:'closed'}}:info);
    host.inspect=async value=>({Ok:{kind:'terminal_snapshot',data:{info:view.terminals.find(info=>info.id===value.data.terminal)}}});
    await view.refresh(cx);return {kind:'terminal'};
  };
  await view.closeTerminal(second.id,cx);await view.closeTerminal(third.id,cx);await view.closeTerminal(terminal.id,cx);
  assert.deepEqual(focused,[second.id,third.id,terminal.id]);
  assert.deepEqual(closed,[second.id,third.id,terminal.id]);assert.equal(view.terminal,null);
});

test('background, rejected and superseded terminal closes do not steal focus',async()=>{
  const {view,cx,host,terminal,focused}=await browsing(),second={...terminal,id:'second-terminal'};
  host.terminals=[terminal,second];await view.refresh(cx);
  view.execute=async()=>null;await view.closeTerminal(terminal.id,cx);assert.equal(focused.length,0);
  const pending=deferred();view.execute=()=>pending.promise;
  const closing=view.closeTerminal(terminal.id,cx);view.selectTerminal(second.id,cx,true);
  pending.resolve({kind:'terminal'});await closing;assert.deepEqual(focused,[second.id]);
  view.execute=async()=>({kind:'terminal'});await view.closeTerminal(terminal.id,cx);
  assert.deepEqual(focused,[second.id]);
  view.pending={};view.control({id:'ssh-close-current'},cx);assert.deepEqual(focused,[second.id]);
});

test('accepted closes focus after the newest overlapping terminal observation',async()=>{
  for(const superseded of [false,true]){
  const {view,cx,host,terminal,profile,focused}=await browsing();
  const second={...terminal,id:'second-terminal'},third={...terminal,id:'third-terminal'};
  host.terminals=[terminal,second,third];await view.refresh(cx);
  view.selectTerminal(second.id,cx,true);focused.length=0;
  host.terminals=[terminal,third];
  host.inspect=async()=>({Ok:{kind:'terminal_snapshot',data:{info:{...second,status:{kind:'closed'}}}}});
  const older=deferred(),newer=deferred();let reads=0;
  Object.defineProperty(host,'profiles',{get:()=>++reads===1?older.promise:newer.promise});
  // Use the real completion reducer; only durable admission is isolated here.
  view.execute=async()=>{
    view.pending={request:{done:true,release(){},output:{kind:'terminal'}}};
    return view.finish(cx);
  };
  const closing=view.closeTerminal(second.id,cx);
  const observation=view.refresh(cx);
  older.resolve([profile]);
  for(let step=0;step<20&&reads<2;step++)await Promise.resolve();
  assert.equal(reads,2,'the latest observation follows the older read without overlapping it');
  assert.deepEqual(focused,[],'the closed content is not focused before its successor is known');
  if(superseded)view.selectTerminal(terminal.id,cx,true);
  newer.resolve([profile]);await Promise.all([closing,observation]);
  assert.equal(view.terminal,superseded?terminal.id:third.id);
  assert.deepEqual(focused,[superseded?terminal.id:third.id]);
  assert.equal(view.terminalClose,null);
  assert.equal(reads,2,'only the two original reads run; the mutation is not replayed');
  }
});

test('the close shortcut captures its terminal before dispatch',async()=>{
  const {view,cx,host,terminal,settle}=await browsing();
  const second={...terminal,id:'second-terminal'},closed=[];
  host.terminals=[terminal,second];await view.refresh(cx);
  view.closeTerminal=async id=>closed.push(id);
  view.control({id:'ssh-close-current'},cx);
  view.selectTerminal(second.id,cx,true);
  await settle();
  assert.deepEqual(closed,[terminal.id]);
  assert.equal(view.terminal,second.id);
});
