import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {Repository} from '../dev.sailry.platform/desktop/repository.js';
import {Menus} from '../dev.sailry.platform/desktop/menus.js';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/main.js', import.meta.url), 'utf8'))
  .replace(/^import[\s\S]*?;\n/gm, '').replace('export default class', 'class');
const contextSource = (await readFile(new URL('../dev.sailry.platform/desktop/context.js', import.meta.url), 'utf8'))
  .replace(/^import[\s\S]*?;\n/gm, '').replace('export default class', 'class');
const deferred = () => { let resolve,reject; const promise=new Promise((done,fail)=>{resolve=done;reject=fail;}); return {promise,resolve,reject}; };
const settle = async () => { for (let index=0;index<12;index++) await Promise.resolve(); };

function fixture(discovery) {
  const changes = [deferred(),deferred(),deferred()]; let reads=0, queries=0, cursor=0;
  const shortcuts=[];
  const globals = {View:class {}, EditorState:text=>({text}), context:()=>'{"locale":"en"}',
    messages:()=>({}), workspaceMode:()=> 'main', createText:()=> 'message', Repository,
    Actions:class {}, Menus:class {}, readResourceTarget:()=>null,
    nextWindowActivation:()=>new Promise(()=>{}),registerShortcuts:bindings=>{shortcuts.push(bindings);return 'FixtureGitKeys';},
    canInvokeContribution:()=>{queries++;return discovery.promise;},
    sdk:{nextChange:()=>changes[cursor++].promise}};
  for (const name of ['nextMenuEvent','nextContextMenuEvent','nextControlEvent','nextTreeEvent',
    'nextNavigationTabEvent','nextPickerEvent','modal_closed','nextTextEvent']) globals[name]=()=>{};
  const Git=vm.runInNewContext(`${source}\nGit`,globals);
  const owner=new Git(); owner.watch=()=>{}; owner.invalidate=async()=>{reads++;};
  const tasks=[];let notifications=0;
  const live={notify:()=>{notifications++;},spawn:task=>{const pending=task(live);tasks.push(pending);return pending;}};
  const cx={notify:()=>{},spawn:live.spawn,focus_handle:()=>({focus(){}})};
  owner.init({},cx);
  return {owner,changes,tasks,cx,live,shortcuts,notifications:()=>notifications,reads:()=>reads,queries:()=>queries};
}

test('renderer owns scoped shortcuts including native text input submission',()=>{
  const state=fixture(deferred());
  assert.equal(state.shortcuts.length,1);
  const bindings=state.shortcuts[0];
  assert.equal(bindings.length,8);
  assert.deepEqual(Array.from(bindings.filter(binding=>binding.context==='Input'),binding=>[binding.keystroke,binding.action]),[
    ['secondary-enter','git-commit-action'],['secondary-shift-enter','git-amend-action'],
  ]);
  assert.equal(state.owner.shortcutContext,'FixtureGitKeys');
  state.owner.release();
});

function contributions() {
  const change=deferred(),event=deferred(),reads=[],branches=[],choices=[],published=[];let changes=0,events=0;
  const never=()=>new Promise(()=>{});
  const globals={View:class {},EditorState:text=>({text}),context:()=>'{"locale":"en"}',
    messages:()=>({}),workspaceMode:()=> 'main',createText:()=> 'message',Repository,Menus,
    Actions:class {choose(title,items){choices.push({title,items});}},registerShortcuts:()=> 'FixtureGitKeys',
    readScope:()=>({worktree:'captured'}),readLocation:()=>({surface:'project',worktree:'captured',branch:'main',main:true,git:true,can_move:true}),
    nextLocationChange:never,nextWindowActivation:never,readResourceTarget:()=>null,canInvokeContribution:async()=>true,
    nextContributionEvent:()=>events++===0 ? event.promise : never(),publishContributions:states=>published.push(states),toast:()=>{},
    sdk:{nextChange:()=>changes++===0 ? change.promise : never(),
      inspectGit:()=>{const pending=deferred();reads.push(pending);return pending.promise;},
      listGitBranches:()=>{const pending=deferred();branches.push(pending);return pending.promise;}}};
  for (const name of ['nextMenuEvent','nextContextMenuEvent','nextControlEvent','nextTreeEvent',
    'nextNavigationTabEvent','nextPickerEvent','modal_closed','nextTextEvent']) globals[name]=never;
  const Context=vm.runInNewContext(`${source}\n${contextSource}\nContext`,globals);
  const owner=new Context();owner.watch=()=>{};
  const live={notify:()=>{},spawn:task=>task(live),focus_handle:()=>({focus(){}})};owner.init({},live);
  return {owner,change,event,reads,branches,choices,published};
}

