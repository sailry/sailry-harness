import {readFile} from "node:fs/promises";
import vm from "node:vm";

export const clone = value => JSON.parse(JSON.stringify(value));
export const id = number => `11111111-1111-4111-8111-${String(number).padStart(12,"0")}`;
export const project = "22222222-2222-4222-8222-222222222222";
export const foreign = "33333333-3333-4333-8333-333333333333";
export const item = (number, changes = {}) => ({id:id(number),project,title:"Review",message:"",completed:false,
  due_ms:null,notified_ms:null,...changes});

export async function fixture() {
  const records = new Map(), requests = new Map(), receipts = new Map();
  const schedules = new Map(), handlers = new Map();
  const prepared = [], queries = [], catalogReads = [], scans = [];
  let state = null, catalog = {projects:[{id:project,name:"Current"},{id:foreign,name:"Other"}],worktrees:[]};
  const get = key => clone(records.get(key) ?? {key,present:false,value:null,revision:"0"});
  const seed = (value, revision = "1") => records.set(`reminder/${value.id}`,
    {key:`reminder/${value.id}`,present:true,value:clone(value),revision});
  function apply(operations) {
    const snapshots = [records,schedules,handlers].map(values => clone([...values])), outputs = [];
    try {
      for (const operation of operations) {
        const data = operation.data;
        if (["write","remove"].includes(operation.kind)) {
          const old = get(data.key);
          if (old.revision !== data.expected_revision) throw Object.assign(new Error("Value changed"), {code:"revision_conflict"});
          const saved = {key:data.key,revision:String(BigInt(old.revision) + 1n),present:operation.kind !== "remove",
            value:operation.kind === "remove" ? null : clone(data.value)};
          records.set(data.key,saved); outputs.push({kind:"plugin_value",data:clone(saved)});
        } else if (operation.kind === "dispatch") {
          const action = data.kind, input = data.data;
          const target = action.endsWith("schedule") ? schedules : handlers;
          const key = input.id ?? input.name, old = target.get(key);
          if (String(old?.revision ?? 0) !== (input.revision ?? input.expected_revision)) {
            throw Object.assign(new Error("Dispatch changed"), {code:"revision_conflict"});
          }
          if (action.startsWith("remove")) target.delete(key);
          else target.set(key,{...clone(input),revision:(old?.revision ?? 0) + 1});
          outputs.push({kind:"plugin_dispatch",data:null});
        } else outputs.push({kind:"notified",data:null});
      }
      return {kind:"plugin_transaction",data:outputs};
    } catch (error) {
      for (const [index, target] of [records,schedules,handlers].entries()) {
        target.clear(); for (const [key,value] of snapshots[index]) target.set(key,value);
      }
      throw error;
    }
  }
  const sdk = {
    context:() => ({invocation:id(999),package:{name:"reminders",digest:"fixture",config_revision:1}}),
    getValue:async key => get(key),
    listKeys:async(prefix,after,limit) => {
      scans.push({prefix,after,limit});
      const values = [...records].filter(([key,value]) => value.present && key.startsWith(prefix) && (after === null || key > after))
        .map(([key]) => key).sort();
      const keys = values.slice(0,limit);
      return {keys,after:values.length > limit ? keys.at(-1) : null};
    },
    readProjectCatalog:async() => {catalogReads.push(true);return clone(catalog);},
    readTurnState:() => clone(state), stageTurnState:value => {state = clone(value);}, callId:() => id(1000),
    prepareRequest:command => {
      const request = `query-${queries.length}`; queries.push(clone(command)); requests.set(request,command); return request;
    },
    prepareTransaction:operations => {
      const request = `transaction-${prepared.length}`; prepared.push(clone(operations)); requests.set(request,operations); return request;
    },
    completeRequest:async request => {
      if (!receipts.has(request)) {
        const command = requests.get(request);
        if (Array.isArray(command)) {
          try { receipts.set(request,{Ok:{data:apply(command).data}}); }
          catch (error) { receipts.set(request,{Err:{code:error.code,message:error.message}}); }
        } else {
          const name = command.data.action.kind;
          if (!["list_schedules","list_handlers"].includes(name)) throw new Error("Unsupported dispatch query");
          const values = name === "list_schedules" ? schedules : handlers;
          receipts.set(request,{Ok:{data:{data:clone([...values.values()])}}});
        }
      }
      return clone(receipts.get(request));
    },
    forgetRequest:request => requests.delete(request),
  };
  const context = vm.createContext({}), cache = new Map();
  const bridge = new vm.SyntheticModule(Object.keys(sdk),function() {
    for (const [name, value] of Object.entries(sdk)) this.setExport(name,value);
  }, {context});
  async function module(name) {
    if (name === "sailry/sdk") return bridge;
    const url = new URL(name,new URL("../dev.sailry.platform/host/",import.meta.url));
    if (cache.has(url.href)) return cache.get(url.href);
    const source = await readFile(url,"utf8"), value = new vm.SourceTextModule(source,{context,identifier:url.href});
    cache.set(url.href,value); return value;
  }
  const main = await module("main.js");
  await main.link((name,parent) => module(name === "sailry/sdk" ? name : new URL(name,parent.identifier).href));
  await main.evaluate();
  const namespace = name => cache.get(new URL(name,new URL("../dev.sailry.platform/host/",import.meta.url)).href).namespace;
  return {main:main.namespace,records:namespace("records.js"),mutations:namespace("mutations.js"),
    entries:records,prepared,queries,catalogReads,scans,seed,get,apply,schedules,handlers,
    get catalog() { return clone(catalog); },set catalog(value) {catalog=clone(value);},get state() {return clone(state);} };
}
