import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import {messages} from '../dev.sailry.platform/desktop/locales.js';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/view.js', import.meta.url), 'utf8'))
  .replace(/^import .*;\n/gm, '').replace(/^export /gm, '');

function fixture(item, error = null) {
  const nodes = new Map(), calls = [];
  function element(kind = 'div', id = null, props = {}) {
    const node = {kind, id, props, items:[], styles:{}, handlers:{}};
    const proxy = new Proxy(node, {get(target, key) {
      if (key === 'id') return id => { target.id = id; nodes.set(id, proxy); return proxy; };
      if (key === 'child') return child => { target.items.push(child); return proxy; };
      if (key === 'children') return children => { target.items.push(...children); return proxy; };
      if (key === 'on_click' || key === 'on_change') return callback => { target.handlers[key] = callback; return proxy; };
      if (key in target) return target[key];
      return (...args) => { target.styles[key] = args; return proxy; };
    }});
    if (id) nodes.set(id, proxy);
    return proxy;
  }
  const constructors = Object.fromEntries(['Button', 'Tab', 'TabBar'].map(kind =>
    [kind, function(id) { return element(kind, id); }]));
  const {render} = vm.runInNewContext(`${source}\n({render})`, {
    div:() => element(), ...constructors, theme:() => ({colors:{destructive:'danger'}}),
    Terminal:{new:(id, props) => element('Terminal', id, props)},
    Menu:{new:(id, props) => element('Menu', id, props)},
  });
  const view = {items:[item], selected:item.id, resource:{id:item.id}, text:messages('en'),
    busy:!error, pending:null, openRetry:error === 'failed' ? item.id : null, error, retry:() => calls.push('retry')};
  const root = render(view);
  const text = node => typeof node === 'string' ? node : node.items.map(text).join(' ');
  return {nodes, calls, text:text(root)};
}

test('stopped ordinary shells show loading without a stopped or open interstitial', () => {
  const setup = fixture({id:'shell', tool:null, ssh:null, status:{kind:'stopped'}});
  assert.match(setup.text, /Loading/);
  assert.doesNotMatch(setup.text, /Terminal stopped/);
  assert.equal(setup.nodes.has('terminal-resume'), false);
  assert.equal(setup.nodes.get('commands-terminal').props.terminal, null);
});

test('an actual open error keeps retry visible without claiming a running terminal', () => {
  const setup = fixture({id:'shell', tool:null, ssh:null, status:{kind:'stopped'}}, 'failed');
  assert.doesNotMatch(setup.text, /Terminal action failed/);
  assert.doesNotMatch(setup.text, /Loading|Terminal stopped/);
  setup.nodes.get('terminal-retry').handlers.on_click();
  assert.deepEqual(setup.calls, ['retry']);
  assert.equal(setup.nodes.get('commands-terminal').props.terminal, null);
});

test('stopped CLI and SSH terminals remain stopped without automatic launch controls', () => {
  for (const extra of [{tool:'claude'}, {ssh:'saved-host'}]) {
    const setup = fixture({id:'external', ...extra, status:{kind:'stopped'}});
    assert.match(setup.text, /Terminal stopped/);
    assert.equal(setup.nodes.has('terminal-resume'), false);
  }
});

test('catalog failures do not add a retry bar or claim an attached terminal', () => {
  const setup = fixture({id:'shell', tool:null, ssh:null, status:{kind:'stopped'}}, 'loadFailed');
  assert.equal(setup.nodes.has('terminal-retry'), false);
  assert.equal(setup.nodes.get('commands-terminal').props.terminal, null);
});