test('branch contributions survive concurrent repository observation',async()=>{
  for (const eventFirst of [false,true]) {
    const state=contributions();
    const invoke=()=>state.event.resolve({id:'manage',handler:'manage',sequence:1,value:null});
    const invalidate=()=>state.change.resolve({cursor:'created-worktree',connected:true});
    (eventFirst ? invoke : invalidate)();await settle();
    state.reads[0].resolve({branch:'stale',kind:'ready',entries:[]});await settle();
    assert.equal(state.branches.length,1);
    (eventFirst ? invalidate : invoke)();await settle();
    assert.equal(state.reads.length,2);
    state.branches[0].resolve({entries:[{name:'stale',commit:'abc1234'}],sync:{tracking:{}}});await settle();
    assert.equal(state.choices.length,0);
    state.reads[1].resolve({branch:'latest',kind:'ready',entries:[]});await settle();
    state.branches[1].resolve({entries:[{name:'managed',commit:'abc1234'}],sync:{tracking:{}}});await settle();
    assert.equal(state.choices.length,1);assert.equal(state.owner.repo.status.branch,'latest');
    assert(state.choices[0].items.some(item=>item.label==='managed'));
    assert(!state.choices[0].items.some(item=>item.label==='stale'));
    assert(state.published.some(states=>states.some(item=>item.id==='manage' && item.reply_to===1)));
    state.owner.release();
  }
});

test('released or failed observation cannot open a contribution picker',async()=>{
  for (const released of [false,true]) {
    const state=contributions();state.event.resolve({id:'manage',handler:'manage',sequence:1,value:null});await settle();
    state.change.resolve({cursor:'created-worktree',connected:true});await settle();
    state.reads[0].resolve({branch:'stale',kind:'ready',entries:[]});await settle();
    state.branches[0].resolve({entries:[],sync:{tracking:{}}});await settle();
    if (released) {
      state.owner.release();state.reads[1].resolve({branch:'latest',kind:'ready',entries:[]});await settle();
      state.branches[1].resolve({entries:[],sync:{tracking:{}}});
    }
    else state.reads[1].reject(Error('repository unavailable'));
    await settle();assert.equal(state.choices.length,0);state.owner.release();
  }
});

test('repository observation continues while optional worktree discovery is pending',async()=>{
  const discovery=deferred();const state=fixture(discovery);
  state.changes[0].resolve({cursor:'1',connected:true});await settle();
  assert.equal(state.reads(),1);assert.equal(state.queries(),1);
  state.changes[1].resolve({cursor:'2',connected:true});await settle();
  assert.equal(state.reads(),2);assert.equal(state.queries(),1);
  discovery.reject(Error('unavailable contribution'));await settle();
  assert.equal(state.owner.worktrees,false);assert.equal(state.owner.worktreesPending,false);
  state.changes[2].reject(Error('closed'));await settle();
});

test('released controllers ignore optional capability discovery results',async()=>{
  const discovery=deferred();const state=fixture(discovery);
  state.changes[0].resolve({cursor:'1',connected:true});await settle();
  state.owner.release();discovery.resolve(true);await settle();
  assert.equal(state.owner.worktrees,false);
  state.changes[1].reject(Error('closed'));await settle();
});

test('composer location exists before eager repository observation starts',async()=>{
  const contextSource = (await readFile(new URL('../dev.sailry.platform/desktop/context.js', import.meta.url), 'utf8'))
    .replace(/^import[\s\S]*?;\n/gm, '').replace('export default class', 'class');
  let reads = 0;
  const location = {surface:'composer',worktree:'captured-checkout',branch:'main',main:true,git:true,can_move:true};
  const globals = {
    Git:class {
      init() {
        this.text = {};
        this.repo = {refresh:async()=>{reads++;return false;}};
        this.invalidate();
      }
    },
    readLocation:()=>location,
    readScope:()=>({worktree:'captured-checkout'}),
    publishContributions:()=>{},
  };
  const Context=vm.runInNewContext(`${contextSource}\nContext`,globals);
  const owner=new Context();
  owner.init({}, {spawn:()=>{},notify:()=>{},focus_handle:()=>({focus(){}})});
  await settle();
  assert.equal(owner.location,location);
  assert.equal(reads,1);
});

