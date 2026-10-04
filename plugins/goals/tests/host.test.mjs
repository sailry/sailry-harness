import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const plain=value=>JSON.parse(JSON.stringify(value));
const value={id:'goal-id',description:'Finish the requested work',state:'active',after:null};
async function fixture(entry={key:'goal',revision:'5',present:true,restored:true,value}) {
  let current=plain(entry),state;
  const requests=[];
  const scope={session:'session-id',turn:null};
  const history={page:{runs:[{turn:'last-turn',sequence:1,status:'completed',origin:null}]},next_before:null};
  const sdk={context:()=>scope,newId:()=> 'new-goal',getConversationValue:async key=>{
    assert.equal(key,'goal');return plain(current);
  },setConversationValue:(key,value,revision)=>prepare([{kind:'conversation_write',data:{key,value,expected_revision:revision}}]),
  prepareTransaction:prepare,completeRequest:async id=>{
    const operations=requests[Number(id)];
    for (const operation of operations) {
      if (operation.kind==='conversation_write' || operation.kind==='conversation_remove') {
        assert.equal(operation.data.key,'goal');
        assert.equal(operation.data.expected_revision,current.revision);
        current={...current,revision:String(BigInt(current.revision)+1n),restored:false,
          present:operation.kind==='conversation_write',value:operation.data.value ?? null};
      }
    }
    return {Ok:{data:operations.length===1 ? plain(current) : operations.map(()=>({data:{}}))}};
  },forgetRequest:()=>{},readSession:async()=>({revision:'3',config:{mode:'code'}}),
  readConversation:async()=>plain(history),readTurn:async()=>({run:plain(history.page.runs[0])}),
  readTurnState:()=>state,stageTurnState:value=>state=plain(value)};
  function prepare(operations) {requests.push(plain(operations));return String(requests.length-1);}
  const context=vm.createContext({});
  const bridge=new vm.SyntheticModule(Object.keys(sdk),function(){
    for (const [name,value] of Object.entries(sdk)) this.setExport(name,value);
  },{context});
  const modules=new Map([['sailry/sdk',bridge]]);
  async function load(path) {
    if (modules.has(path)) return modules.get(path);
    const source=await readFile(new URL(`../dev.sailry.platform/host/${path}`,import.meta.url),'utf8');
    const module=new vm.SourceTextModule(source,{context,identifier:path});
    modules.set(path,module);await module.link(load);return module;
  }
  const main=await load('./main.js');await main.evaluate();
  return {api:main.namespace,requests,state:()=>state,current:()=>plain(current)};
}

test('restored active goals remain suspended without a write or continuation',async()=>{
  const {api,requests,state}=await fixture();
  assert.equal((await api.read()).goal.state,'paused');
  const initialized=await api.initialize({mode:'code',tools:['get_goal','create_goal','update_goal']});
  assert.equal(initialized.instruction,'');
  assert.equal(state().executingGoal,false);
  assert.deepEqual(plain(await api.completed({turn:'last-turn'})),{});
  assert.equal(requests.length,0);
  await assert.rejects(api.update({goal_id:value.id,expected_revision:'5',description:value.description,state:'completed'}),/No active goal/);
});

test('restoration preserves terminal state and exact live revision',async()=>{
  for (const state of ['paused','blocked','failed','completed']) {
    const {api}=await fixture({key:'goal',revision:'9007199254740993',present:true,restored:true,value:{...value,state}});
    const result=await api.read();
    assert.equal(result.goal.id,value.id);
    assert.equal(result.goal.description,value.description);
    assert.equal(result.goal.state,state);
    assert.equal(result.goal.revision,'9007199254740993');
  }
});

test('only explicit resume writes a live state and submits work',async()=>{
  const {api,requests,current}=await fixture();
  const result=await api.control({kind:'resume',expected_revision:'5'});
  assert.equal(result.goal.state,'active');
  assert.equal(current().restored,false);
  assert.equal(requests.length,1);
  assert.deepEqual(requests[0].map(operation=>operation.kind),['conversation_write','submit']);
  assert.equal(requests[0][0].data.key,'goal');
  assert.equal(requests[0][0].data.value.after,'last-turn');
  assert.equal(requests[0][1].data.session,'session-id');
  await api.completed({turn:'last-turn'});
  assert.equal(requests[1][0].kind,'continue');
  assert.equal(requests[1][0].data.scope,'conversation');
  assert.equal(requests[1][0].data.expected_revision,'6');
});

test('clear records absence in the captured conversation',async()=>{
  const {api,requests,current}=await fixture();
  assert.equal((await api.control({kind:'clear',expected_revision:'5'})).goal,null);
  assert.deepEqual(requests[0],[{kind:'conversation_remove',data:{key:'goal',expected_revision:'5'}}]);
  assert.equal(current().present,false);
  assert.equal(current().revision,'6');
});
