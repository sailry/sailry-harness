import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

const shell = (id = 'shell') => ({id, revision:1, tool:null, ssh:null, status:{kind:'stopped'}});

async function fixture(items = [shell()], resource = {kind:'terminal', id:items[0]?.id}) {
  const context = vm.createContext({});
  const prepared = [], completed = [], forgotten = [], panes = [], updates = [], notices = [], requests = new Map();
  const host = {items, complete:null, prepareError:null};
  const prepare = (action, target) => {
    const id = `request-${prepared.length}`;
    const request = {id, action, target};
    prepared.push(request); requests.set(id, request); return id;
  };
  const exports = {
    'gpui-kit':{View:class {}},
    sailry:{context:() => JSON.stringify({locale:'en'})},
    'sailry/sdk':{
      listTerminals:async () => host.items,
      listTerminalTools:async () => [],
      prepareTerminal:target => prepare('create', target),
      prepareOpenTerminal:target => { if (host.prepareError) throw host.prepareError; return prepare('open', target); },
      prepareCloseTerminal:target => prepare('close', target),
      completeRequest:async id => {
        completed.push(id);
        const request = requests.get(id);
        if (host.complete) return host.complete(request);
        host.items = host.items.map(item => item.id === request.target
          ? {...item, revision:item.revision + 1, status:{kind:'running'}} : item);
        return {Ok:{kind:'terminal', data:{id:request.target}}};
      },
      forgetRequest:id => forgotten.push(id), nextChange() {},
    },
    'sailry/ui':{nextMenuEvent() {}, pane:() => resource ? {resource} : null,
      openPane:value => { panes.push(value); return true; }, updatePane:value => updates.push(value),
      closePane() {}, nextPaneEvent() {},toast:value=>notices.push(value)},
    './locales.js':{messages:() => ({terminal:'Terminal', tools:{},failed:'Terminal action failed',unknown:'Outcome unknown',loadFailed:'Could not load terminals'})},
    './view.js':{render() {}},
    './labels.js':{title:() => null, directory:() => null},
  };
  const module = new vm.SourceTextModule(await readFile(new URL('../dev.sailry.platform/desktop/main.js', import.meta.url), 'utf8'), {context});
  await module.link(name => new vm.SyntheticModule(Object.keys(exports[name]), function() {
    for (const [key, value] of Object.entries(exports[name])) this.setExport(key, value);
  }, {context}));
  await module.evaluate();
  const view = new module.namespace.default();
  view.init(null, {spawn() {}});
  const pending = [];
  const cx = {notify() {}, spawn(callback) { pending.push(Promise.resolve().then(() => callback(cx))); }};
  let cursor = 0;
  const settle = async () => { while (cursor < pending.length) await pending[cursor++]; };
  return {view, host, cx, settle, prepared, completed, forgotten, panes, updates,notices};
}

test('reopens the captured stopped shell once with its existing identity', async () => {
  const setup = await fixture();
  setup.view.refresh(setup.cx); await setup.settle();
  assert.deepEqual(setup.prepared, [{id:'request-0', action:'open', target:'shell'}]);
  assert.equal(setup.view.selected, 'shell');
  assert.equal(setup.view.items[0].status.kind, 'running');
  assert.equal(setup.view.pending, null);
  assert.equal(setup.panes.length, 0);
  assert.deepEqual(setup.forgotten, ['request-0']);
  setup.view.refresh(setup.cx); setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.prepared.length, 1);
});

test('opens only the selected ordinary shell and never replays CLI or SSH launches', async () => {
  for (const item of [
    {...shell(), tool:'claude'}, {...shell(), ssh:'saved-host'},
    {...shell(), status:{kind:'exited', data:{code:0}}}, {...shell(), status:{kind:'running'}},
  ]) {
    const setup = await fixture([item]);
    setup.view.refresh(setup.cx); await setup.settle();
    assert.equal(setup.prepared.length, 0);
  }
  const setup = await fixture([shell('first'), shell('second')], null);
  setup.view.selected = 'second';
  setup.view.refresh(setup.cx); await setup.settle();
  assert.deepEqual(setup.prepared.map(request => request.target), ['second']);
  assert.equal(setup.view.items[0].status.kind, 'stopped');
});

test('selecting a stopped shell tab opens it without creating another pane', async () => {
  const setup = await fixture([{...shell('first'), status:{kind:'running'}}, shell('second')], null);
  setup.view.selected = 'first';
  setup.view.refresh(setup.cx); await setup.settle();
  setup.view.select(1, setup.cx); await setup.settle();
  assert.equal(setup.view.selected, 'second');
  assert.deepEqual(setup.prepared.map(request => request.target), ['second']);
  assert.equal(setup.panes.length, 0);
});

test('confirmed open failure requires an explicit retry with a new request', async () => {
  const setup = await fixture();
  setup.host.complete = async () => ({Err:{code:'busy'}});
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.view.error, 'failed');
  assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].message,'Terminal action failed');
  assert.equal(setup.view.openRetry, 'shell');
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.prepared.length, 1);
  setup.host.complete = null;
  setup.view.retry(setup.cx); await setup.settle();
  assert.deepEqual(setup.prepared.map(request => [request.id, request.target]),
    [['request-0', 'shell'], ['request-1', 'shell']]);
  assert.equal(setup.view.items[0].status.kind, 'running');
  assert.equal(setup.view.error, null);
});

