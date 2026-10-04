import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

async function fixture(name) {
  const nodes=[];
  class Element {
    constructor(type,id=null){this.type=type;this.id=id;this.nodes=[];this.style={};nodes.push(this);}
    child(node){this.nodes.push(node);return this;}
    children(values){for(const node of values)this.child(node);return this;}
  }
  const styles=['v_flex','h_flex','gap_2','gap_3','gap_4','w','w_full','max_h','min_h_0','overflow_y_scroll','py_1','flex_wrap','text_sm','text_lg','font_semibold','justify_end'];
  for(const method of styles)Element.prototype[method]=function(...args){this.style[method]=args;return this;};
  const div=()=>{
    const value=new Element('div');
    value.id=id=>{value.selector=id;return value;};
    return value;
  };
  class Field extends Element {
    constructor(){super('Field');}
    label(value){assert.equal(typeof value,'string');this.labelText=value;return this;}
  }
  class VForm extends Element {
    constructor(){super('VForm');}
    child(value){assert.equal(value.type,'Field','pinned forms accept typed Field children only');return super.child(value);}
  }
  class Button extends Element {
    constructor(id){super('Button',id);}
    label(value){assert.equal(typeof value,'string');this.labelText=value;return this;}
    primary(){this.primaryButton=true;return this;}
    disabled(value){assert.equal(typeof value,'boolean');this.disabledValue=value;return this;}
    on_click(callback){this.click=callback;return this;}
  }
  class Checkbox extends Element {
    constructor(id){super('Checkbox',id);}
    label(value){this.labelText=value;return this;}
    checked(value){assert.equal(typeof value,'boolean');this.checkedValue=value;return this;}
    disabled(value){assert.equal(typeof value,'boolean');this.disabledValue=value;return this;}
    on_change(callback){this.change=callback;return this;}
  }
  class Radio extends Element {
    constructor(id){super('Radio',id);}
    label(value){this.labelText=value;return this;}
  }
  class HorizontalRadioGroup extends Element {
    constructor(id){super('HorizontalRadioGroup',id);}
    child(value){assert.equal(value.type,'Radio');return super.child(value);}
    selected_index(value){this.selected=value;return this;}
    disabled(value){assert.equal(typeof value,'boolean');this.disabledValue=value;return this;}
    on_change(callback){this.change=callback;return this;}
  }
  const native=type=>({new(id,props){const value=new Element(type,id);value.props=props;return value;}});
  const policy=await import(`../../${name}/dev.sailry.platform/desktop/editor.js`);
  const {messages}=await import(`../../${name}/dev.sailry.platform/desktop/locales.js`);
  const key={file:null,picking:false};
  const exports={
    'gpui-kit':{div},'gpui-component':{Button,Checkbox,HorizontalRadioGroup,Radio,VForm,Field},
    'sailry/ui':{SelectField:native('SelectField'),SegmentedTabs:native('SegmentedTabs')},
    'sailry/forms':{TextField:native('TextField')},
    'sailry/credentials':{SecretField:native('SecretField'),describeSecret:()=>key},
    './editor.js':{setSharing:policy.setSharing},
  };
  const context=vm.createContext({window:{viewport_size:()=>({width:1440,height:940})}});
  const module=new vm.SourceTextModule(await readFile(new URL(`../../${name}/dev.sailry.platform/desktop/editor-view.js`,import.meta.url),'utf8'),{context});
  await module.link(specifier=>new vm.SyntheticModule(Object.keys(exports[specifier]),function(){for(const [key,value]of Object.entries(exports[specifier]))this.setExport(key,value);},{context}));
  await module.evaluate();
  const original=name==='ssh'
    ?{id:'connection',revision:3,name:'Saved',host:'host',port:22,username:'user',authentication:'password',sharing:{scope:'projects',projects:['one']}}
    :{id:'connection',revision:3,name:'Saved',connection:{kind:'ssh',engine:'postgres',host:'host',port:5432,database:'rows',username:'user',tls:'require',ssh:'ssh-one'},read_only:true,sharing:{scope:'projects',projects:['one']}};
  const owner=policy.draft(original);owner.inputs=owner.fields.map((_,index)=>`input-${index}`);owner.key='key';owner.password='password';owner.passphrase='passphrase';owner.secret='secret';
  const view={editor:owner,text:messages('en'),dialog:7,connected:true,pending:null,
    projects:[{id:'one',name:'First'},{id:'two',name:'Second'}],ssh:[{id:'ssh-one',name:'Tunnel'}],
    locked(){return !this.connected||!!this.pending;},close(){},save(){},chooseKey(){},changeEngine(){}};
  const cx={notify(){}};
  return {view,owner,key,cx,render(){nodes.length=0;const body=module.namespace.editor(view);module.namespace.editorFooter(view);return body;},
    node(type,id){return nodes.find(node=>node.type===type&&(!id||node.id===id));},nodes};
}
const plain=value=>JSON.parse(JSON.stringify(value));

