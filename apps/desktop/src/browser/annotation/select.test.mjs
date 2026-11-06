import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('./select.js', import.meta.url), 'utf8');
const escape = value => value.replaceAll(' ', '\\ ');

function setup({id = 'target', text = 'Visible text', duplicate = false, attribute = null, url = 'https://example.com/page'} = {}) {
  const listeners = new Map(), globals = new Map(), messages = [], frames = [];
  let document;
  class Element {
    constructor(name, id = '') {
      Object.assign(this, {localName:name,id,style:{},children:[],parentElement:null,removed:false,
        attributes:new Map(),classList:[],innerText:''});
    }
    getRootNode() { return document; }
    getAttribute(name) { return this.attributes.get(name) ?? null; }
    hasAttribute(name) { return this.attributes.has(name); }
    setAttribute(name, value) { this.attributes.set(name, String(value)); }
    removeAttribute(name) { this.attributes.delete(name); }
    getBoundingClientRect() { return {x:10,y:20,width:160,height:30}; }
    getElementsByTagName() { return []; }
    appendChild(child) { child.parentElement = this; this.children.push(child); return child; }
    remove() { this.removed = true; }
    get firstElementChild() { return this.children[0] ?? null; }
    cloneNode() {
      const copy = new Element(this.localName, this.id);
      copy.attributes = new Map(this.attributes); copy.innerText = this.innerText;
      return copy;
    }
    get outerHTML() {
      const attributes = [...this.attributes].map(([name, value]) => ` ${name}="${value}"`).join('');
      return `<${this.localName}${attributes}>${this.innerText}</${this.localName}>`;
    }
  }
  const walk = node => node.children.filter(child => !child.attributes.has('data-sailry-selection'))
    .flatMap(child => [child, ...walk(child)]);
  const match = (nodes, selector) => {
    if (selector.startsWith('#')) return nodes.filter(node => node.id && `#${escape(node.id)}` === selector);
    const attribute = /^\[([\w-]+)="(.*)"\]$/.exec(selector);
    if (attribute) return nodes.filter(node => node.getAttribute(attribute[1]) !== null
      && escape(node.getAttribute(attribute[1])) === attribute[2]);
    if (/^[\w-]+$/.test(selector)) return nodes.filter(node => node.localName === selector);
    return [];
  };
  const root = new Element('html'), body = new Element('body'), element = new Element('button', id);
  root.appendChild(body); body.appendChild(element);
  element.innerText = text; element.value = 'private input value';
  if (duplicate) body.id = id;
  if (attribute) element.setAttribute('data-testid', attribute);
  document = {
    title:'Example page',documentElement:root,createElement:name => new Element(name),
    getElementById:value => walk(root).find(node => node.id === value) ?? null,
    querySelectorAll:selector => match(walk(root), selector),
    addEventListener(name, callback) { listeners.set(name, callback); },
    removeEventListener(name, callback) { if (listeners.get(name) === callback) listeners.delete(name); }
  };
  const overlays = () => root.children.filter(child => child.attributes.has('data-sailry-selection'));
  const window = {ipc:{postMessage(value) {
    const message = JSON.parse(value);
    if (message.kind === 'element') assert.ok(overlays().every(node => node.removed || node.style.display === 'none'));
    messages.push(message);
  }}};
  vm.runInNewContext(`(${source})('selection-token')`, {document,window,Element,TextEncoder,performance,
    CSS:{escape},location:{href:url},innerWidth:640,innerHeight:480,
    requestAnimationFrame(callback) { frames.push(callback); },
    setTimeout:() => 1,clearTimeout() {},
    getComputedStyle:() => ({getPropertyValue:name => ({color:'rgb(0, 0, 0)',display:'block'})[name] ?? ''}),
    addEventListener(name, callback) { globals.set(name, callback); },
    removeEventListener(name, callback) { if (globals.get(name) === callback) globals.delete(name); }
  });
  return {messages,listeners,globals,root,body,element,window,Element,overlays,
    fire(name, target = element, values = {}) {
      let prevented = false, stopped = false;
      listeners.get(name)?.({target,button:0,key:'',shiftKey:false,altKey:false,...values,
        preventDefault() { prevented = true; },stopImmediatePropagation() { stopped = true; }});
      return {prevented,stopped};
    },
    paint() { while (frames.length) frames.shift()(); }
  };
}

