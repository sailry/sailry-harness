import test from 'node:test';
import assert from 'node:assert/strict';
import {State,query,choices,nodeId} from '../dev.sailry.platform/desktop/state.js';
import {metrics} from '../dev.sailry.platform/desktop/metrics.js';
import {requestRows} from '../dev.sailry.platform/desktop/requests.js';
import {messages} from '../dev.sailry.platform/desktop/locales.js';
const text=messages('en');
test('normalizes public endpoint bytes into an opaque stable selection',()=>{
  assert.equal(nodeId(Array(32).fill(15)),'0f'.repeat(32));
  assert.equal(new State(Array(32).fill(255)).node,'ff'.repeat(32));
});
const position={node:'node-a',timestamp_ms:'1',sequence:'18446744073709551615'};
const report={requests:{items:[{position}],has_more:true}};
test('cursor pagination retains prior geometry and rejects stale or incomplete observations',()=>{
  const state=new State('node-a'),generation=state.start();
  assert(state.accept({cursor:'9',all:false,view:{report,connected:true,refreshing:false}},generation));
  assert(state.turn(2));assert.equal(state.data(),report);assert.equal(state.query.before.sequence,position.sequence);
  const next=state.start();assert(!state.accept({cursor:'10',all:false,view:{}},generation));
  assert(state.accept({cursor:'1',all:false,view:{report,connected:true,refreshing:true}},next));
  assert.equal(state.data(),report);assert(!state.ready());assert(!state.turn(3));
  assert(state.accept({cursor:'11',all:false,view:{report,connected:true,refreshing:false}},next));
  assert(!state.accept({cursor:'9',all:false,view:null},next));assert(state.ready());
  assert(state.turn(1));assert.equal(state.query.before,null);
});
test('bound pages reject aggregate reports and page-level host retargeting',()=>{
  const state=new State('a'),generation=state.start();
  assert(!state.select('host','all'));assert(!state.select('host','b'));assert.equal(state.node,'a');
  assert(!state.accept({cursor:'1',all:true,view:{summary:report,complete:true}},generation));
  assert.equal(state.data(),null);assert(!state.turn(2));
  assert(state.accept({cursor:'2',all:false,view:{report,connected:true,refreshing:false}},generation));assert(state.turn(2));
});
test('revoked access clears retained pagination data while transient errors preserve it',()=>{
  for(const code of ['not_configured','not_found','permission_denied','revision_conflict']) {
    const state=new State('a'),generation=state.start();
    state.accept({cursor:'1',all:false,view:{report,connected:true,refreshing:false}},generation);
    assert(state.turn(2));const next=state.start();
    state.accept({cursor:'1',all:false,view:{report:null,connected:true,refreshing:false,error:{code:'unavailable'}}},next);
    assert.equal(state.data(),report);
    state.accept({cursor:'2',all:false,view:{report:null,connected:true,refreshing:false,error:{code}}},next);
    assert.equal(state.data(),null);assert.equal(state.previous,null);assert(!state.ready());
    state.accept({cursor:'3',all:false,view:{report,connected:true,refreshing:false,error:null}},next);
    assert.equal(state.data(),report);assert(state.ready());
  }
});
test('filter changes reset dependent fields and preserve frozen deleted model choices',()=>{
  const state=new State('a');state.query.providers=['p'];state.query.models=['removed'];state.query.projects=['x'];
  state.select('range','30');assert.deepEqual(state.query.models,['removed']);
  const options=choices(state,{projects:[],providers:[]},[{kind:'model',data:{provider:'p',model:'old'}}],text);
  assert(options.model.some(item=>item.id==='old'));assert(options.model.some(item=>item.id==='removed'));
  state.select('project','next');assert.deepEqual(state.query.providers,[]);assert.deepEqual(state.query.models,[]);
  assert.equal(query(7,Date.UTC(2026,8,30,23)).end_ms,Date.UTC(2026,9,1));
});
test('composer distinguishes unknown usage from known zero and exposes context thresholds',()=>{
  const unknown=metrics({has_messages:true},text);assert.equal(unknown[0].value,'—');assert.equal(unknown[2].value,'—');
  const zero=metrics({has_messages:true,statistics:{usage:{input:'0',output:'0',cached_input:'0',reasoning:'0'},context_tokens:'100'},context_limit:100,compacting:true},text);
  assert.equal(zero[0].value,'0');assert.equal(zero.at(-1).value.tone,'danger');assert(zero.at(-1).value.loading);
});
test('requests preserve exact token details and complete identity despite rounded display',()=>{
  const request={position,model:'model',provider_name:'Removed provider',scope_name:'Removed project',tokens:{input:'18446744073709551615',output:'1',cached_input:'0',reasoning:'0'},first_token_us:'1250',elapsed_us:'1000000',usd_micros:null};
  const rows=requestRows([request],text);assert.equal(rows.rows[0].length,6);assert.equal(rows.cells[0].length,6);
  assert.equal(rows.rows[0][2],`${request.tokens.input} · 1`);assert.equal(rows.details[0][2],`Input: ${request.tokens.input}\nOutput: 1`);
  assert.equal(rows.cells[0][4].primary.text,'1.3 ms');assert.equal(rows.cells[0][4].secondary.text,'1.00 s');assert.equal(rows.rows[0][5],'—');
  assert.equal(rows.details[0][4],'First token: 1250 µs\nDuration: 1000000 µs');
  assert.equal(rows.row_ids[0],`node-a:1:${position.sequence}`);assert.equal(rows.details[0][0],'Model: model\nProvider: Removed provider');
  assert.notEqual(rows.cells[0][2].primary.icon,rows.cells[0][2].secondary.icon);
  assert.notEqual(rows.cells[0][2].primary.tone,rows.cells[0][2].secondary.tone);
});
test('unassigned requests use a localized scope without changing response identity',()=>{
  const request={session:'session-a',position,model:'model',provider_name:'Provider',scope_name:'',
    tokens:{input:'12',output:'4',cached_input:'0',reasoning:'0'},first_token_us:null,elapsed_us:null,usd_micros:null};
  for(const locale of ['en','zh-CN']) {
    const text=messages(locale),rows=requestRows([request],text);
    assert.equal(rows.cells[0][1].primary.text,text.usage_unassigned);assert(rows.details[0][1].includes(text.usage_unassigned));
    assert.equal(rows.row_ids[0],`node-a:1:${position.sequence}`);
    assert.equal(rows.rows[0][2],'12 · 4');
  }
});
test('request cells keep missing counters unknown and exact microdollar costs available',()=>{
  const request={position,model:'model',provider_name:'Provider',scope_name:'Project',tokens:{input:null,output:'0',cached_input:null,reasoning:'0'},
    first_token_us:null,elapsed_us:null,usd_micros:'18446744073709551615'};
  const rows=requestRows([request],text);
  assert.equal(rows.cells[0][2].primary.text,'—');assert.equal(rows.cells[0][2].secondary.text,'0');
  assert.equal(rows.details[0][2],'Input: —\nOutput: 0');assert.equal(rows.details[0][3],'Cached: —\nReasoning: 0');
  assert.equal(rows.details[0][4],'First token: —\nDuration: —');
  assert.equal(rows.rows[0][5],'18446744073709.551615');assert.equal(rows.details[0][5],rows.rows[0][5]);
});
