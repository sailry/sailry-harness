import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const element = new Proxy({}, {get:() => () => element});
let picker;
let checkbox;
// The pinned component-shell contract exposes semantic size and boolean changes.
class Checkbox {
  constructor(id) {this.id=id;checkbox=this;}
  size(value) {assert(['xsmall','small','medium','large'].includes(value));this.controlSize=value;return this;}
  label(value) {this.text=value;return this;}
  checked(value) {assert.equal(typeof value,'boolean');this.value=value;return this;}
  disabled(value) {this.readonly=value;return this;}
  on_change(callback) {this.change=callback;return this;}
}
const {render} = vm.runInNewContext(`${source}\n({render})`, {
  div:() => element,
  Button:class {constructor(){return element;}},
  Checkbox,
  TextField:{new:() => element},
  window:{viewport_size:() => ({width:800,height:600})},
  Picker:{new:(_,props) => {picker=props;return element;}},
  Modal:{new:() => element},
});

test('picker descriptors always encode disabled as a boolean', () => {
  const owner = {kind:'list',token:'picker',pending:false,loading:false,
    choices:[{id:'available',label:'Available'},{id:'locked',label:'Locked',disabled:true}]};
  render({model:{dialog:owner},text:key => key});
  assert.deepEqual(Array.from(picker.items,item => item.disabled),[false,true]);
  owner.pending=true;
  render({model:{dialog:owner},text:key => key});
  assert.deepEqual(Array.from(picker.items,item => item.disabled),[true,true]);
});

test('managed checkbox uses semantic size and boolean changes', () => {
  const owner={kind:'managed',token:'managed',pending:false,request:null,status:{},fields:{branch:'branch'},include_changes:true};
  const view={model:{dialog:owner},text:key => key};let notifications=0;
  render(view);
  assert.equal(checkbox.id,'location-copy-changes');assert.equal(checkbox.controlSize,'small');
  assert.equal(checkbox.value,true);assert.equal(checkbox.readonly,false);
  checkbox.change(false,{notify:()=>{notifications++;}});assert.equal(owner.include_changes,false);
  render(view);assert.equal(checkbox.value,false);
  checkbox.change(true,{notify:()=>{notifications++;}});assert.equal(owner.include_changes,true);
  assert.equal(notifications,2);
});

test('managed checkbox preserves readonly state', () => {
  for (const pending of [false,true]) {
    const owner={kind:'managed',token:'managed',pending,request:pending ? null : 'admitted',status:{},fields:{branch:'branch'},include_changes:true};
    render({model:{dialog:owner},text:key => key});assert.equal(checkbox.readonly,true);
    checkbox.change(false,{notify:()=>{assert.fail('readonly control notified');}});
    assert.equal(owner.include_changes,true);
  }
});
