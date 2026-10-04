import test from 'node:test';
import assert from 'node:assert/strict';
import {Controller} from '../dev.sailry.platform/desktop/controller.js';
import {listing} from '../dev.sailry.platform/desktop/policy.js';

function fixture(surface = 'project') {
  let location = {cursor:'1',surface,node:'node-a',project:'project-a',worktree:'tree-a',session:surface === 'composer' ? 'session-a' : null,can_move:true,main:true,project_name:'Project',branch:'main',git:true};
  const state = {prepared:[],completed:[],forgotten:[],selected:[],released:[],toasts:[],resolution:'head-a',reads:0,result:null,drafts:false};
  const api = {
    readLocation:() => location,
    selectLocation:async (cursor,value) => {if (value.request) assert.ok(!state.forgotten.includes(value.request));state.selected.push({cursor,value});return true;},
    readWorktreeCatalog:async () => ({projects:[{id:'project-a',name:'Project'}],worktrees:[{id:'tree-a',path:'/main',main:true}]}),
    listWorktrees:async () => ({kind:'repository',entries:[{path:'/main',branch:'main',head:'head-a',main:true,locked:false,available:true},{path:'/native',branch:'topic',head:'head-b',main:false,locked:false,available:true}],truncated:false,omitted_paths:'0'}),
    inspectGit:async () => ({head:'head-a',branch:'main',index_revision:'index-a'}),
    resolveGitRevision:async () => {state.reads++;return {commit:state.resolution};},
    prepareWorktreeChange:value => {const id = `request-${state.prepared.length}`;state.prepared.push({id,value});return id;},
    prepareGitChange:value => api.prepareWorktreeChange(value),
    completeRequest:async id => {state.completed.push(id);if (state.result instanceof Error) throw state.result;if (typeof state.result === 'function') return state.result(id);return state.result ?? {Ok:{kind:'worktree',data:{id:'tree-new'}}};},
    forgetRequest:id => state.forgotten.push(id),
    worktreeHasDrafts:async (_id,request) => {if (request) assert.ok(!state.forgotten.includes(request));return state.drafts;},
    releaseWorktree:async (id,request) => {assert.ok(!state.forgotten.includes(request));state.released.push({id,request});},
    toast:value => state.toasts.push(value),newId:() => '12345678-abcd',
  };
  const model = new Controller(api,key => key,() => {});
  return {model,state,api,change:patch => {location = {...location,...patch};model.accept(location);}};
}
const inputs = {revision:'release',branch:'topic/new',path:'/checkouts/new'};

test('manual create pins resolution and ID for unchanged retries', async () => {
  const {model,state} = fixture();
  model.create({branch:'main',head:'head-a'});
  state.result = {Err:{code:'invalid_request'}};
  await model.submit(inputs);
  state.resolution = 'head-moved';
  await model.submit(inputs);
  assert.equal(state.reads,1);
  assert.deepEqual(state.completed,['request-0','request-0']);
  assert.equal(state.prepared[0].value.commit,'head-a');
  state.result = null;
  await model.submit({...inputs,branch:'other'});
  assert.equal(state.reads,2);
  assert.equal(state.prepared[1].value.commit,'head-moved');
  assert.deepEqual(state.selected,[{cursor:'1',value:{worktree:'tree-new',fork:false,request:'request-1'}}]);
  assert.equal(model.dialog,null);
});

test('unconfirmed create retains original immutable request', async () => {
  const {model,state} = fixture();
  model.create({branch:'main',head:'head-a'});
  state.result = Error('receipt unavailable');
  await model.submit(inputs);
  assert.equal(model.dialog.uncertain,true);
  await model.submit({...inputs,branch:'must-not-run'});
  assert.equal(state.prepared.length,1);
  assert.equal(state.completed.length,1);
  state.result = null;
  await model.submit(inputs);
  assert.deepEqual(state.completed,['request-0','request-0']);
});

test('closing while revision resolves cannot create a checkout', async () => {
  const {model,state,api} = fixture();let resolve;
  api.resolveGitRevision = () => new Promise(done => {resolve = done;});
  model.create({branch:'main'});const task = model.submit(inputs);
  model.close();resolve({commit:'head-a'});await task;
  assert.equal(state.prepared.length,0);
  assert.equal(model.dialog,null);
});

