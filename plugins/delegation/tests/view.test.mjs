import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const plain = value => JSON.parse(JSON.stringify(value));

function fixture() {
  const nodes=new Map(),calls=[];
  function element(kind='div',id=null,props={}) {
    const value={kind,selector:id,props,items:[],style:{}};
    const proxy=new Proxy(value,{get(target,key) {
      if(key==='id')return id=>{target.selector=id;nodes.set(id,proxy);return proxy;};
      if(key==='child')return child=>{target.items.push(child);return proxy;};
      if(key==='children')return children=>{target.items.push(...children);return proxy;};
      if(key in target)return target[key];
      if(key==='on_click')return callback=>{target.on_click=callback;return proxy;};
      return (...args)=>{target.style[key]=args;return proxy;};
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const adapters=Object.fromEntries(['EmptyState','SettingsGroup','IconButton','Appearance','Modal']
    .map(kind=>[kind,{new:(id,props)=>element(kind,id,props)}]));
  const {render}=vm.runInNewContext(`${source}\n({render})`,{
    div:()=>element(),Button:function(id){return element('Button',id);},...adapters,
    theme:()=>({colors:{muted_foreground:'muted'}}),
  });
  const view={text:{settings_empty:'No roles',settings_add:'Add',settings_edit:'Edit',settings_delete:'Delete',roles_profiles:'Roles'},
    roles:[],ready:true,connected:true,error:null,dialog:null,edit:role=>calls.push(['edit',role])};
  return {view,nodes,calls,render(){nodes.clear();return render(view);}};
}

test('empty roles reuse their settings card and retain the add action',()=>{
  const setup=fixture();setup.render();
  const group=setup.nodes.get('roles_profiles'),empty=setup.nodes.get('role-empty');
  assert.deepEqual(plain(group.props),{title:'Roles',header_action:true});
  assert.deepEqual(plain(empty.props),{variant:'list',icon:'inbox',label:'No roles'});
  assert.equal(group.items[1],empty);
  setup.nodes.get('role-add').items[0].on_click({},{});
  assert.deepEqual(setup.calls,[['edit',null]]);
});

test('populated roles retain native appearance and action targets without an empty state',()=>{
  const setup=fixture(),appearance={icon:'code',color:'blue'};
  setup.view.roles=[{id:'retained',name:'Retained role',key:'review',appearance}];setup.render();
  assert.equal(setup.nodes.has('role-empty'),false);
  assert.deepEqual(plain(setup.nodes.get('role-icon-retained').props),{value:appearance});
  assert.deepEqual(plain(setup.nodes.get('role-edit-retained').props),{icon:'settings-2',label:'Edit',disabled:false});
  assert.deepEqual(plain(setup.nodes.get('role-delete-retained').props),{icon:'circle-x',label:'Delete',disabled:false});
});

test('catalog failures keep the role list without a retry bar',()=>{
  const setup=fixture();setup.view.error='role_failed';
  setup.view.roles=[{id:'retained',name:'Retained role',key:'review',appearance:{icon:'code',color:'blue'}}];
  setup.render();
  assert.equal(setup.nodes.has('role-reload'),false);
  assert.ok(setup.nodes.has('role-edit-retained'));
  assert.ok(setup.nodes.has('role-delete-retained'));
});
