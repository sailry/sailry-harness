import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
async function fixture(packageName) {
  const context=vm.createContext({});const calls=[];let result,inspection;
  const sdk=new vm.SyntheticModule(['prepareRequest','completeRequest','forgetRequest'],function(){
    this.setExport('prepareRequest',command=>{calls.push(['prepare',command]);return 'captured-request';});
    this.setExport('completeRequest',async id=>{calls.push(['complete',id]);if(result instanceof Error)throw result;return result;});
    this.setExport('forgetRequest',id=>calls.push(['forget',id]));
  },{context});
  const connections=new vm.SyntheticModule(['requestOutcome'],function(){this.setExport('requestOutcome',async id=>{calls.push(['inspect',id]);return inspection;});},{context});
  const module=new vm.SourceTextModule(await readFile(new URL(`../../${packageName}/dev.sailry.platform/desktop/requests.js`,import.meta.url),'utf8'),{context});
  await module.link(name=>name==='sailry/sdk'?sdk:connections);await module.evaluate();
  return {Request:module.namespace.Request,calls,setResult:value=>result=value,setInspection:value=>inspection=value};
}
for(const packageName of ['databases','ssh'])test(`${packageName} checks the original receipt without replaying uncertain work`,async()=>{
  const setup=await fixture(packageName),cx={notify(){}};
  const command={kind:packageName==='ssh'?'run_ssh':'query_database',data:{profile:'original',expected_revision:7,sql:'UPDATE notes SET value = 1'}};
  const request=new setup.Request(command);setup.setResult(new Error('transport lost after admission'));
  await request.start(cx);assert.equal(request.unknown,true);assert.equal(request.done,false);
  await request.start(cx);assert.equal(setup.calls.filter(call=>call[0]==='complete').length,1);
  setup.setInspection({kind:'admitted'});await request.check(cx);assert.equal(request.unknown,true);
  setup.setInspection({kind:'completed',data:{Ok:{kind:'result',data:'9223372036854775807'}}});await request.check(cx);
  assert.equal(request.output.data,'9223372036854775807');assert.equal(request.done,true);
  assert.deepEqual(setup.calls.filter(call=>call[0]==='inspect'),[['inspect','captured-request'],['inspect','captured-request']]);
  assert.equal(setup.calls.filter(call=>call[0]==='prepare').length,1);
  request.release();assert.deepEqual(setup.calls.at(-1),['forget','captured-request']);
});
for(const packageName of ['databases','ssh'])test(`${packageName} retains authoritative uncertainty but releases definitive failure`,async()=>{
  const setup=await fixture(packageName),cx={notify(){}};
  setup.setResult({Err:{code:'outcome_unknown',message:'Remote mutation may have completed'}});
  const unknown=new setup.Request({kind:'write'});await unknown.start(cx);assert.equal(unknown.done,false);assert.equal(unknown.unknown,true);
  setup.setInspection({kind:'completed',data:{Err:{code:'outcome_unknown',message:'Remote mutation may have completed'}}});await unknown.check(cx);assert.equal(unknown.done,false);
  setup.setResult({Err:{code:'unavailable',message:'Connection failed before execution'}});
  const failed=new setup.Request({kind:'write'});await failed.start(cx);assert.equal(failed.done,true);assert.equal(failed.error.code,'unavailable');failed.release();
});
for(const packageName of ['databases','ssh'])test(`${packageName} releases a request confirmed not admitted without replay`,async()=>{
  const setup=await fixture(packageName),cx={notify(){}};
  setup.setResult(new Error('transport lost'));const request=new setup.Request({kind:'write'});
  await request.start(cx);setup.setInspection({kind:'not_admitted'});await request.check(cx);
  assert.equal(request.unknown,false);assert.equal(request.error.code,'unavailable');
  await request.start(cx);assert.equal(setup.calls.filter(call=>call[0]==='complete').length,1);
  request.release();assert.deepEqual(setup.calls.at(-1),['forget','captured-request']);
});