test('unassigned controllers do not read or publish a repository contribution',async()=>{
  const contextSource = (await readFile(new URL('../dev.sailry.platform/desktop/context.js', import.meta.url), 'utf8'))
    .replace(/^import[\s\S]*?;\n/gm, '').replace('export default class', 'class');
  let reads = 0, published;
  const globals = {
    Git:class {
      init() {
        this.text = {};
        this.repo = {refresh:async()=>{reads++;return false;}};
        this.invalidate();
      }
    },
    readScope:()=>({worktree:null}),
    // A later presentation update cannot grant a retained controller a checkout.
    readLocation:()=>({surface:'composer',worktree:'new-checkout',branch:'main',main:true,git:true,can_move:true}),
    publishContributions:states=>{published=states;},
  };
  const Context=vm.runInNewContext(`${contextSource}\nContext`,globals);
  const owner=new Context();
  owner.init({}, {spawn:()=>{},notify:()=>{},focus_handle:()=>({focus(){}})});
  await owner.invalidate();
  await settle();
  assert.equal(reads,0);
  assert.deepEqual(Array.from(published,state=>state.visible),[false,false,false]);
});

test('composer controllers skip repository reads until location metadata confirms Git',async()=>{
  for (const project of [null,'plain-directory']) {
    let reads=0,published;
    const updates=[deferred(),deferred()]; let cursor=0;
    const location={cursor:'0',surface:'composer',project,worktree:'captured',main:true,git:false,can_move:true};
    const globals={Git:class {init(){this.text={};this.repo={refresh:async()=>{reads++;return false;}};this.invalidate();}},
      readScope:()=>({worktree:'captured'}),readLocation:()=>location,
      nextLocationChange:()=>updates[cursor++].promise,nextContributionEvent:()=>new Promise(()=>{}),
      publishContributions:states=>{published=states;}};
    const Context=vm.runInNewContext(`${contextSource}\nContext`,globals),owner=new Context();
    const live={notify:()=>{},spawn:task=>task(live)};
    owner.init({},live);await owner.invalidate();await settle();
    assert.equal(reads,0);assert.deepEqual(Array.from(published,state=>state.visible),[false,false,false]);
    updates[0].resolve({...location,cursor:'1',project:'initialized',git:true});await settle();
    assert.equal(reads,1,'a newly detected repository starts normal observation');
    owner.counts={added:1,removed:2};
    updates[1].resolve({...location,cursor:'2'});await settle();
    assert.equal(reads,1,'returning to a non-Git location does not read Git');
    assert.equal(owner.counts,null);assert.deepEqual(Array.from(published,state=>state.visible),[false,false,false]);
  }
});

test('hidden repository controls omit initial path labels',()=>{
  const path = `/isolated/${'workspace/'.repeat(24)}session`;
  const location = {surface:'composer',project:null,worktree:'captured-checkout',branch:path,
    main:true,git:false,can_move:true};
  let published;
  const globals = {
    Git:class {init() {this.text = {git_detached:'Detached HEAD'};}},
    readScope:()=>({worktree:location.worktree}),
    readLocation:()=>location,
    publishContributions:states=>{
      for (const state of states) if (state.label) {
        assert(Buffer.byteLength(state.label.label) <= 128);
      }
      published=states;
    },
  };
  const Context=vm.runInNewContext(`${contextSource}\nContext`,globals);
  const owner=new Context();
  owner.init({}, {spawn:()=>{},notify:()=>{},focus_handle:()=>({focus(){}})});
  const branch = published.find(state=>state.id === 'branch');
  assert.equal(branch.visible,false);
  assert.equal(branch.label,undefined);
  assert.equal(owner.location.branch,path);
  assert.equal(owner.worktree,location.worktree);

  owner.location = {...location,project:'project',git:true,branch:'feature/visible-label'};
  owner.publish();
  const visible = published.find(state=>state.id === 'branch');
  assert.equal(visible.visible,true);
  assert.equal(visible.label.label,'feature/visible-label');
});

test('repository notifications use a spawned context after initialization returns',async()=>{
  const discovery=deferred();
  const state=fixture(discovery);
  state.cx.notify=()=>{throw Error('initialization context expired');};
  state.owner.repo.notify();
  assert.equal(state.notifications(),1);
  state.changes[0].reject(Error('closed'));await settle();
});

test('event streams and actions receive the spawned context',async()=>{
  const state=fixture(deferred());let received,resolve;
  delete state.owner.watch;
  const ready=new Promise(done=>{resolve=done;});let reads=0;
  state.owner.watch(()=>reads++===0?ready:new Promise(()=>{}),(event,cx)=>{
    received=cx;
    state.owner.run(async cx=>{await Promise.resolve();cx.notify();},cx);
  },state.cx);
  state.cx.notify=()=>{throw Error('initialization context expired');};
  resolve({id:'git-refresh'});await settle();
  assert.equal(received,state.live);assert.equal(state.notifications(),3);
  state.changes[0].reject(Error('closed'));await settle();
});