test('picks stay active and hide overlays during capture', () => {
  const fixture = setup({id:'unique target'});
  fixture.fire('pointermove');
  assert.equal(fixture.overlays()[0].style.display, 'block');
  assert.equal(fixture.overlays()[1].textContent, 'button#unique target  160 × 30');
  assert.deepEqual(fixture.fire('pointerdown'), {prevented:true,stopped:true});
  assert.deepEqual(fixture.fire('click'), {prevented:true,stopped:true});
  assert.equal(fixture.messages.length, 0);
  fixture.paint();
  assert.equal(fixture.messages.length, 1);
  const message = fixture.messages[0];
  assert.equal(message.token, 'selection-token');
  assert.equal(message.review, false);
  assert.equal(message.element.selector, '#unique\\ target');
  assert.equal(message.element.text, 'Visible text');
  assert.equal(message.element.tag, 'button');
  assert.equal(message.element.title, 'Example page');
  assert.equal(message.element.path, 'html > body > button#unique target');
  assert.deepEqual(message.element.styles, {color:'rgb(0, 0, 0)',display:'block'});
  assert.deepEqual(message.element.bounds, [10,20,160,30]);
  assert.deepEqual(message.element.viewport, [640,480]);
  assert.equal(message.element.url, 'https://example.com/page');
  assert.equal(message.element.component, null);
  assert.equal(message.element.truncated, false);
  assert.ok(fixture.listeners.has('click'));
  fixture.fire('click'); fixture.paint();
  assert.equal(fixture.messages.length, 1);
  fixture.window.__sailryElementSelection.resume('other-token');
  assert.equal(fixture.overlays()[0].style.display, 'none');
  fixture.window.__sailryElementSelection.resume('selection-token');
  assert.equal(fixture.overlays()[0].style.display, 'block');
  fixture.fire('click'); fixture.paint();
  assert.equal(fixture.messages.length, 2);
});

test('shift selections commit one message per element', () => {
  const fixture = setup();
  const link = fixture.body.appendChild(new fixture.Element('a', 'link'));
  const label = fixture.body.appendChild(new fixture.Element('span'));
  fixture.fire('click', fixture.element, {shiftKey:true});
  fixture.fire('click', link, {shiftKey:true});
  fixture.fire('click', link, {shiftKey:true});
  fixture.fire('click', link, {shiftKey:true});
  assert.equal(fixture.overlays().filter(node => !node.removed).length, 4);
  fixture.fire('click', label); fixture.paint();
  assert.deepEqual(fixture.messages.map(message => message.element.selector), ['#target','#link','span']);
  assert.ok(fixture.messages.every(message => !message.review));
  assert.equal(fixture.overlays().filter(node => !node.removed).length, 2);
});

test('alternate pick requests review and ends picking', () => {
  const fixture = setup();
  fixture.fire('click', fixture.element, {altKey:true}); fixture.paint();
  assert.equal(fixture.messages.length, 1);
  assert.equal(fixture.messages[0].review, true);
  assert.equal(fixture.listeners.size, 0);
  assert.equal(fixture.globals.size, 0);
  assert.equal(fixture.window.__sailryElementSelection, undefined);
});

test('arrow keys move between parent and child', () => {
  const fixture = setup();
  fixture.fire('pointermove');
  assert.deepEqual(fixture.fire('keydown', fixture.element, {key:'ArrowUp'}), {prevented:true,stopped:true});
  fixture.fire('keydown', fixture.element, {key:'ArrowDown'});
  fixture.fire('keydown', fixture.element, {key:'ArrowUp'});
  fixture.fire('keydown', fixture.element, {key:'Enter'}); fixture.paint();
  assert.equal(fixture.messages[0].element.tag, 'body');
  assert.equal(fixture.messages[0].element.selector, 'body');
});

test('escape removes selection listeners without capturing', () => {
  const fixture = setup();
  fixture.fire('keydown', fixture.element, {key:'Escape'});
  assert.deepEqual(fixture.messages, [{kind:'cancelled',token:'selection-token'}]);
  assert.equal(fixture.listeners.size, 0);
  assert.ok(fixture.overlays().every(node => node.removed));
  fixture.fire('click'); fixture.paint();
  assert.equal(fixture.messages.length, 1);
});