test('late admitted completion preserves a replacement form and selection', async () => {
  const {model,state} = fixture();let finish;
  state.result = () => new Promise(done => {finish = done;});
  model.create({branch:'main'});const task = model.submit(inputs);
  await Promise.resolve();model.close();const replacement = model.create({branch:'other'});
  finish({Ok:{data:{id:'tree-new'}}});await task;
  assert.equal(model.dialog,replacement);
  assert.deepEqual(state.selected,[]);
  assert.deepEqual(state.forgotten,['request-0']);
});

test('managed checkout keeps source revisions and deferred conversation handoff', async () => {
  const {model,state} = fixture('composer');
  await model.open('fork');
  assert.equal(model.dialog.include_changes,true);
  assert.equal(model.dialog.values.branch,'sailry/task-12345678');
  await model.submit({branch:'  task/branch  '});
  assert.deepEqual(state.prepared[0].value,{kind:'managed',project:'project-a',branch:'task/branch',expected_head:'head-a',expected_index:'index-a',include_changes:true});
  assert.deepEqual(state.selected,[{cursor:'1',value:{worktree:'tree-new',fork:true,request:'request-0'}}]);
});

test('changed conversation cannot be moved after checkout succeeds', async () => {
  const {model,state,change} = fixture('composer');let finish;
  await model.open('create');state.result = () => new Promise(done => {finish = done;});
  const task = model.submit({branch:'task/branch'});change({cursor:'2'});
  finish({Ok:{data:{id:'tree-new'}}});await task;
  assert.equal(model.dialog.error,'location_created_changed');
  assert.deepEqual(state.selected,[]);
  assert.equal(state.prepared.length,1);
});

test('unregistered removal registers without selecting, then checks drafts again', async () => {
  const {model,state} = fixture();await model.open();await model.choose('entry-1');await model.choose('action-1');
  assert.equal(state.prepared[0].value.kind,'register');
  assert.equal(model.dialog.kind,'remove');assert.deepEqual(state.selected,[]);
  state.drafts = true;await model.submit();assert.equal(state.prepared.length,1);
  assert.equal(model.dialog.error,'worktree_remove_unsaved');
  state.drafts = false;state.result = {Ok:{kind:'worktree_removed',data:{id:'tree-new'}}};await model.submit();
  assert.deepEqual(state.prepared[1].value,{kind:'remove',worktree:'tree-new',expected_head:'head-b',expected_branch:'topic'});
  assert.deepEqual(state.released,[{id:'tree-new',request:'request-1'}]);
});

test('removal conflicts retain original observed revision and ID', async () => {
  const {model,state} = fixture();await model.open();await model.choose('entry-1');await model.choose('action-1');
  state.result = {Err:{code:'revision_conflict'}};await model.submit();await model.submit();
  assert.equal(model.dialog.error,'worktree_remove_conflict');
  assert.deepEqual(state.completed,['request-0','request-1','request-1']);
  assert.deepEqual(state.released,[]);
});

test('frozen picker rows cannot be substituted by a refreshed array', async () => {
  const {model,state} = fixture();await model.open();const owner = model.dialog;
  const captured = {token:owner.token,choice:owner.choices[1]};
  owner.choices = [];
  await model.choose('entry-1',captured);await model.choose('action-0');
  assert.equal(state.prepared[0].value.path,'/native');
});

test('stale and locked native entries preserve action policy', () => {
  const catalog = {projects:[{id:'project-a',name:'Project'}],worktrees:[]};
  const location = {project:'project-a',surface:'project'};
  const base = {main:false,path:'/missing',head:'abcdef012345',branch:null,available:false,locked:false};
  const found = {kind:'repository',entries:[base,{...base,path:'/locked',locked:true}],truncated:true,omitted_paths:'1'};
  const choices = listing(catalog,found,location,key => key);
  assert.equal(choices[0].label,'abcdef01');assert.equal(choices[0].action.kind,'prune');
  assert.equal(choices[1].disabled,true);assert.equal(choices.at(-1).id,'partial');
  assert.equal(choices.at(-1).disabled,true);
});
