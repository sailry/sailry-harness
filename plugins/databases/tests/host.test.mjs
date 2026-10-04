import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const plain=value=>JSON.parse(JSON.stringify(value));
async function fixture() {
  let state;
  const context=vm.createContext({});
  const sdk=new vm.SyntheticModule(['stageTurnState','readTurnState'],function(){this.setExport('stageTurnState',value=>state=plain(value));this.setExport('readTurnState',()=>plain(state));},{context});
  const modules=new Map([['sailry/sdk',sdk]]);
  async function load(name) {
    if(modules.has(name))return modules.get(name);
    const source=await readFile(new URL(`../dev.sailry.platform/host/${name}`,import.meta.url),'utf8');
    const module=new vm.SourceTextModule(source,{context,identifier:name});modules.set(name,module);await module.link(load);return module;
  }
  const main=await load('./main.js');await main.evaluate();
  return main.namespace;
}
const profiles=[{id:'a',name:'Reporting',engine:'mysql',database:'schema-name',read_only:true},{id:'b',name:'Analysis',engine:'postgres',database:'analytics',read_only:false}];
const tools=['database_catalog','database_query','database_execute'];

test('keeps exact captured choices and engine policy in the declared schemas',async()=>{
  const api=await fixture();
  const initialized=api.initialize({tools,resources:true,connections:{bound:{kind:'database',id:'a'},databases:profiles,ssh:[]}});
  assert.deepEqual(plain(initialized.tools),tools);
  assert.equal(initialized.instruction,'');
  const manifest=JSON.parse(await readFile(new URL('../plugin.json',import.meta.url),'utf8'));
  assert.ok(manifest.extensions['dev.sailry.platform'].desktop.conversations[0].context.includes('database workspace assistant'));
  for(const name of tools) {
    const schema=initialized.parameters[name];
    assert.deepEqual(plain(schema.properties.connection.enum),['a','b']);
    assert.ok(schema.properties.connection.description.includes('backtick-quoted identifiers'));
    assert.ok(schema.properties.connection.description.includes('qualify tables with schema, not database'));
    assert.equal(schema.additionalProperties,false);
  }
  assert.deepEqual(plain(initialized.parameters.database_query.required),['connection','sql']);
});

test('retains SQL exactly and only prepares the admitted targets and operation defaults',async()=>{
  const api=await fixture();api.initialize({tools,resources:true,connections:{bound:null,databases:profiles,ssh:[]}});
  const sql='SELECT "value" FROM "资料" WHERE id = 9007199254740993';
  assert.deepEqual(plain(api.query({connection:'b',database:'analytics',sql})),{connection:'b',database:'analytics',sql,row_limit:1000,timeout_ms:30000});
  assert.deepEqual(plain(api.catalog({connection:'a'})),{connection:'a'});
  for(const args of [{connection:'other',sql},{connection:'a',sql,read_only:false},{connection:'a',sql:7},{connection:'a',database:{},sql}]) assert.equal(api.execute(args).isError,true);
});

test('does not add workspace instructions to imported tools or project use',async()=>{
  const api=await fixture();
  for(const [resources,bound] of [[false,{kind:'database',id:'a'}],[true,null]]) {
    const result=api.initialize({tools:['database_query'],resources,connections:{bound,databases:profiles,ssh:[]}});
    assert.equal(result.instruction,'');assert.deepEqual(Object.keys(result.parameters),['database_query']);
  }
  const empty=api.initialize({tools,resources:true,connections:{bound:null,databases:[],ssh:[]}});
  assert.deepEqual(plain(empty.tools),[]);
});

test('preserves raw outputs by declaring no headless result formatter',async()=>{
  const manifest=JSON.parse(await readFile(new URL('../plugin.json',import.meta.url),'utf8'));
  for(const tool of manifest.extensions['dev.sailry.platform'].tools) assert.equal(tool.handler.result,undefined);
});
