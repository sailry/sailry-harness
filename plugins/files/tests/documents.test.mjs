import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/documents.js',import.meta.url),'utf8'))
  .replace(/^import[\s\S]*?from 'sailry\/documents';\n/m,'').replace(/^export /gm,'');
const plain = value => JSON.parse(JSON.stringify(value));
const document = (id, fields = {}) => ({id,path:`${id}.txt`,revision:'original',dirty:false,
  saving:false,uncertain:false,truncated:false,mode:'source',markdown:false,can_copy:true,
  can_edit:true,can_save:false,can_cut_paste:true,readonly:false,error:null,...fields});

function fixture(initial, afterClose = async () => {}, confirm = async () => false) {
  let state = initial;
  const calls = [],errors = [];
  const cx = {notify(){}};
  const api = {
    readDocuments:() => state,
    nextDocumentChange:async () => { throw new Error('closed'); },
    openDocument:async (path,options) => { calls.push(['open',path,options]); },
    documentAction:async (id,action) => { calls.push(['action',id,action]); },
    saveDocument:async id => { calls.push(['save',id]); },
    closeDocument:async (id,options) => {
      calls.push(['close',id,options]);
      const value = state.documents.find(document => document.id === id);
      if (!options?.discard && (value.dirty || value.saving || value.uncertain) &&
          (!options?.confirm || !await confirm(id))) return false;
      state = {...state,cursor:String(BigInt(state.cursor) + 1n),documents:state.documents.filter(document => document.id !== id)};
      await afterClose();
      return true;
    },
  };
  const Documents = vm.runInNewContext(`${source}\nDocuments`,api);
  const documents = new Documents((...error) => errors.push(error));
  return {documents,calls,errors,cx,set:value => {state = value;}};
}

test('tab order and selection survive snapshots and exact decimal cursor ordering', () => {
  const fixture = fixtureState('9',[document('first'),document('second')]);
  const {documents,cx} = fixture;
  assert.deepEqual(plain(documents.order),['first','second']);
  assert.equal(documents.selected,'first');
  assert.equal(documents.accept({cursor:'10',documents:[document('second'),document('first'),document('third')],
    reveal:{document:'third',sequence:'1'}},cx),true);
  assert.deepEqual(plain(documents.order),['first','second','third']);
  assert.equal(documents.selected,'third');
  assert.equal(documents.accept({cursor:'9',documents:[],reveal:null},cx),false);
  assert.equal(documents.selected,'third');
});

function fixtureState(cursor,documents,confirm) { return fixture({cursor,documents,reveal:null},undefined,confirm); }

test('reveal selects existing documents and waits for an admitted loading document', async () => {
  const {documents,cx,calls} = fixtureState('1',[document('one'),document('two',{dirty:true})]);
  await documents.select('two',cx);
  assert.equal(documents.selected,'two');
  assert.deepEqual(plain(calls[0]),['action','two',{kind:'focus'}]);
  documents.accept({cursor:'2',documents:documents.state.documents,reveal:{document:'loading',sequence:'3'}},cx);
  assert.equal(documents.selected,'two');
  documents.accept({cursor:'3',documents:[...documents.state.documents,document('loading')],
    reveal:{document:'loading',sequence:'3'}},cx);
  assert.equal(documents.selected,'loading');
  documents.accept({cursor:'4',documents:documents.state.documents,reveal:{document:'two',sequence:'4'}},cx);
  assert.equal(documents.selected,'two');
  assert.equal(documents.current().dirty,true);
});

test('native confirmation cancellation preserves the draft and selection', async () => {
  const {documents,cx,calls} = fixtureState('1',[document('dirty',{dirty:true}),document('other')]);
  await documents.close('dirty',cx);
  assert.equal(documents.items().length,2);
  assert.equal(documents.selected,'dirty');
  assert.equal(documents.current().dirty,true);
  assert.deepEqual(plain(calls),[['close','dirty',{confirm:true}]]);
});

test('native confirmation closes the captured document after selection changes', async () => {
  let answer;
  const pending = new Promise(resolve => { answer = resolve; });
  const {documents,cx,calls} = fixtureState('1',[document('dirty',{dirty:true}),document('other')],() => pending);
  const closing = documents.close('dirty',cx);
  await documents.select('other',cx);
  answer(true);
  await closing;
  assert.deepEqual(plain(documents.order),['other']);
  assert.equal(documents.selected,'other');
  assert.deepEqual(plain(calls),[['close','dirty',{confirm:true}],['action','other',{kind:'focus'}]]);
});

