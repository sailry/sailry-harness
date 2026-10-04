import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const plain = value => JSON.parse(JSON.stringify(value));

function fixture() {
  const nodes = new Map(),calls = [];
  function element(kind='div',id=null,props={}) {
    const value = {kind,selector:id,props,items:[],style:{}};
    const proxy = new Proxy(value,{get(target,key) {
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
  const functions = vm.runInNewContext(`${source}\n({catalog,configuration,editor})`,{
    div:()=>element(),theme:()=>({colors:{muted_foreground:'muted'}}),
    Button:function(id){return element('Button',id);},
    Switch:function(id){return element('Switch',id);},
    Field:function(){return element('Field');},
    DropdownMenu:function(id){return element('DropdownMenu',id);},
    SelectField:{new:(id,props)=>element('SelectField',id,props)},
    SegmentedTabs:{new:(id,props)=>element('SegmentedTabs',id,props)},
    TextField:{new:(id,props)=>element('TextField',id,props)},
    SettingsGroup:{new:(id,props)=>element('SettingsGroup',id,props)},
    EmptyState:{new:(id,props)=>element('EmptyState',id,props)},
    window:{viewport_size:()=>({width:1280,height:820})},
  });
  const view = {text:{settings_memory:'Memories',memory_empty:'No memories',settings_edit:'Edit',settings_delete:'Delete'},
    results:[],reviews:[],loading:false,busy:()=>false,kindLabel:()=> 'Feedback',projectLabel:()=> 'Global',
    edit:entry=>calls.push(['edit',entry]),remove:entry=>calls.push(['remove',entry])};
  return {view,nodes,calls,functions,render(){nodes.clear();return functions.catalog(view);}};
}

test('settings choices use real selects and editor tabs fill their form control',()=>{
  const setup=fixture(),view=setup.view;
  view.text={...view.text,memory_context_budget:'Budget',memory_review_age:'Review',memory_title:'Title',memory_kind:'Type',memory_body:'Content'};
  view.draft={enabled:true,auto_write:true,context_bytes:8192,review_after_days:90};
  view.configuration={pending:false,request:null,error:null};view.connected=true;
  setup.functions.configuration(view);
  const budget=setup.nodes.get('memory-budget'),age=setup.nodes.get('memory-review-age');
  assert.equal(budget.kind,'SelectField');assert.equal(age.kind,'SelectField');
  assert.equal(budget.props.selected,'8');assert.equal(age.props.selected,'90');
  assert.deepEqual(plain(budget.props.items.map(item=>item.id)),['2','4','8','16','32','64']);
  view.dialogId=4;view.entries=[];
  view.editing={entry:{summary:{id:'draft',kind:'feedback',project:null,revision:0}},title:'title',body:'body',sources:[],loading:false,pending:false,request:null};
  setup.functions.editor(view);
  const tabs=setup.nodes.get('memory-kind-4');
  assert.equal(tabs.kind,'SegmentedTabs');assert.equal(tabs.props.selected,'feedback');
  assert.deepEqual(plain(tabs.props.items.map(item=>item.id)),['user','feedback','project','reference']);
  view.configuration.pending=true;setup.functions.configuration(view);
  assert.equal(setup.nodes.get('memory-budget').props.disabled,true);
  assert.equal(setup.nodes.get('memory-review-age').props.disabled,true);
});

test('empty catalogs reuse their existing settings card and loading hides the placeholder',()=>{
  const setup=fixture(),group=setup.render();
  assert.equal(group.kind,'SettingsGroup');
  assert.deepEqual(plain(group.props),{heading:false,loading:false});
  const empty=setup.nodes.get('memory-empty');
  assert.deepEqual(plain(empty.props),{variant:'list',icon:'inbox',label:'No memories'});
  assert.equal(group.items[0],empty);
  setup.view.loading=true;setup.render();
  assert.equal(setup.nodes.has('memory-empty'),false);
});

test('populated catalogs preserve captured edit and remove actions without empty content',()=>{
  const setup=fixture(),entry={id:'retained',title:'Retained memory',kind:'feedback',project:null};
  setup.view.results=[entry];setup.render();
  assert.equal(setup.nodes.has('memory-empty'),false);
  assert.ok(setup.nodes.has('memory-retained'));
  setup.nodes.get('memory-edit-retained').on_click({},{});
  setup.nodes.get('memory-delete-retained').on_click({},{});
  assert.deepEqual(setup.calls,[['edit',entry],['remove',entry]]);
});

test('catalog failures keep existing memories without a retry bar',()=>{
  const setup=fixture();setup.view.error='memory_failed';
  setup.view.results=[{id:'retained',title:'Retained memory',kind:'feedback',project:null}];setup.render();
  assert.equal(setup.nodes.has('memory-search-retry'),false);
  assert.ok(setup.nodes.has('memory-retained'));
});
