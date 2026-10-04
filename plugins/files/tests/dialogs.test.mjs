import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/dialogs.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');

function element(kind,props = {}) {
  const node = {kind,props,items:[]};
  const api = new Proxy(node,{get(target,key) {
    if (key in target) return target[key];
    return (...args) => {
      if (key === 'child') target.items.push(args[0]);
      else if (key === 'children') target.items.push(...args[0]);
      else target.props[key] = args.length === 1 ? args[0] : args;
      return api;
    };
  }});
  return api;
}

function render(value,paths = []) {
  const transfer = vm.runInNewContext(`${source}\ntransfer`,{
    div:() => element('div'),theme:() => ({colors:{}}),window:{viewport_size:() => ({width:900})},
    Modal:{new:(id,props) => element('modal',{id,...props})},TextField:{new:(id,props) => element('input',{id,...props})},
    Button:function(id) {return element('button',{id});},Progress:function(id) {return element('progress',{id});},
    status:() => null,parent:path => path.slice(0,path.lastIndexOf('/')),
  });
  return transfer({text:{files_paste_source:'Source',files_paste_directory:'Folder',files_paste:'Paste',files_upload:'Upload'},
    transfers:{value:() => value,queue:{paths},busy:false},transferInput:'name'});
}

function all(node) { return typeof node === 'object' ? [node,...node.items.flatMap(all)] : [node]; }

test('copy dialogs retain the captured source and remaining batch while the destination is edited', () => {
  const nodes = all(render({id:'one',kind:'copy',stage:'ready',path:'target/renamed.txt',
    source_label:'Other project',source_path:'nested/original.txt',can_start:true},[{path:'next.txt'}]));
  for (const text of ['Other project','nested/original.txt','next.txt','target']) assert.ok(nodes.includes(text),text);
  assert.equal(nodes.find(node => node?.kind === 'input').props.disabled,false);
  assert.equal(nodes.some(node => node?.kind === 'progress'),false);
});

test('native progress switches between indeterminate phases and transfer percentages without byte-count copy', () => {
  const value = {id:'one',kind:'upload',path:'target.txt',source_path:'picked.txt'};
  for (const [stage,progress,loading,percentage] of [
    ['preparing',null,true,0],['transferring',{copied:3,size:4},false,75],
    ['publishing',{copied:4,size:4},true,0],['done',{copied:4,size:4},false,100],
  ]) {
    const nodes = all(render({...value,stage,progress}));
    const bar = nodes.find(node => node?.kind === 'progress');
    assert.equal(bar.props.loading,loading,stage); assert.equal(bar.props.value,percentage,stage);
    assert.ok(nodes.includes('picked.txt'));
    assert.equal(nodes.includes('3 / 4'),false);
  }
});