test('closing the active tab focuses its selected successor for consecutive closes', async () => {
  const {documents,cx,calls} = fixtureState('1',[document('first'),document('second'),document('third')]);
  await documents.select('second',cx);
  calls.length = 0;
  await documents.close('second',cx);
  assert.equal(documents.selected,'third');
  assert.deepEqual(plain(calls),[['close','second',{confirm:true}],['action','third',{kind:'focus'}]]);
  calls.length = 0;
  await documents.close('third',cx);
  assert.equal(documents.selected,'first');
  assert.deepEqual(plain(calls),[['close','third',{confirm:true}],['action','first',{kind:'focus'}]]);
  calls.length = 0;
  await documents.close('first',cx);
  assert.equal(documents.selected,null);
  assert.deepEqual(plain(calls),[['close','first',{confirm:true}]]);
});

test('background and rejected closes preserve focus while confirmed active discard transfers it', async () => {
  let accept = false;
  const {documents,cx,calls} = fixtureState('1',[document('dirty',{dirty:true}),document('background'),document('next')],async () => accept);
  await documents.close('background',cx);
  assert.equal(documents.selected,'dirty');
  assert.equal(calls.some(call => call[0] === 'action'),false);
  await documents.close('dirty',cx);
  assert.equal(documents.selected,'dirty');
  assert.equal(calls.some(call => call[0] === 'action'),false);
  accept = true;
  await documents.close('dirty',cx);
  assert.equal(documents.selected,'next');
  assert.deepEqual(plain(calls.at(-1)),['action','next',{kind:'focus'}]);
});

test('a snapshot before the close reply preserves focus handoff without overriding a newer selection', async () => {
  for (const select of [false,true]) {
    let complete;
    const pending = new Promise(resolve => { complete = resolve; });
    const {documents,cx,calls} = fixture({cursor:'1',documents:[document('first'),document('second'),document('third')],reveal:null},() => pending);
    await documents.select('third',cx);
    calls.length = 0;
    const closing = documents.close('third',cx);
    documents.refresh(cx);
    assert.equal(documents.selected,'second');
    if (select) await documents.select('first',cx);
    complete();
    await closing;
    assert.equal(documents.selected,select ? 'first' : 'second');
    assert.deepEqual(plain(calls.filter(call => call[0] === 'action')),
      [['action',select ? 'first' : 'second',{kind:'focus'}]]);
  }
});

test('save and editor commands use selected opaque handles and refresh authoritative state', async () => {
  const {documents,cx,calls,set} = fixtureState('1',[document('a'),document('b',{dirty:true,can_save:true})]);
  await documents.save(cx);
  assert.equal(calls.length,0);
  await documents.select('b',cx);
  set({cursor:'2',documents:[document('a'),document('b',{dirty:false,revision:'saved'})],reveal:null});
  await documents.save(cx);
  assert.deepEqual(plain(calls.at(-1)),['save','b']);
  assert.equal(documents.current().revision,'saved');
  await documents.action('find',cx);
  assert.deepEqual(plain(calls.at(-1)),['action','b',{kind:'find'}]);
  await documents.open('nested/资料.txt',42,cx);
  assert.deepEqual(plain(calls.at(-1)),['open','nested/资料.txt',{line:42}]);
});

test('uncertain outcomes preserve drafts and report each changed failure once', async () => {
  const error = {code:'outcome_unknown',message:'Publication is unconfirmed'};
  const state = {cursor:'1',documents:[document('a',{dirty:true,uncertain:true,error})],reveal:null};
  const {documents,cx,errors,calls} = fixture(state);
  assert.equal(errors.length,1);
  documents.accept({...state,cursor:'2'},cx);
  assert.equal(errors.length,1);
  await documents.save(cx);
  assert.equal(calls.length,0);
  await documents.close('a',cx);
  assert.equal(documents.current().dirty,true);
  assert.deepEqual(plain(calls),[['close','a',{confirm:true}]]);
  documents.accept({cursor:'3',documents:[document('a')],reveal:null},cx);
  documents.accept({...state,cursor:'4'},cx);
  assert.equal(errors.length,2);
});