test('duplicate identifiers use a unique stable attribute', () => {
  const fixture = setup({duplicate:true,attribute:'submit button'});
  fixture.fire('click'); fixture.paint();
  assert.equal(fixture.messages[0].element.selector, '[data-testid="submit\\ button"]');
});

test('repeated structures fall back to the nearest selector', () => {
  const fixture = setup({id:'',duplicate:false});
  fixture.body.appendChild(new fixture.Element('button'));
  fixture.fire('click'); fixture.paint();
  assert.equal(fixture.messages[0].element.selector, 'html > body > button:nth-of-type(1)');
});

test('shadow content keeps a piercing host selector', () => {
  const fixture = setup({id:''});
  const host = fixture.body.appendChild(new fixture.Element('my-card', 'card'));
  const inner = new fixture.Element('button');
  const shadow = {host,children:[inner],querySelectorAll:selector => selector === 'button' ? [inner] : []};
  inner.getRootNode = () => shadow;
  fixture.fire('pointermove', inner);
  fixture.fire('keydown', inner, {key:'Enter'}); fixture.paint();
  assert.equal(fixture.messages[0].element.selector, '#card >>> button');
  fixture.window.__sailryElementSelection.resume('selection-token');
  fixture.fire('keydown', inner, {key:'ArrowUp'});
  fixture.fire('keydown', inner, {key:'Enter'}); fixture.paint();
  assert.equal(fixture.messages[1].element.selector, '#card');
});

test('text is bounded without exposing input values', () => {
  const fixture = setup({text:'中'.repeat(800)});
  fixture.element.setAttribute('type', 'password');
  fixture.element.setAttribute('value', 'private input value');
  fixture.fire('click'); fixture.paint();
  const element = fixture.messages[0].element;
  assert.equal(element.truncated, true);
  assert.equal(element.text, '中'.repeat(500));
  assert.deepEqual(element.attributes, {type:'password'});
  assert.equal(JSON.stringify(element).includes('private'), false);
});

test('messages fit the native IPC limit', () => {
  const fixture = setup({url:`https://example.com/${'a'.repeat(8000)}`});
  for (const name of ['role','type','name','href','src','alt','title','placeholder','for','aria-label',
    'aria-describedby','aria-expanded','aria-selected','aria-checked','aria-disabled','aria-current']) {
    // Control characters expand sixfold in JSON.
    fixture.element.setAttribute(name, '\u0001'.repeat(512));
  }
  fixture.element.innerText = '"'.repeat(2000);
  fixture.fire('click'); fixture.paint();
  const [message] = fixture.messages;
  assert.ok(Buffer.byteLength(JSON.stringify(message)) <= 31 * 1024);
  assert.equal(message.element.truncated, true);
  assert.equal(message.element.html, '');
  assert.deepEqual(message.element.attributes, {});
});

test('development components report their names and sources', () => {
  const fixture = setup();
  function PayButton() {}
  function App() {}
  fixture.element['__reactFiber$test'] = {_debugSource:{fileName:'src/Pay.tsx',lineNumber:12},
    _debugOwner:{type:PayButton,_debugOwner:{type:{displayName:'Checkout'},_debugOwner:{type:App}}}};
  fixture.fire('pointermove');
  assert.equal(fixture.overlays()[1].textContent, 'PayButton  160 × 30');
  fixture.fire('click'); fixture.paint();
  assert.deepEqual(fixture.messages[0].element.component,
    {framework:'react',name:'PayButton',source:'src/Pay.tsx:12',stack:['PayButton','Checkout','App']});
  const vue = setup();
  vue.element.__vueParentComponent = {type:{__file:'src/components/Card.vue'},parent:{type:{name:'Shell'}}};
  vue.fire('click'); vue.paint();
  assert.deepEqual(vue.messages[0].element.component,
    {framework:'vue',name:'Card',source:'src/components/Card.vue',stack:['Card','Shell']});
  const production = setup();
  production.element['__reactFiber$test'] = {return:{type:function e() {}}};
  production.fire('click'); production.paint();
  assert.equal(production.messages[0].element.component, null);
});
