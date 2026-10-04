import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

test('controller changes use a spawned context after initialization returns',async()=>{
  const source=(await readFile(new URL('../dev.sailry.platform/desktop/main.js',import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class');
  let changed,notifications=0;
  const globals={View:class{},context:()=>'{"locale":"en"}',messages:()=>({}),
    Controller:class {constructor(api,text,notify){changed=notify;this.location={main:true,can_move:true};}},
  };
  for(const name of ['readWorktreeCatalog','listWorktrees','inspectGit','resolveGitRevision','prepareWorktreeChange',
    'prepareGitChange','completeRequest','forgetRequest','newId','readLocation','selectLocation','worktreeHasDrafts',
    'releaseWorktree','toast','publishContributions'])globals[name]=()=>{};
  for(const name of ['nextLocationChange','nextContributionEvent','nextPickerEvent','modal_closed','nextTextEvent','nextToastEvent'])
    globals[name]=()=>new Promise(()=>{});
  const Worktrees=vm.runInNewContext(`${source}\nWorktrees`,globals);
  const owner=new Worktrees();
  const live={notify(){notifications++;},spawn:task=>task(live)};
  const initial={notify(){throw Error('initialization context expired');},spawn:live.spawn};
  owner.init({},initial);changed();
  assert.equal(notifications,1);
});