for(const name of ['ssh','databases']) {
  const prefix=name==='ssh'?'ssh':'db';
  test(`${name} editors use typed forms and original sharing controls`,async()=>{
    const setup=await fixture(name);setup.render();
    const forms=setup.nodes.filter(node=>node.type==='VForm');
    assert.equal(forms.length,name==='ssh'?2:3);
    assert.ok(forms.every(form=>form.nodes.every(field=>field.type==='Field')));
    assert.equal(forms[0].nodes[0].labelText,'Name');
    assert.equal(setup.node('HorizontalRadioGroup',`${prefix}-sharing`).selected,2);
    assert.equal(setup.node('Checkbox',`${prefix}-sharing-one`).checkedValue,true);
    assert.equal(setup.node('Checkbox',`${prefix}-sharing-two`).checkedValue,false);
    assert.equal(setup.nodes.filter(node=>node.type==='Field').length,forms.reduce((count,form)=>count+form.nodes.length,0));
    assert.ok(setup.nodes.some(node=>node.type==='SegmentedTabs'&&node.id.endsWith('-7')));
  });
  test(`${name} sharing retains selections across scope changes and boolean toggles`,async()=>{
    const setup=await fixture(name);setup.render();
    const scope=setup.node('HorizontalRadioGroup',`${prefix}-sharing`);
    for(const index of [2,1,0,2])scope.change(index,setup.cx);
    assert.deepEqual(plain(setup.owner.sharing),{scope:'projects',projects:['one']});
    const second=setup.node('Checkbox',`${prefix}-sharing-two`);
    second.change(true,setup.cx);second.change(true,setup.cx);
    assert.deepEqual(plain(setup.owner.sharing.projects),['one','two']);
    second.change(false,setup.cx);
    assert.deepEqual(plain(setup.owner.sharing.projects),['one']);
  });
  test(`${name} controls reject pending disconnected and replaced editor callbacks`,async()=>{
    for(const state of ['pending','disconnected','replaced']) {
      const setup=await fixture(name);setup.render();
      const scope=setup.node('HorizontalRadioGroup',`${prefix}-sharing`),project=setup.node('Checkbox',`${prefix}-sharing-two`);
      if(state==='pending')setup.view.pending={};
      if(state==='disconnected')setup.view.connected=false;
      if(state==='replaced')setup.view.editor={...setup.owner};
      scope.change(1,setup.cx);project.change(true,setup.cx);
      assert.deepEqual(plain(setup.owner.sharing),{scope:'projects',projects:['one']},state);
      if(state!=='replaced') {
        setup.render();
        assert.equal(setup.node('HorizontalRadioGroup',`${prefix}-sharing`).disabledValue,true);
        assert.equal(setup.node('Checkbox',`${prefix}-sharing-two`).disabledValue,true);
        assert.ok(setup.nodes.filter(node=>['TextField','SecretField','SegmentedTabs','SelectField'].includes(node.type)).every(node=>node.props.disabled),state);
      }
    }
  });
  test(`${name} closed editor actions do not target a replacement draft`,async()=>{
    const setup=await fixture(name);setup.render();
    const buttons=setup.nodes.filter(node=>node.type==='Button'),calls=[];
    setup.view.save=()=>calls.push('save');setup.view.close=()=>calls.push('close');setup.view.chooseKey=()=>calls.push('choose');
    setup.view.editor={...setup.owner};
    for(const button of buttons)button.click?.({},setup.cx);
    assert.deepEqual(calls,[]);
  });
}

test('SSH key-file import locks Save authentication and Choose without exposing contents',async()=>{
  const setup=await fixture('ssh');setup.owner.authentication='key_path';setup.key.file='selected_key';setup.key.picking=true;setup.render();
  assert.equal(setup.node('Button','ssh-save').disabledValue,true);
  assert.equal(setup.node('Button','ssh-key-choose').disabledValue,true);
  assert.equal(setup.node('Button','ssh-key-choose').labelText,'selected_key');
  assert.equal(setup.node('SegmentedTabs','ssh-authentication-7').props.disabled,true);
  assert.ok(setup.nodes.some(node=>node.type==='Field'&&node.labelText==='Key file'));
  assert.equal(setup.node('TextField','input-0').props.disabled,false);
});

test('database TLS and SSH selects preserve selection order and native disabled states',async()=>{
  const setup=await fixture('databases');setup.render();
  assert.deepEqual(plain(setup.node('SelectField','db-tls-7').props),{
    label:'TLS',placeholder:'TLS',selected:'require',disabled:false,
    items:[{id:'require',label:'Require'},{id:'prefer',label:'Prefer'},{id:'disable',label:'Disabled'}],
  });
  assert.equal(setup.node('SelectField','db-ssh-7').props.selected,'ssh-one');
  assert.equal(setup.node('Checkbox','db-read-only').checkedValue,true);
  const readonly=setup.node('Checkbox','db-read-only');readonly.change(false,setup.cx);assert.equal(setup.owner.read_only,false);
  setup.view.connected=false;readonly.change(true,setup.cx);assert.equal(setup.owner.read_only,false);
  setup.view.connected=true;setup.view.ssh=[];setup.render();assert.equal(setup.node('SelectField','db-ssh-7').props.disabled,true);
});
