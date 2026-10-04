import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const manifest = JSON.parse(await readFile(new URL('../plugin.json',import.meta.url),'utf8'));
const extension = manifest.extensions['dev.sailry.platform'];
const source = (await readFile(new URL('../dev.sailry.platform/host/main.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
function adapter() {
  let ids = 0;
  const value = vm.runInNewContext(`${source}\n({inspect,image,video,result})`,{
    newId:() => `00000000-0000-4000-8000-${String(++ids).padStart(12,'0')}`
  });
  return {...value,ids:() => ids};
}
const plain = value => JSON.parse(JSON.stringify(value));

test('three tools preserve schemas, descriptions and operation declarations', () => {
  const expected = [
    ['inspect_image','inspect',['prompt','path','attachment']],
    ['generate_image','image',['prompt','file_name']],
    ['generate_video','video',['prompt','file_name']]
  ];
  assert.deepEqual(extension.tools.map(tool => tool.name),expected.map(([name]) => name));
  for (const [name,handler,keys] of expected) {
    const tool = extension.tools.find(tool => tool.name === name);
    assert.deepEqual(tool.handler.parameters,{
      type:'object',properties:Object.fromEntries(keys.map(key => [key,{type:'string'}])),
      required:['prompt'],additionalProperties:false
    });
    assert.equal(tool.handler.operation,`media.${handler}`);
    assert.equal(tool.handler.name,handler);
    assert.equal(tool.handler.result,'result');
    assert.equal(tool.presentation,'details');
    assert.ok(tool.display.locales['zh-CN']);
    assert.match(tool.display.icon.svg,/<line x1="22"/);
  }
  assert.match(extension.tools[0].description,/attachment ID from this conversation/);
  assert.match(extension.tools[1].description,/without replacing existing files/);
  assert.match(extension.tools[2].description,/does not cancel a job already accepted/);
  assert.deepEqual(extension.settings_page.placement,{group:'ai',order:200});
});

test('prompt validation preserves original text and UTF-8 bounds', () => {
  const host = adapter();
  const prompt = '  Describe this image\n';
  assert.equal(host.inspect({prompt,path:'image.png'}).prompt,prompt);
  assert.equal(host.image({prompt:'🙂'.repeat(8000)}).prompt.length,16000);
  for (const args of [
    {},null,[],{prompt:42},{prompt:''},{prompt:' \n\t'},
    {prompt:'🙂'.repeat(8001)},{prompt:'x',owner:'another-session'},
    {prompt:'x',file_name:null},{prompt:'x',path:3}
  ]) assert.equal(host.image(args).error.code,'invalid_request');
  assert.equal(host.ids(),1);
});

test('inspection chooses exactly one source without changing its identity', () => {
  const host = adapter();
  for (const [kind,value] of [['path','目录/image.png'],['attachment','captured-attachment']]) {
    const args = {prompt:'Describe', [kind]:value};
    assert.deepEqual(plain(host.inspect(args)),{prompt:'Describe',source:{kind,value}});
    assert.deepEqual(args,{prompt:'Describe',[kind]:value});
  }
  for (const args of [
    {prompt:'Describe'},
    {prompt:'Describe',path:'image.png',attachment:'captured'},
    {prompt:'Describe',path:'image.png',file_name:'output.png'}
  ]) assert.equal(host.inspect(args).error.message,'select one image source');
  assert.equal(host.ids(),0);
});

test('generation prepares one stable output path with the original filename rules', () => {
  const host = adapter();
  for (const [kind,extension] of [['image','png'],['video','mp4']]) {
    const prepared = host[kind]({prompt:'Create'});
    assert.match(prepared.path,new RegExp(`^assets/generated/[0-9a-f-]+\\.${extension}$`));
    const args = {prompt:'Create',file_name:`我的 file.${extension}`};
    assert.deepEqual(plain(host[kind](args)),{prompt:'Create',path:`assets/generated/我的 file.${extension}`});
    assert.deepEqual(args,{prompt:'Create',file_name:`我的 file.${extension}`});
    for (const file_name of [`../x.${extension}`,`a/x.${extension}`,`a\\x.${extension}`,
      `a:x.${extension}`,`a\n.${extension}`,`a\u0085.${extension}`,`x.${extension.toUpperCase()}`,'']) {
      assert.equal(host[kind]({prompt:'Create',file_name}).isError,true,file_name);
    }
    for (const source of [{path:'input.png'},{attachment:'attachment'}]) {
      assert.equal(host[kind]({prompt:'Create',...source}).error.message,'generation does not accept input files');
    }
    assert.equal(prepared.path,host.result({output:{kind:'media',data:prepared}}).path);
  }
  assert.equal(host.ids(),2);
});

test('results preserve native values and authoritative uncertain faults', () => {
  const host = adapter();
  for (const data of [
    {text:'One bright pixel',usage:{total_tokens:12}},
    {path:'assets/generated/image.png',job:null,usage:null},
    {path:'assets/generated/video.mp4',job:'job-1',usage:{seconds:8}}
  ]) assert.equal(host.result({output:{kind:'media',data}}),data);
  const failure = {isError:true,error:{code:'outcome_unknown',message:'The provider outcome is uncertain'}};
  assert.equal(host.result({output:failure}),failure);
});
