import test from 'node:test';
import assert from 'node:assert/strict';
import {draft,switchEngine,prepare,validSharing,setSharing} from '../dev.sailry.platform/desktop/editor.js';
test('transport edits preserve profile identity, sharing and explicit port',()=>{
  const original = {id:'db',revision:3,name:'Local',read_only:false,sharing:{scope:'projects',projects:['one']},connection:{kind:'ssh',engine:'postgres',host:'host',port:15432,database:'catalog',username:'user',tls:'require',ssh:'ssh-one'}};
  const editor = draft(original);
  assert.deepEqual(prepare(editor,[{id:'ssh-one'}]),original);
  switchEngine(editor,'mysql');
  assert.equal(editor.fields[3],'15432');
  editor.method = 'socket'; editor.fields[1] = ' /tmp/mysql.sock ';
  assert.deepEqual(prepare(editor,[]).connection,{kind:'socket',engine:'mysql',port:3306,database:'catalog',username:'user',path:'/tmp/mysql.sock'});
  assert.equal(original.connection.kind,'ssh');
});
test('default ports follow engines while invalid tunnel targets are rejected',()=>{
  const editor = draft(null,'new');
  assert.equal(editor.read_only,true);
  switchEngine(editor,'postgres'); assert.equal(editor.fields[3],'5432');
  switchEngine(editor,'mysql'); assert.equal(editor.fields[3],'3306');
  editor.method = 'ssh'; editor.ssh = 'missing';
  assert.throws(()=>prepare(editor,[]),/unavailable/);
  editor.method = 'tcp'; editor.fields[3] = '65536';
  assert.throws(()=>prepare(editor,[]),/port/);
  editor.fields[3] = '22;'; assert.throws(()=>prepare(editor,[]),/port/);
});
test('sharing is explicit and valid selected projects are retained',()=>{
  assert.equal(validSharing(null),true);
  assert.equal(validSharing({scope:'global'}),true);
  assert.equal(validSharing({scope:'projects',projects:[]}),false);
  assert.equal(validSharing({scope:'projects',projects:['a','a']}),false);
  const editor = draft(null,'new');
  editor.sharing = {scope:'projects',projects:['a','b']};
  assert.deepEqual(prepare(editor,[]).sharing,editor.sharing);
});
test('sharing scope changes retain the selected project draft',()=>{
  const original={id:'db',revision:2,name:'Rows',connection:{kind:'sqlite',path:'rows.sqlite'},read_only:true,sharing:{scope:'projects',projects:['one','two']}};
  const editor=draft(original);
  for(const scope of ['projects','global','private','projects'])setSharing(editor,scope);
  assert.deepEqual(prepare(editor,[]).sharing,original.sharing);
  editor.projects.push('three');setSharing(editor,'projects');
  assert.deepEqual(editor.sharing.projects,['one','two','three']);
  assert.deepEqual(original.sharing.projects,['one','two']);
});