test('uncertain open retains the original request and never retries automatically', async () => {
  for (const failure of [async () => ({Err:{code:'outcome_unknown'}}), async () => { throw new Error('disconnected'); }]) {
    const setup = await fixture(); setup.host.complete = failure;
    setup.view.refresh(setup.cx); await setup.settle();
    assert.equal(setup.view.error, 'unknown');
    assert.equal(setup.view.pending.id, 'request-0');
    assert.equal(setup.forgotten.length, 0);
    setup.view.refresh(setup.cx); await setup.settle();
    assert.deepEqual(setup.completed, ['request-0']);
    setup.host.complete = null;
    setup.view.retry(setup.cx); await setup.settle();
    assert.equal(setup.prepared.length, 1);
    assert.deepEqual(setup.completed, ['request-0', 'request-0']);
    assert.deepEqual(setup.forgotten, ['request-0']);
    assert.equal(setup.view.pending, null);
  }
});

test('a completed open does not replace a subsequently selected tab', async () => {
  const setup = await fixture([shell('first'), shell('second')], null);
  setup.view.selected = 'first';
  let release;
  setup.host.complete = request => new Promise(resolve => { release = () => {
    setup.host.items = setup.host.items.map(item => item.id === request.target
      ? {...item, status:{kind:'running'}} : item);
    resolve({Ok:{kind:'terminal', data:{id:request.target}}});
  }; });
  setup.view.refresh(setup.cx);
  while (!release) await Promise.resolve();
  setup.view.select(1, setup.cx);
  setup.host.complete = null; release(); await setup.settle();
  assert.equal(setup.view.selected, 'second');
  assert.deepEqual(setup.prepared.map(request => request.target), ['first', 'second']);
  assert.equal(setup.panes.length, 0);
});

test('a subsequent stopped revision reopens the same shell exactly once', async () => {
  const setup = await fixture();
  setup.view.refresh(setup.cx); await setup.settle();
  setup.host.items = setup.host.items.map(item => ({...item, revision:item.revision + 1, status:{kind:'stopped'}}));
  setup.view.refresh(setup.cx); setup.view.refresh(setup.cx); await setup.settle();
  assert.deepEqual(setup.prepared.map(request => [request.id, request.target]),
    [['request-0', 'shell'], ['request-1', 'shell']]);
  assert.equal(setup.view.items.length, 1);
  assert.equal(setup.view.items[0].status.kind, 'running');
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.prepared.length, 2);
});

test('switching after an open failure opens another shell and preserves explicit retry for the failed revision', async () => {
  const setup = await fixture([shell('first'), shell('second')], null);
  setup.view.selected = 'first';
  setup.host.complete = async () => ({Err:{code:'busy'}});
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.view.openRetry, 'first');
  setup.host.complete = null;
  setup.view.select(1, setup.cx); await setup.settle();
  assert.equal(setup.view.selected, 'second');
  assert.equal(setup.view.error, null);
  assert.deepEqual(setup.prepared.map(request => request.target), ['first', 'second']);
  setup.view.select(0, setup.cx); await setup.settle();
  assert.equal(setup.view.error, 'failed');
  assert.equal(setup.view.openRetry, 'first');
  assert.equal(setup.prepared.length, 2);
  setup.view.retry(setup.cx); await setup.settle();
  assert.deepEqual(setup.prepared.map(request => request.target), ['first', 'second', 'first']);
  assert.equal(setup.view.items[0].status.kind, 'running');
});

test('switching shells does not clear unrelated load or action failures', async () => {
  for (const error of ['loadFailed', 'failed']) {
    const setup = await fixture([shell('first'), shell('second')], null);
    setup.view.items = setup.host.items; setup.view.selected = 'first'; setup.view.error = error;
    setup.view.select(1, setup.cx); await setup.settle();
    assert.equal(setup.view.error, error);
    assert.equal(setup.prepared.length, 0);
  }
});

test('a preparation failure retains the open target for an explicit retry', async () => {
  const setup = await fixture(); setup.host.prepareError = new Error('draft capacity exhausted');
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.view.error, 'failed');
  assert.equal(setup.view.openRetry, 'shell');
  assert.equal(setup.view.pending, null);
  assert.equal(setup.prepared.length, 0);
  assert.equal(setup.completed.length, 0);
  setup.host.prepareError = null;
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.prepared.length, 0);
  setup.view.retry(setup.cx); await setup.settle();
  assert.deepEqual(setup.prepared, [{id:'request-0', action:'open', target:'shell'}]);
  assert.equal(setup.view.items[0].status.kind, 'running');
});

test('a pending unknown open keeps the pane busy and rejects new actions until explicit retry', async () => {
  const setup = await fixture(); setup.host.complete = async () => ({Err:{code:'outcome_unknown'}});
  setup.view.refresh(setup.cx); await setup.settle();
  assert.equal(setup.view.busy, false);
  assert.equal(setup.updates.filter(update => Object.hasOwn(update, 'busy')).at(-1).busy, true);
  for (const action of ['close', 'create', 'open']) setup.view.perform(action, 'shell', setup.cx);
  await setup.settle();
  assert.deepEqual(setup.completed, ['request-0']);
  assert.equal(setup.prepared.length, 1);
  assert.equal(setup.view.pending.id, 'request-0');
  setup.host.complete = null;
  setup.view.retry(setup.cx); await setup.settle();
  assert.deepEqual(setup.completed, ['request-0', 'request-0']);
  assert.equal(setup.prepared.length, 1);
  assert.equal(setup.updates.filter(update => Object.hasOwn(update, 'busy')).at(-1).busy, false);
});
