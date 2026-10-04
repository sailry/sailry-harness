import test from 'node:test';
import assert from 'node:assert/strict';
import {status,diff,log} from '../dev.sailry.platform/host/main.js';

test('history preserves first-page defaults and exact continuation', () => {
  assert.deepEqual(log({}),{worktree:null,limit:20,cursor:null});
  assert.deepEqual(log({limit:5,cursor:null}),{worktree:null,limit:5,cursor:null});
  const cursor = {head:'a'.repeat(40),offset:5};
  assert.deepEqual(log({cursor,worktree:'captured-project-tree'}),
    {worktree:'captured-project-tree',limit:20,cursor});
  for (const cursor of ['HEAD',{head:'abcdef',offset:5},{head:'a'.repeat(40),offset:-1}]) {
    assert.equal(log({cursor}).isError,true);
  }
});

test('status and diff preserve explicit registered target and scope', () => {
  assert.deepEqual(status({}),{worktree:null});
  assert.deepEqual(status({worktree:'tree'}),{worktree:'tree'});
  assert.deepEqual(diff({path:'src/资料.rs'}),{worktree:null,path:'src/资料.rs',scope:'all'});
  assert.deepEqual(diff({path:'file',scope:'staged',worktree:'tree'}),
    {worktree:'tree',path:'file',scope:'staged'});
  for (const args of [{path:'file',scope:'both'},{path:'file',scope:null},{path:8}]) {
    assert.equal(diff(args).isError,true);
  }
});

test('arguments cannot inject project or execution commands', () => {
  assert.equal(status({project:'other'}).isError,true);
  assert.equal(diff({path:'file',command:'checkout'}).isError,true);
  for (const args of [{limit:0},{limit:101},{limit:null},{worktree:7}]) {
    assert.equal(log(args).isError,true);
  }
});