test('history controls open the captured commit without shadowing the commit action',async()=>{
  const state=fixture(deferred()),commit='b'.repeat(40),requests=[];
  let commits=0;
  state.owner.actions.invoke=value=>{requests.push(value);};
  state.owner.actions.commit=()=>{commits++;};
  state.owner.control(`git-commit-${commit}`,state.live);
  await settle();
  assert.equal(JSON.stringify(requests),JSON.stringify([{kind:'open',request:{kind:'commit',commit}}]));
  state.owner.control('git-commit-action',state.live);
  await settle();
  assert.equal(commits,1);assert.equal(requests.length,1);
  state.changes[0].reject(Error('closed'));await settle();
});

test('native branch controls open the existing captured chooser only when loaded',async()=>{
  const state=fixture(deferred()),choices=[],items=[{id:'main'}];
  state.owner.actions.choose=(title,value)=>choices.push({title,value});
  state.owner.menus.checkout=()=>items;
  state.owner.repo.branches=null;
  state.owner.control('git-branch-menu',state.live);
  assert.equal(choices.length,0);
  state.owner.repo.branches={};
  state.owner.control('git-branch-menu',state.live);
  assert.deepEqual(choices,[{title:'git_checkout',value:items}]);
  state.changes[0].reject(Error('closed'));await settle();
});

test('closing the selected document focuses the actual successor surface',()=>{
  const surfaces=[],fallback=[],choices=new Map();
  const Git=vm.runInNewContext(`${source}\nGit`,{View:class {},focusSurface:id=>surfaces.push(id),
    collapseOpen:id=>choices.get(id) ?? null,views:{defaultOpen:true}});
  const owner=new Git();
  owner.repo=new Repository({},()=>{});owner.contentFocus={focus:()=>fallback.push(true)};
  const documents=[
    {id:'first',request:{kind:'file'},files:[{path:'first.txt',text:'diff'}]},
    {id:'output',request:{kind:'output'},output:'captured output'},
    {id:'combined',request:{kind:'changes'},files:[{path:'hidden.txt',text:'diff'},{path:'visible.txt',text:'diff'}]},
  ];
  owner.repo.tabs=documents.map(document=>document.id);
  owner.repo.documents=new Map(documents.map(document=>[document.id,document]));
  owner.repo.selected='first';choices.set('diff-file-combined/hidden.txt',false);
  owner.closeDocument('first');assert.equal(owner.repo.selected,'output');
  owner.closeDocument('output');assert.equal(owner.repo.selected,'combined');
  assert.deepEqual(surfaces,['git-output-output','git-diff-combined/visible.txt']);
  choices.set('diff-file-combined/visible.txt',false);owner.focusDocument();
  assert.equal(fallback.length,1,'an empty or collapsed document keeps its content keyboard scope');
  owner.closeDocument('combined');assert.equal(owner.repo.selected,null);
  assert.equal(surfaces.length,2);assert.equal(fallback.length,1,'the last close does not steal focus');
});

test('binary successors keep the content keyboard scope without requesting a diff surface',()=>{
  const surfaces=[],fallback=[];
  const Git=vm.runInNewContext(`${source}\nGit`,{View:class {},focusSurface:id=>surfaces.push(id),
    collapseOpen:()=>null,views:{defaultOpen:true}});
  const owner=new Git();owner.contentFocus={focus:()=>fallback.push(true)};
  owner.repo={selected:'binary',documents:new Map([
    ['binary',{id:'binary',request:{kind:'file'},files:[{path:'image.png',text:'raw metadata',binary:true}]}],
    ['combined',{id:'combined',request:{kind:'changes'},files:[{path:'image.png',text:'raw metadata',binary:true}]}],
  ])};
  owner.focusDocument();owner.repo.selected='combined';owner.focusDocument();
  assert.equal(surfaces.length,0);assert.equal(fallback.length,2);
});

test('closing a background document does not change content focus',()=>{
  const Git=vm.runInNewContext(`${source}\nGit`,{View:class {},focusSurface:()=>assert.fail('background close must not focus')});
  const owner=new Git();owner.repo=new Repository({},()=>{});
  owner.repo.tabs=['first','second'];owner.repo.documents=new Map(owner.repo.tabs.map(id=>[id,{id,request:{kind:'file'}}]));
  owner.repo.selected='second';owner.closeDocument('first');assert.equal(owner.repo.selected,'second');
});
