import test from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {literal,statement,statements,primaryKeys,structure,definition,definitionText} from '../dev.sailry.platform/desktop/sql.js';
const integer = value => ({kind:'integer',value:String(value)}), text = value => ({kind:'text',value}), blob = value => ({kind:'blob',value});
const table = {schema:'main',name:'odd"table'}, columns = ['id','part','value','data'];
test('sqlite roundtrip preserves composite keys, Unicode, NUL and bytes',()=>{
  const db = new DatabaseSync(':memory:');
  db.exec('CREATE TABLE "odd""table"(id INTEGER,part TEXT,value TEXT,data BLOB,PRIMARY KEY(id,part))');
  const keys = db.prepare(primaryKeys('sqlite',table)).all().map(row=>row.name);
  assert.deepEqual(keys,['id','part']);
  const row = [integer(7),text("a'b"),text("line\n\0'\\项目"),blob([0,255,39])];
  db.exec(statement('sqlite',table,keys,columns,row,'insert'));
  const actual = db.prepare('SELECT value,data FROM "odd""table"').get();
  // SQLite's Node text adapter terminates strings at NUL; verify encoded bytes.
  assert.equal(db.prepare('SELECT hex(value) AS value FROM "odd""table"').get().value,Buffer.from(row[2].value).toString('hex').toUpperCase());
  assert.deepEqual(Array.from(actual.data),[0,255,39]);
  row[2] = {kind:'null'};
  assert.equal(db.prepare(statement('sqlite',table,keys,columns,row,'update')).run().changes,1);
  assert.equal(db.prepare(statement('sqlite',table,keys,columns,row,'delete')).run().changes,1);
  db.close();
});
test('dialects keep exact integer and decimal text without interpolation',()=>{
  assert.equal(literal('sqlite',integer('9223372036854775807')),'9223372036854775807');
  assert.equal(literal('sqlite',{kind:'integer',value:9223372036854775807}),null);
  assert.equal(literal('mysql',{kind:'real',value:'1234567890.12345678901234567890'}),'1234567890.12345678901234567890');
  for (const value of ['NaN','inf','1; DELETE FROM items',"'1'",'1e']) assert.equal(literal('sqlite',{kind:'real',value}),null);
  assert.equal(literal('postgres',text("quote'\\path")),"E'quote''\\\\path'");
  assert.equal(literal('postgres',blob([0,255])),"decode('00ff', 'hex')");
  assert.equal(literal('postgres',text('\0')),null);
  assert.equal(literal('mysql',text("quote'\\path")),"CONVERT(X'71756f7465275c70617468' USING utf8mb4)");
});
test('mutations require complete nonnull primary keys and bounded numeric cells',()=>{
  for (const kind of ['update','delete']) {
    assert.equal(statement('sqlite',table,[],['id','value'],[integer(1),text('a')],kind),null);
    assert.equal(statement('sqlite',table,['missing'],['id','value'],[integer(1),text('a')],kind),null);
    assert.equal(statement('sqlite',table,['id'],['id','value'],[{kind:'null'},text('a')],kind),null);
  }
  assert.equal(statement('sqlite',table,['id'],['id'],[integer(1)],'update'),null);
  assert.equal(literal('sqlite',blob([256])),null);
});
test('batch deletion targets only selected primary keys',()=>{
  const db = new DatabaseSync(':memory:');
  db.exec("CREATE TABLE items(id INTEGER PRIMARY KEY,value TEXT);INSERT INTO items VALUES(1,'a'),(2,'b'),(3,'c')");
  const sql = statements('sqlite',{schema:'main',name:'items'},['id'],['id','value'],[[integer(1),text('a')],[integer(3),text('c')]],'delete');
  assert.equal(db.prepare(sql).run().changes,2);
  assert.equal(db.prepare('SELECT id FROM items').get().id,2);
  db.close();
});
test('structure and definition use the selected table without replacing current results',()=>{
  const db = new DatabaseSync(':memory:');
  db.exec('CREATE TABLE "odd""table"(id INTEGER PRIMARY KEY,value TEXT)');
  assert.equal(db.prepare(structure('sqlite',table)).all().length,2);
  const sql = db.prepare(definition('sqlite',table)).get().sql;
  assert.equal(definitionText('sqlite',{rows:[[text(sql)]],truncated:false}),sql);
  assert.equal(definitionText('sqlite',{rows:[[text(sql)]],truncated:true}),null);
  assert.equal(definition('postgres',table),null);
  assert.ok(definition('mysql',table).startsWith('SHOW CREATE TABLE `main`.'));
  db.close();
});
