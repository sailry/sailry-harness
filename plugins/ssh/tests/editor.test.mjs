import test from 'node:test';
import assert from 'node:assert/strict';
import {draft,prepare,setSharing} from '../dev.sailry.platform/desktop/editor.js';
test('editing preserves stored secrets unless a new credential is provided',()=>{
  const original = {id:'ssh',revision:7,name:'Host',host:'host',port:22,username:'user',authentication:'private_key',host_key:{algorithm:'ed25519',fingerprint:'trusted'},sharing:{scope:'global'}};
  const editor = draft(original);
  assert.deepEqual(prepare(editor,{}),{profile:original,replace:false,source:'private_key'});
  assert.equal(prepare(editor,{key:true}).replace,true);
  assert.equal(prepare(editor,{passphrase:true}).replace,false);
  editor.authentication = 'password';
  assert.equal(prepare(editor,{}).replace,true);
  assert.deepEqual(original.host_key,{algorithm:'ed25519',fingerprint:'trusted'});
});
test('key-file imports become private-key material on the captured Node',()=>{
  const editor = draft(null,'ssh'); editor.authentication = 'key_path';
  assert.throws(()=>prepare(editor,{}),/key file required/);
  const prepared = prepare(editor,{file:true});
  assert.equal(prepared.profile.authentication,'private_key');
  assert.equal(prepared.replace,true);
  assert.equal(prepared.source,'key_path');
  assert.equal(JSON.stringify(prepared).includes('secret'),false);
});
test('existing key paths can retain their credential and invalid sharing is rejected',()=>{
  const editor = draft({id:'ssh',revision:2,name:'Host',host:'host',port:22,username:'user',authentication:'key_path',host_key:null,sharing:null});
  assert.equal(prepare(editor,{}).replace,false);
  editor.sharing = {scope:'projects',projects:[]};
  assert.throws(()=>prepare(editor,{}),/sharing/);
  editor.sharing = null; editor.authentication = 'agent';
  assert.throws(()=>prepare(editor,{}),/authentication/);
  editor.authentication = 'password'; editor.fields[2] = '22.5';
  assert.throws(()=>prepare(editor,{}),/port/);
});
test('sharing scope changes retain the selected project draft',()=>{
  const original={id:'ssh',revision:2,name:'Host',host:'host',port:22,username:'user',authentication:'password',sharing:{scope:'projects',projects:['one','two']}};
  const editor=draft(original);
  for(const scope of ['projects','global','private','projects'])setSharing(editor,scope);
  assert.deepEqual(prepare(editor,{}).profile.sharing,original.sharing);
  editor.projects.push('three');setSharing(editor,'projects');
  assert.deepEqual(editor.sharing.projects,['one','two','three']);
  assert.deepEqual(original.sharing.projects,['one','two']);
});
