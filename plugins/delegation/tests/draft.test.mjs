import assert from 'node:assert/strict';
import test from 'node:test';
import {catalog,create,source,selectModel,profile,efforts,errorKey} from '../dev.sailry.platform/desktop/draft.js';

const models = [
  {kind:'provider',id:'provider-a/plain',provider:'First',model:'plain',default:false,reasoning:false,efforts:[],default_effort:'default'},
  {kind:'provider',id:'provider-a/vendor/reasoned',provider:'First',model:'vendor/reasoned',default:true,reasoning:true,efforts:['default','low','high'],default_effort:'high'},
  {kind:'provider',id:'provider-b/other',provider:'Second',model:'other',default:true,reasoning:true,efforts:[{budget:2048},{budget:4096}],default_effort:{budget:2048}},
  {kind:'unsupported',id:'unsupported/private',provider:'Unavailable',model:'private',default:false,reasoning:false,efforts:[],default_effort:'none'}
];
const fields = {key:'reviewer',name:'Reviewer',turns:'',instructions:'Review changes'};
const original = {id:'role-one',revision:9,key:'old',name:'Original',description:'Preserved description',
  appearance:{icon:'ai',color:'blue'},model:{provider:'missing',model:'old/model',effort:'high'},
  max_turns:8,skills:['retained-skill'],instructions:'Original instructions'};

test('fixed source uses the first provider default and keeps the full model identifier',() => {
  const state = create(null,'new',catalog({models:[models[2],...models.slice(0,2),models[3]]}));
  source(state,true);
  assert.deepEqual(state.model,{provider:'provider-a',model:'vendor/reasoned',effort:'high'});
  assert.equal(state.models.length,3);
  source(state,false); assert.equal(state.model,null);
  state.models[1].default = false; source(state,true);
  assert.deepEqual(state.model,{provider:'provider-a',model:'plain',effort:null});
});

test('model changes retain supported efforts and use the native initial choice otherwise',() => {
  const state = create(null,'new',catalog({models})); source(state,true);
  state.model.effort = 'low'; selectModel(state,state.models[1]); assert.equal(state.model.effort,'low');
  selectModel(state,state.models[2]); assert.deepEqual(state.model.effort,{budget:2048});
  state.model.effort = {budget:4096}; selectModel(state,state.models[2]);
  assert.deepEqual(state.model.effort,{budget:4096});
  assert.deepEqual(efforts(state),[null,{budget:2048},{budget:4096}]);
  const empty = {...state.models[1],efforts:[],default_effort:'default'};
  selectModel(state,empty); assert.equal(state.model.effort,'default');
  selectModel(state,state.models[0]); assert.equal(state.model.effort,null);
});

test('editing preserves unavailable model identity and unexposed description and skills',() => {
  const state = create(original,'unused',catalog({models}));
  const result = profile(state,{...fields,turns:' 25 ',instructions:'Changed'});
  assert.equal(result.id,'role-one'); assert.equal(result.revision,9);
  assert.deepEqual(result.model,original.model); assert.equal(result.description,original.description);
  assert.deepEqual(result.skills,original.skills); assert.equal(result.max_turns,25);
  assert.equal(original.instructions,'Original instructions');
  source(state,false); assert.equal(profile(state,fields).model,null);
});

test('validates exact key, UTF-8 and u32 turn limits before preparing a command',() => {
  const state = create(null,'new',[]);
  for (const key of ['', 'UPPER', '-leading', 'trailing-', 'with space', 'x'.repeat(129)]) {
    assert.throws(() => profile(state,{...fields,key}),/role_invalid/);
  }
  for (const turns of ['0','-1','1.5','4294967296','2x']) {
    assert.throws(() => profile(state,{...fields,turns}),/role_invalid/);
  }
  assert.equal(profile(state,{...fields,key:' a ',name:' Name ',turns:'+4294967295'}).max_turns,4294967295);
  assert.throws(() => profile(state,{...fields,name:'界'.repeat(43)}),/role_invalid/);
  assert.throws(() => profile(state,{...fields,instructions:'界'.repeat(5462)}),/role_invalid/);
  source(state,true); assert.throws(() => profile(state,fields),/role_invalid/);
});

test('retains native conflict, unavailable, and uncertain distinctions',() => {
  assert.equal(errorKey('revision_conflict'),'role_conflict');
  assert.equal(errorKey('not_found'),'role_conflict');
  assert.equal(errorKey('conflict'),'role_duplicate');
  assert.equal(errorKey('not_configured'),'role_model_unavailable');
  assert.equal(errorKey('outcome_unknown'),'role_unknown');
  assert.equal(errorKey('busy'),'role_capacity');
});
