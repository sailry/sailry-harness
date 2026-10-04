import test from "node:test";
import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import vm from "node:vm";
import {messages} from "../dev.sailry.platform/desktop/locales.js";

const plain = value => JSON.parse(JSON.stringify(value));
const read = async path => (await readFile(new URL(path, import.meta.url), "utf8"))
  .replace(/^import .*;\n/gm, "").replace(/^export default /gm, "").replace(/^export /gm, "");
const source = await read("../dev.sailry.platform/desktop/main.js");
const records = await read("../dev.sailry.platform/host/records.js");

test("all loads pending and completed records across real page continuations", async () => {
  const entries = new Map(), scans = [], pages = [], tasks = [];
  for (let index=1;index<=35;index++) {
    const id = `11111111-1111-4111-8111-${String(index).padStart(12,"0")}`;
    entries.set(`reminder/${id}`, {key:`reminder/${id}`,present:true,revision:"1",value:{
      id, project:null, title:`Item ${index}`, message:"Keep this note", completed:index%2===0,
      due_ms:null,notified_ms:null,
    }});
  }
  const page = vm.runInNewContext(`${records}\npage`, {
    getValue:async key => plain(entries.get(key)),
    listKeys:async(prefix,after,limit) => {
      scans.push({prefix,after,limit});
      const matching = [...entries.keys()].filter(key=>key.startsWith(prefix) && (after===null || key>after)).sort();
      const keys = matching.slice(0,limit);
      return {keys,after:matching.length>limit ? keys.at(-1) : null};
    },
  });
  const Reminders = vm.runInNewContext(`${source}\nReminders`, {
    View:class {},messages,context:() => JSON.stringify({locale:"en"}),
    page:async input => {pages.push(plain(input));return page(input);},
    readProjectCatalog:async() => ({projects:[],worktrees:[]}),
  });
  const view = new Reminders(), cx = {notify() {},spawn:task=>tasks.push(task)};
  view.init({},cx);
  assert.equal(view.loaded,false);
  assert.equal(view.completed,null);
  const draft = {draft:{title:"Unsaved",message:"Keep the draft"}};
  view.editing=draft;view.pending="original-request";view.error="unknown";
  view.refresh(cx);
  await tasks.at(-1)(cx);
  assert.equal(view.loading,false);
  assert.equal(view.loaded,true);
  assert.equal(view.items.length,35);
  assert.equal(view.items.filter(item=>item.completed).length,17);
  assert.equal(view.items.filter(item=>!item.completed).length,18);
  assert.deepEqual(pages,[{after:null},{after:[...entries.keys()][31]}]);
  assert.ok(scans.every(scan=>scan.prefix==="reminder/" && scan.limit===32));
  assert.equal(view.editing,draft);
  assert.equal(view.pending,"original-request");
  assert.equal(view.error,"unknown");
  assert.ok(view.items.every(item=>item.message==="Keep this note"));
});
