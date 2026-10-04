import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const code = await readFile(new URL('../dev.sailry.platform/desktop/format.js',import.meta.url),'utf8');
const format = await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));

test('missing usage and prices stay unknown while reported zero remains zero',()=>{
  assert.equal(format.total(null),null);
  assert.equal(format.amount(null),null);
  assert.equal(format.cost({responses:'4',cost:null}),null);
  assert.equal(format.amount('0'),'0.00');
  assert.equal(format.speed({elapsed_us:'0',output_tokens:'9'}),null);
  assert.equal(format.cache({input:'0',cached_input:'0'}),null);
});
test('money and aggregate counts preserve integers beyond the script number limit',()=>{
  assert.equal(format.amount('9007199254740991'),'9007199254.74');
  assert.equal(format.total({input:'18446744073709551615',output:'18446744073709551615'}),36893488147419103230n);
  assert.equal(format.amount('999999'),'0.99');
  assert.equal(format.cost({responses:'5',cost:{responses:'3',usd_micros:'12300'}}),'0.01+');
});
test('formatting retains original compact units and context thresholds',()=>{
  assert.deepEqual(['999','1000','92027','7234549','1200000000'].map(format.compact),['999','1.0k','92.0k','7.2M','1.2B']);
  assert.equal(format.context('120','100'),120);
  assert.equal(format.context('20','0'),null);
  assert.deepEqual([null,79.9,80,99.9,100,120].map(format.severity),['muted','muted','warning','warning','danger','danger']);
});
test('ranking keeps exact totals and stable node/provider ties',()=>{
  const row = (node,provider,model,input) => ({node,group:{key:{kind:'model',data:{provider,model}},metrics:{tokens:input === null ? null : {input,output:'0'}}}});
  const rows=[row('b','a','same','9007199254740992'),row('a','b','same','9007199254740993'),row('a','a','same','9007199254740992'),row('a','a','missing',null)];
  assert.deepEqual(format.ranking(rows).map(row=>[row.node,row.group.key.data.provider]),[['a','b'],['a','a'],['b','a'],['a','a']]);
});
