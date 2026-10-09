import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('./select.js', import.meta.url), 'utf8');

function setup({id = 'target',text = 'Visible text',duplicate = false,attribute = null} = {}) {
  const listeners = new Map(), messages = [], frames = [], selectors = [];
  let document;
  class Element {
    constructor(name, id = '') { this.localName = name; this.id = id; this.style = {}; this.children = []; this.removed = false; }
    getRootNode() { return document; }
    getAttribute(name) { return name === 'data-testid' ? attribute : null; }
    setAttribute() {}
    getBoundingClientRect() { return {x:10,y:20,width:160,height:30}; }
    appendChild(child) { child.parentElement = this; this.children.push(child); }
    remove() { this.removed = true; }
  }
  const root = new Element('html'), body = new Element('body'), element = new Element('button', id);
  root.appendChild(body); body.appendChild(element); element.innerText = text; element.value = 'private input value';
  document = {
    documentElement:root, createElement:name => new Element(name),
    querySelectorAll(selector) {
      selectors.push(selector);
      if (selector.startsWith('#')) return duplicate ? [element,body] : [element];
      if (selector.startsWith('[data-testid=')) return [element];
      if (selector === 'button') return [element];
      return [];
    },
    addEventListener(name, callback) { listeners.set(name,callback); },
    removeEventListener(name, callback) { if (listeners.get(name) === callback) listeners.delete(name); }
  };
  const window = {ipc:{postMessage(value) { assert.equal(root.children.at(-1).removed,true); messages.push(JSON.parse(value)); }}};
  vm.runInNewContext(`(${source})('selection-token')`, {document,window,Element,TextEncoder,
    CSS:{escape(value) { return value.replaceAll(' ', '\\ '); }},
    location:{href:'https://example.com/page'},innerWidth:640,innerHeight:480,
    requestAnimationFrame(callback) { frames.push(callback); }
  });
  return {messages,listeners,selectors,element,window,
    event(name, values = {}) {
      let prevented = false, stopped = false;
      listeners.get(name)?.({target:element,button:0,...values,
        preventDefault() { prevented = true; },stopImmediatePropagation() { stopped = true; }});
      return {prevented,stopped};
    },
    paint() { while (frames.length) frames.shift()(); }
  };
}

test('selection blocks page actions and removes highlight before capture', () => {
  const fixture = setup({id:'unique target'});
  assert.deepEqual(fixture.event('pointerdown'), {prevented:true,stopped:true});
  assert.deepEqual(fixture.event('click'), {prevented:true,stopped:true});
  assert.equal(fixture.messages.length,0);
  assert.equal(fixture.listeners.size,0);
  fixture.paint();
  assert.equal(fixture.messages.length,1);
  const message = fixture.messages[0];
  assert.equal(message.token,'selection-token');
  assert.equal(message.element.selector,'#unique\\ target');
  assert.equal(message.element.text,'Visible text');
  assert.deepEqual(message.element.bounds,[10,20,160,30]);
  assert.deepEqual(message.element.viewport,[640,480]);
  assert.equal(message.element.url,'https://example.com/page');
  assert.equal(message.element.truncated,false);
});

test('duplicate identifiers use a unique stable attribute', () => {
  const fixture = setup({duplicate:true,attribute:'submit button'});
  fixture.event('click'); fixture.paint();
  assert.equal(fixture.messages[0].element.selector,'[data-testid="submit\\ button"]');
});

test('unicode text is bounded without exposing input values', () => {
  const fixture = setup({text:'中'.repeat(8000)});
  fixture.event('click'); fixture.paint();
  const element = fixture.messages[0].element;
  assert.equal(element.truncated,true);
  assert.equal(element.text,'中'.repeat(5461));
  assert.ok(Buffer.byteLength(element.text) <= 16384);
  assert.equal(element.text.includes('private'),false);
});

test('escape removes selection listeners without capturing', () => {
  const fixture = setup();
  fixture.event('keydown',{key:'Escape'});
  assert.deepEqual(fixture.messages,[{kind:'cancelled',token:'selection-token'}]);
  assert.equal(fixture.listeners.size,0);
  fixture.event('click'); fixture.paint();
  assert.equal(fixture.messages.length,1);
});

test('a shadow root never yields a misleading document selector', () => {
  const fixture = setup();
  fixture.element.getRootNode = () => ({});
  fixture.event('click'); fixture.paint();
  assert.deepEqual(fixture.messages,[{kind:'cancelled',token:'selection-token'}]);
});
