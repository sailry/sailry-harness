import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import {messages,errorKey,imported} from '../dev.sailry.platform/desktop/locales.js';

async function fixture(importProfile, listProfiles = async()=>[]) {
  const source=(await readFile(new URL('../dev.sailry.platform/desktop/settings.js',import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class');
  const notices=[],tasks=[],pending=()=>new Promise(()=>{});
  const Controller=vm.runInNewContext(`${source}\nSettings`,{View:class{},context:()=>'{"locale":"en"}',messages,errorKey,imported,
    readBrowserSettings:()=>({supported:true,enabled:true,persistent:false}),nextControlEvent:pending,modal_closed:pending,
    importBrowserProfile:importProfile,listBrowserProfiles:listProfiles,toast:value=>notices.push(value)});
  const owner=new Controller(),cx={notify(){},spawn:work=>tasks.push(work(cx))};owner.init({},cx);
  return {owner,cx,notices,act:async work=>{const before=tasks.length;work();await Promise.all(tasks.slice(before));}};
}

test('failed imports toast once and keep the captured profile picker',async()=>{
  const setup=await fixture(async()=>{throw new Error('Import unavailable');}),profiles=[{id:'captured',name:'Fixture'}];setup.owner.profiles=profiles;
  await setup.act(()=>setup.owner.import(profiles[0],setup.cx));
  assert.equal(setup.owner.profiles,profiles);assert.equal(setup.owner.message,null);assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].kind,'error');
});

test('successful imports close their picker and report only through a toast',async()=>{
  const setup=await fixture(async()=>({count:2,skipped:0}));setup.owner.profiles=[{id:'captured',name:'Fixture'}];
  await setup.act(()=>setup.owner.import(setup.owner.profiles[0],setup.cx));
  assert.equal(setup.owner.profiles,null);assert.equal(setup.owner.message,null);assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].kind,'info');
  assert.equal(setup.notices[0].message,imported(messages('en'),2,0));
});

test('scanning displays profiles without reporting them missing',async()=>{
  const profiles=[{id:'captured',name:'Fixture'}];
  const setup=await fixture(null,async()=>profiles);
  await setup.act(()=>setup.owner.scan(setup.cx));
  assert.equal(setup.owner.profiles,profiles);assert.equal(setup.owner.busy,false);assert.equal(setup.notices.length,0);
});

test('cancelled folder selection is silent and releases the scan',async()=>{
  const setup=await fixture(null,async()=>null);
  await setup.act(()=>setup.owner.scan(setup.cx));
  assert.equal(setup.owner.profiles,null);assert.equal(setup.owner.busy,false);assert.equal(setup.notices.length,0);
});

test('denied access is not reported as a missing profile',async()=>{
  const setup=await fixture(null,async()=>{throw new Error('browser_chrome_access_denied');});
  await setup.act(()=>setup.owner.scan(setup.cx));
  assert.equal(setup.owner.busy,false);assert.equal(setup.notices.length,1);
  assert.equal(setup.notices[0].message,messages('en').browser_chrome_access_denied);
});
