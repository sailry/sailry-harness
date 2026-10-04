import test from "node:test";
import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import vm from "node:vm";
import {messages} from "../dev.sailry.platform/desktop/locales.js";

const source = (await readFile(new URL("../dev.sailry.platform/desktop/editor.js",import.meta.url),"utf8"))
  .replace(/^import[\s\S]*?;\n/gm,"").replace(/^export /gm,"");
const plain = value => JSON.parse(JSON.stringify(value));
const controller = (await readFile(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8"))
  .replace(/^import .*;\n/gm, "").replace(/^export default /gm, "");
const control = vm.runInNewContext(`${controller}\nReminders.prototype.control`, {View:class {}});
function fixture() {
  const nodes = new Map(), texts = new Map(), dates = new Map(), released = [], created = [];
  function element(kind="div",id=null,props={}) {
    const target={kind,selector:id,props,items:[],style:{}};
    const proxy=new Proxy(target,{get(target,key) {
      if (key==="id") return value => {target.selector=value;nodes.set(value,proxy);return proxy;};
      if (key==="child") return value => {target.items.push(value);return proxy;};
      if (key==="children") return values => {target.items.push(...values);return proxy;};
      if (key==="item") return (label,callback) => {target.items.push({label,callback});return proxy;};
      if (key in target) return target[key];
      return (...args) => {target.style[key]=args;return proxy;};
    }});
    if (id) nodes.set(id,proxy); return proxy;
  }
  const constructors=Object.fromEntries(["Button","Checkbox","Field"]
    .map(kind=>[kind,function(id,label){return element(kind,id,{label});}]));
  const api=vm.runInNewContext(`${source}\n({editor,release,value,render})`,{div:()=>element(),...constructors,
    VForm:function(){return element("Form");},SelectField:{new:(id,props)=>element("SelectField",id,props)},
    theme:()=>({colors:{destructive:"danger"}}),window:{viewport_size:()=>({width:1200,height:900})},
    createText:(value,props)=>{const id=`text-${texts.size}`;texts.set(id,value);created.push({id,props});return id;},
    readText:id=>texts.get(id),releaseText:id=>released.push(id),
    createDateTime:value=>{const id="time";dates.set(id,value);return id;},readDateTime:id=>dates.get(id),releaseDateTime:id=>released.push(id),
    TextField:{new:(id,props)=>element("TextField",id,props)},DateTimeField:{new:(id,props)=>element("DateTimeField",id,props)},
  });
  const text=messages("en"),cx={notify(){}},view={text,items:[],catalog:{projects:[{id:"current",name:"Current"}]},
    busy:false,pending:null,error:null,projectFilter:"current",close(){},save(){},
    control,
    projectName(id){return id===null ? text.noProject : this.catalog.projects.find(project=>project.id===id)?.name ?? text.unavailableProject;}};
  return {api,view,cx,nodes,texts,created,released,render(){nodes.clear();return api.render(view);}};
}

test("new drafts remain unassigned despite the project filter and preserve text field order", () => {
  const setup=fixture(); setup.view.editing=setup.api.editor(null,setup.view.text); setup.render();
  assert.equal(setup.view.editing.draft.project,null);
  assert.deepEqual(setup.created.map(value=>value.props.label),["Title","Notes"]);
  assert.deepEqual(setup.created.map(value=>value.props.placeholder),["What needs doing?","Add a note"]);
  const form = setup.nodes.get("reminder-fields").items[0];
  assert.equal(form.kind,"Form");
  assert.ok(form.items.every(field=>field.kind === "Field"));
  assert.equal(setup.nodes.get("reminder-project").kind,"SelectField");
  assert.equal(setup.nodes.get("reminder-project").props.selected,"none");
  assert.equal(setup.nodes.get("reminder-project").props.placeholder,"Choose a project");
  setup.view.control({id:"reminder-project",value:"current"},setup.cx);
  assert.equal(setup.view.editing.draft.project,"current");
  setup.texts.set(setup.view.editing.title,"Chosen");
  assert.equal(setup.api.value(setup.view.editing).project,"current");
  setup.render(); setup.view.control({id:"reminder-project",value:"none"},setup.cx);
  assert.equal(setup.api.value(setup.view.editing).project,null);
  setup.api.release(setup.view.editing); assert.deepEqual(setup.released,["text-0","text-1","time"]);
});

test("unavailable associations keep their ID and change only after explicit selection", () => {
  const setup=fixture(),item={id:"saved",project:"removed",revision:"3",title:"Saved",message:"",due_ms:null,completed:false};
  setup.view.editing=setup.api.editor(item,setup.view.text); setup.render();
  assert.equal(setup.nodes.get("reminder-project").props.selected,"removed");
  assert.equal(setup.nodes.get("reminder-project").props.items.at(-1).label,"Unavailable project");
  assert.equal(setup.api.value(setup.view.editing).project,"removed");
  setup.view.control({id:"reminder-project",value:"current"},setup.cx);
  assert.equal(setup.api.value(setup.view.editing).project,"current");
  assert.equal(item.project,"removed");
  setup.view.pending="original-request"; setup.render();
  assert.equal(setup.nodes.get("reminder-project").kind,"SelectField");
  assert.equal(setup.nodes.get("reminder-project").props.disabled,true);
  setup.view.control({id:"reminder-project",value:"none"},setup.cx);
  assert.equal(setup.api.value(setup.view.editing).project,"current");
  setup.view.pending=null; setup.view.busy=true;
  setup.view.control({id:"reminder-project",value:"none"},setup.cx);
  assert.equal(setup.api.value(setup.view.editing).project,"current");
  setup.view.busy=false;
  setup.view.control({id:"reminder-project",value:"not-a-choice"},setup.cx);
  assert.equal(setup.api.value(setup.view.editing).project,"current");
});
