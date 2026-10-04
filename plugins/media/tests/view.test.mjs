import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {messages} from '../dev.sailry.platform/desktop/locales.js';

const source=(await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const plain=value=>JSON.parse(JSON.stringify(value));
function fixture({missing=false,busy=false}={}) {
  const nodes=new Map();
  function element(kind='div',id=null,props={}) {
    const value={kind,id,props,items:[]};
    const proxy=new Proxy(value,{get(target,key) {
      if(key==='id')return id=>{target.id=id;nodes.set(id,proxy);return proxy;};
      if(key==='child')return child=>{target.items.push(child);return proxy;};
      if(key==='children')return children=>{target.items.push(...children);return proxy;};
      if(key in target)return target[key];
      return ()=>proxy;
    }});
    if(id)nodes.set(id,proxy);
    return proxy;
  }
  const render=vm.runInNewContext(`${source}\nrender`,{
    div:()=>element(),theme:()=>({colors:{}}),
    SelectField:{new:(id,props)=>element('SelectField',id,props)},
    SettingsGroup:{new:(id,props)=>element('SettingsGroup',id,props)},
  });
  const view={text:messages('en'),busy:()=>busy,label:kind=>missing&&kind==='vision'?'Model unavailable / kept':'Unconfigured',
    items:(id,kind)=>[{id:`${id}-clear`,label:'Unconfigured',checked:!missing},
      {id:`${id}-model`,label:`Provider / ${kind}`,checked:false}]};
  render(view);
  return nodes;
}

test('only supported media roles use the shared select field',()=>{
  const nodes=fixture();
  for(const key of ['media_image_understanding','media_image_generation','media_video_generation']) {
    const field=nodes.get(key);
    assert.equal(field.kind,'SelectField');
    assert.deepEqual(plain(field.props),{label:messages('en')[key],placeholder:'Unconfigured',disabled:false,
      selected:`${key}-clear`,items:[{id:`${key}-clear`,label:'Unconfigured'},
        {id:`${key}-model`,label:`Provider / ${key==='media_image_understanding'?'vision':key==='media_image_generation'?'image':'video'}`}]});
  }
  for(const key of ['media_audio_transcription','media_speech_synthesis','media_video_understanding']) {
    assert.equal(nodes.has(`settings-row-${key}`),false);
  }
  assert.equal(nodes.get('media_roles').items.length,3);
});

test('missing saved bindings remain explicit and busy selects stay disabled',()=>{
  const field=fixture({missing:true,busy:true}).get('media_image_understanding');
  assert.equal(field.props.selected,null);
  assert.equal(field.props.placeholder,'Model unavailable / kept');
  assert.equal(field.props.disabled,true);
});
