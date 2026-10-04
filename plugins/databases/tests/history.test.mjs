import test from 'node:test';
import assert from 'node:assert/strict';
import {updates,result} from '../dev.sailry.platform/desktop/history.js';
test('distinguishes restored results from new canonical responses and preserves exact cells',()=>{
  const call={key:'turn:call',name:'database_query',arguments:{connection:'original',sql:'SELECT id'},state:'returned',sequence:'9007199254740993',result:{kind:'database_outcome',data:{kind:'query',data:{columns:['id'],rows:[[{kind:'integer',value:'9223372036854775807'}]],affected_rows:'0',truncated:false}}}};
  const snapshot={connected:true,session:'session',revision:'4',latest:call.sequence,calls:[call]};
  assert.equal(updates(snapshot,null)[0].live,false);
  const before={connected:true,session:'session',revision:'4',latest:'9007199254740992',calls:[]};
  assert.equal(updates(snapshot,before)[0].live,true);
  assert.equal(updates(snapshot,{...before,revision:'3'})[0].live,false);
  assert.deepEqual(updates(snapshot,snapshot),[]);
  assert.equal(result(call).rows.rows[0][0].value,'9223372036854775807');
  assert.deepEqual(result({...call,result:{error:{code:'permission_denied',message:'Denied'}}}),{error:{code:'permission_denied',message:'Denied'}});
  assert.deepEqual(result({...call,result:null,state:'interrupted'}),{error:{code:'cancelled'}});
});
