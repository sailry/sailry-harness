import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/transfers.js',import.meta.url),'utf8'))
  .replace(/^import[\s\S]*?;\n/gm,'').replace(/^export /gm,'');
const cx = {notify(){}};
const plain = value => JSON.parse(JSON.stringify(value));
function fixture(options = {}) {
  let state = {cursor:'0',transfers:[]};
  let clipboard = {id:'captured',available:true,cut:true,source_label:'Source',paths:[{path:'a.txt',kind:'file'},{path:'b.txt',kind:'file'}]};
  const calls = [],errors = [],notices=[];
  const add = (path,kind) => {
    const transfer = {id:`transfer-${state.transfers.length}`,kind,stage:'ready',path,can_start:true,can_check:false,can_cancel:true};
    state = {cursor:String(BigInt(state.cursor)+1n),transfers:[...state.transfers,transfer]};
    return transfer;
  };
  const {Transfers,status} = vm.runInNewContext(`${source}\n({Transfers,status})`,{
    join:(directory,name) => directory ? `${directory}/${name}` : name,
    readTransfers:() => state,readClipboard:() => clipboard,
    preparePaste:async (...args) => {calls.push(['preparePaste',...args]); return add(args[2],'move');},
    prepareMove:async (...args) => {calls.push(['prepareMove',...args]); return add(args[1],'move');},
    transferAction:async (...args) => {calls.push(['action',...args]); return state.transfers.find(value => value.id === args[0]);},
    captureClipboard:async (...args) => {calls.push(['capture',...args]); if (options.capture) clipboard = await options.capture; return clipboard;},
    nextTransferChange:async () => {throw new Error('closed');},
  });
  return {transfers:new Transfers((key,kind)=>{errors.push(key);notices.push({key,kind});}),status,calls,errors,notices,clipboard,
    update:(id,fields) => {state={cursor:String(BigInt(state.cursor)+1n),transfers:state.transfers.map(value => value.id === id ? {...value,...fields} : value)};}};
}

test('paste waits for the pending capture instead of using the previous owner', async () => {
  let complete;
  const pending = new Promise(resolve => { complete = resolve; });
  const f = fixture({capture:pending});
  const captured = f.transfers.capture(['current.txt'],false,cx);
  const pasted = f.transfers.paste('target',cx);
  await Promise.resolve();
  assert.equal(f.calls.some(call => call[0] === 'preparePaste'),false);
  complete({id:'current-owner',available:true,paths:[{path:'current.txt',kind:'file'}]});
  await Promise.all([captured,pasted]);
  assert.deepEqual(plain(f.calls),[['capture',['current.txt'],false],['preparePaste','current-owner','current.txt','target/current.txt']]);
  assert.equal(f.transfers.capturing,null);
});

test('transfer outcomes are reported once while retaining the transfer and recovery controls',async()=>{
  const f=fixture();await f.transfers.move('source.txt','target/source.txt',cx);const id=f.transfers.current;
  f.update(id,{stage:'uncertain',can_start:false,can_check:true});f.transfers.refresh(cx);f.transfers.refresh(cx);
  assert.deepEqual(f.notices,[{key:'files_move_unknown',kind:'error'}]);assert.equal(f.transfers.current,id);assert.equal(f.transfers.value().can_check,true);
  f.update(id,{stage:'failed',error:{code:'permission_denied'},can_start:true,can_check:false});f.transfers.refresh(cx);f.transfers.refresh(cx);
  assert.equal(f.notices.length,2);assert.equal(f.notices[1].kind,'error');assert.equal(f.transfers.value().path,'target/source.txt');
  f.update(id,{stage:'cancelled',error:null,can_start:false});f.transfers.refresh(cx);f.transfers.refresh(cx);
  assert.deepEqual(f.notices.at(-1),{key:'files_paste_cancelled',kind:'info'});assert.equal(f.notices.length,3);
});

test('a failed pending capture never prepares a paste from the previous owner', async () => {
  let fail;
  const pending = new Promise((_,reject) => { fail = reject; });
  const f = fixture({capture:pending});
  const captured = f.transfers.capture(['current.txt'],false,cx);
  const pasted = f.transfers.paste('target',cx);
  fail(new Error('Capture unavailable'));
  const results = await Promise.allSettled([captured,pasted]);
  assert.equal(results.every(result => result.status === 'rejected'),true);
  assert.equal(f.calls.some(call => call[0] === 'preparePaste'),false);
  assert.equal(f.transfers.current,null);assert.equal(f.transfers.capturing,null);
});

test('paste freezes source clipboard and never begins before confirmation', async () => {
  const f = fixture(); await f.transfers.paste('target',cx);
  assert.deepEqual(plain(f.calls),[['preparePaste','captured','a.txt','target/a.txt']]);
  const id = f.transfers.current;
  await f.transfers.action('start','target/edited.txt',cx);
  f.update(id,{stage:'uncertain',can_start:false,can_check:true,can_cancel:false}); f.transfers.refresh(cx);
  await f.transfers.action('check',undefined,cx);
  assert.deepEqual(plain(f.calls.at(-1)),['action',id,{kind:'check'}]);
  assert.equal(f.calls.filter(call => call[0] === 'preparePaste').length,1);
});

test('observer and action completion advance a batch only once', async () => {
  const f = fixture(); await f.transfers.paste('',cx);
  f.update(f.transfers.current,{stage:'done'}); f.transfers.refresh(cx);
  await Promise.all([f.transfers.next(cx),f.transfers.next(cx)]);
  assert.deepEqual(plain(f.calls.filter(call => call[0] === 'preparePaste')),
    [['preparePaste','captured','a.txt','a.txt'],['preparePaste','captured','b.txt','b.txt']]);
  assert.equal(f.transfers.value().path,'b.txt');
});

test('drag moves use a scoped handle without replacing the existing clipboard', async () => {
  const f = fixture(); await f.transfers.move('source.txt','folder/source.txt',cx);
  assert.deepEqual(plain(f.calls),[['prepareMove','source.txt','folder/source.txt']]);
  assert.equal(f.transfers.clipboard.id,'captured');
  assert.equal(f.transfers.queue,null);
});

test('closing uncertainty dismisses the view and retains its authoritative operation', async () => {
  const f = fixture(); await f.transfers.paste('',cx);
  const id = f.transfers.current; f.update(id,{stage:'uncertain',can_cancel:false}); f.transfers.refresh(cx);
  await f.transfers.close(cx);
  assert.deepEqual(plain(f.calls.at(-1)),['action',id,{kind:'dismiss'}]);
  assert.equal(f.transfers.current,null); assert.equal(f.transfers.queue,null);
  assert.equal(f.status({kind:'move',stage:'uncertain',published:true}),'files_move_partial');
});
