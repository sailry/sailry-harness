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
  const main=await load('./main.js');await main.evaluate();return main.namespace;
}
const tools=['ssh_run','ssh_transfer'];
const connections={bound:{kind:'ssh',id:'a'},databases:[],ssh:[{id:'a',name:'Work host'}]};

test('binds the saved target and package workspace guidance',async()=>{
  const api=await fixture();
  const result=api.initialize({tools,resources:true,connections});
  assert.deepEqual(plain(result.parameters.ssh_run.properties.connection.enum),['a']);
  assert.equal(result.parameters.ssh_run.properties.connection.description,'[["a","Work host"]]');
  assert.equal(result.instruction,'');
  const manifest=JSON.parse(await readFile(new URL('../plugin.json',import.meta.url),'utf8'));
  assert.ok(manifest.extensions['dev.sailry.platform'].desktop.conversations[0].context.includes('SSH workspace assistant'));
  assert.equal(api.initialize({tools,resources:false,connections}).instruction,'');
  assert.equal(api.initialize({tools,resources:true,connections:{...connections,bound:null}}).instruction,'');
});

test('keeps command text, enforces target and timeout, and never accepts caller credentials',async()=>{
  const api=await fixture();api.initialize({tools,resources:true,connections});
  const command='printf "资料 🙂\\n"';
  assert.deepEqual(plain(api.run({connection:'a',command})),{connection:'a',command,timeout_ms:120000});
  assert.equal(api.run({connection:'a',command,timeout_ms:900000}).timeout_ms,900000);
  for(const args of [{connection:'b',command},{connection:'a',command,timeout_ms:0},{connection:'a',command,timeout_ms:900001},{connection:'a',command,password:'hidden'}]) assert.equal(api.run(args).isError,true);
});

test('prepares only explicit upload or download while preserving both path domains',async()=>{
  const api=await fixture();api.initialize({tools,resources:true,connections});
  const input={connection:'a',path:'local/资料.txt',remote_path:'/srv/资料.txt',direction:'upload'};
  assert.deepEqual(plain(api.transfer(input)),{...input,timeout_ms:120000});
  assert.equal(api.transfer({...input,direction:'other'}).isError,true);
  assert.equal(api.transfer({...input,worktree:'caller-selected'}).isError,true);
  assert.deepEqual(plain(api.initialize({tools,resources:true,connections:{...connections,ssh:[]}}).tools),[]);
});
