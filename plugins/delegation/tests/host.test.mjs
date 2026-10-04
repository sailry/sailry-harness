import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
const source = (await readFile(new URL('../dev.sailry.platform/host/main.js',import.meta.url),'utf8')).replace(/^import .*\n/gm,'').replace(/^export /gm,'');
const plain = value => JSON.parse(JSON.stringify(value));
function host(roles = []) {
  let state;
  const functions = vm.runInNewContext(`${source}\n({initialize,prepare,result})`,{readTurnState:()=>state,stageTurnState:value=>state=value});
  const initialized = functions.initialize({tools:['spawn_agent'],roles});
  return {...functions,initialized};
}
test('the admitted roster controls selection and instructions',()=>{
  const policy = host([{key:'review',name:'Review',description:'Inspect changes'}]);
  assert.match(policy.initialized.instruction,/review: Inspect changes/);
  assert.deepEqual(plain(policy.initialized.tools),['spawn_agent']);
  const args = {role:'review',task:'Review the change',title:'检查 🙂'};
  assert.deepEqual(plain(policy.prepare(args)),args);
  for (const role of [null,'unknown',undefined]) assert.equal(policy.prepare({...args,role}).isError,true);
  assert.equal(host().prepare({role:null,task:'Independent task'}).isError,undefined);
});
test('invalid titles, scopes and empty tasks never reach child admission',()=>{
  const policy = host(), base = {role:null,task:'Task'};
  for(const args of [{...base,title:'a\nb'},{...base,title:'😀'.repeat(81)},{...base,task:' '},{...base,worktree:'../other'},{...base,session:'elsewhere'}]) assert.equal(policy.prepare(args).isError,true);
  assert.equal(policy.prepare({...base,title:'😀'.repeat(80),worktree:'11111111-1111-4111-8111-111111111111'}).isError,undefined);
});
test('child outcomes remain authoritative, including literal failures',()=>{
  const policy = host(), failure = {error:{code:'outcome_unknown',message:'Literal error'},isError:true};
  assert.deepEqual(plain(policy.result({output:failure})),failure);
  const result = {session:'child',turn:'turn',status:'completed',response:'Ready'};
  assert.deepEqual(plain(policy.result({output:{kind:'delegation',data:result}})),result);
});
