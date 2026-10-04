import test from 'node:test';
import assert from 'node:assert/strict';
import {Repository} from '../dev.sailry.platform/desktop/repository.js';
import {rows,checkbox,selectedId} from '../dev.sailry.platform/desktop/changes.js';

const deferred = () => { let resolve,reject; const promise=new Promise((done,fail) => {resolve=done;reject=fail;}); return {promise,resolve,reject}; };
const settle = async () => { for (let index=0;index<12;index++) await Promise.resolve(); };
const status = () => ({kind:'ready',head:'original',branch:'main',index_revision:'index-1',entries:[{path:'a.txt',staged:'modified',unstaged:null,untracked:false,conflicted:false}]});
function fixture(extra = {}) {
  const prepared=[],completed=[],forgotten=[],reported=[];
  const api={inspectGit:async()=>status(),listGitBranches:async()=>({sync:{head_message:'Previous message'}}),
    prepareGitCommit:draft=>{prepared.push(draft);return 'stable-request';},
    prepareGitIndex:draft=>{prepared.push(draft);return 'stable-request';},
    prepareGitChange:draft=>{prepared.push(draft);return 'stable-request';},
    completeRequest:async id=>{completed.push(id);return {Ok:{kind:'git_index',data:{revision:'index-2'}}};},
    forgetRequest:id=>forgotten.push(id),faultCode:()=> 'outcome_unknown',...extra};
  const owner=new Repository(api,()=>{},key=>reported.push(key));owner.status=status();return {owner,prepared,completed,forgotten,reported};
}

test('initialization retains its request until recovery and then enables normal writes',async()=>{
  let attempt=0,initialized=false;
  const {owner,prepared,completed,forgotten}=fixture({
    inspectGit:async()=>initialized ? {...status(),kind:'unborn',head:null} : {kind:'directory',head:null,branch:null,index_revision:null,entries:[]},
    completeRequest:async id=>{completed.push(id); if (++attempt===1) return {Err:{code:'outcome_unknown'}}; initialized=true;return {Ok:{kind:'git_action_completed'}};},
  });
  await owner.refresh();
  assert.equal(owner.writable(),false); assert.equal(owner.canInitialize(),true);
  assert.equal(await owner.initialize(),false);
  assert.deepEqual(prepared,[{kind:'action',action:'initialize',expected_index:'',expected_head:null,expected_branch:null}]);
  assert.equal(owner.canInitialize(),false);
  assert.equal(await owner.initialize(),false);
  assert.equal(await owner.complete(),true);
  assert.deepEqual(completed,['stable-request','stable-request']);
  assert.deepEqual(forgotten,['stable-request']);
  assert.equal(owner.status.kind,'unborn'); assert.equal(owner.writable(),true);
  assert.equal(owner.canInitialize(),false);
});

test('commit preserves newer input and freezes the submitted revisions',async()=>{
  const receipt=deferred();const {owner,prepared,forgotten,reported}=fixture({completeRequest:()=>receipt.promise});
  owner.message='Submitted';const pending=owner.commit();owner.message='New draft';
  assert.equal(prepared[0].message,'Submitted');assert.equal(prepared[0].expected_index,'index-1');
  assert.equal(owner.writable(),false);
  receipt.resolve({Ok:{kind:'git_commit_created',data:{id:'commit',follow_up:{code:'unavailable',message:'Push failed'}}}});
  assert.equal(await pending,true);assert.equal(owner.message,'New draft');
  assert.equal(owner.followUp.message,'Push failed');assert.deepEqual(forgotten,['stable-request']);assert.equal(owner.pending,null);
  assert.deepEqual(reported,['git_commit_sync_failed']);
});

test('accepted read failures use the ordinary notification channel',async()=>{
  for (const [method,read,key] of [
    ['inspectGit',owner=>owner.refresh(),'git_read_failed'],
    ['listGitBranches',owner=>owner.refresh(),'git_branches_failed'],
    ['readGitLog',owner=>owner.loadHistory(),'git_read_failed'],
    ['readGitDiff',owner=>owner.open({kind:'file',path:'a.txt'}),'git_read_failed'],
  ]) {
    const {owner,reported}=fixture({[method]:async()=>{throw Error('read failed');}});
    await read(owner);assert.equal(owner.error,key);assert.deepEqual(reported,[key]);
  }
});

test('stale and released reads cannot report failures',async()=>{
  for (const [method,read] of [
    ['inspectGit',owner=>owner.refresh()],
    ['listGitBranches',owner=>owner.refresh()],
    ['readGitLog',owner=>owner.loadHistory()],
    ['readGitDiff',owner=>owner.open({kind:'file',path:'a.txt'})],
  ]) {
    const pending=deferred();const {owner,reported}=fixture({[method]:()=>pending.promise});
    const readResult=read(owner);owner.stop();pending.reject(Error('late failure'));
    await readResult;assert.deepEqual(reported,[]);
  }
  const first=deferred();let count=0;
  const {owner,reported}=fixture({readGitDiff:async path=>++count===1 ? first.promise : {path,text:'accepted'}});
  const stale=owner.open({kind:'file',path:'a.txt'});await owner.open({kind:'file',path:'b.txt'});
  first.reject(Error('stale failure'));await stale;
  assert.equal(owner.selected,'b.txt');assert.deepEqual(reported,[]);
});

test('superseded refreshes follow the latest result',async()=>{
  for (const latestFirst of [false,true]) {
    const first=deferred(),second=deferred();let reads=0;
    const {owner}=fixture({inspectGit:()=>++reads===1 ? first.promise : second.promise});
    const older=owner.refresh(),latest=owner.refresh();let finished=false;
    older.then(()=>{finished=true;});
    if (latestFirst) { second.resolve({...status(),branch:'latest'});await latest; }
    first.resolve({...status(),branch:'stale'});await settle();
    if (!latestFirst) { assert.equal(finished,false);second.resolve({...status(),branch:'latest'}); }
    assert.deepEqual(await Promise.all([older,latest]),[true,true]);
    assert.equal(owner.status.branch,'latest');assert.equal(reads,2);
  }
});

test('stale inspect and branch failures follow the accepted refresh',async()=>{
  for (const method of ['inspectGit','listGitBranches']) {
    const first=deferred(),second=deferred();let reads=0;
    const {owner,reported}=fixture({[method]:()=>++reads===1 ? first.promise : second.promise});
    const older=owner.refresh();await settle();const latest=owner.refresh();await settle();
    first.reject(Error('stale failure'));await settle();
    second.resolve(method === 'inspectGit' ? status() : {sync:{head_message:'Latest'}});
    assert.deepEqual(await Promise.all([older,latest]),[true,true]);assert.deepEqual(reported,[]);
  }
});

test('latest failure and release reach every refresh waiter',async()=>{
  for (const released of [false,true]) {
    const first=deferred(),second=deferred();let reads=0;
    const {owner,reported}=fixture({inspectGit:()=>++reads===1 ? first.promise : second.promise});
    const older=owner.refresh(),latest=owner.refresh();
    first.resolve(status());await settle();
    if (released) {owner.stop();second.resolve(status());}
    else second.reject(Error('latest failure'));
    assert.deepEqual(await Promise.all([older,latest]),[false,false]);
    assert.deepEqual(reported,released ? [] : ['git_read_failed']);
  }
});

test('active review follows the newest read without carrying into later refreshes',async()=>{
  for (const reviewFirst of [false,true]) {
    const first=deferred(),second=deferred();let reads=0;
    const {owner,forgotten}=fixture({inspectGit:()=>++reads===1 ? first.promise : reads===2 ? second.promise : Promise.resolve(status())});
    owner.pending={id:'unknown',kind:'index',busy:false,reviewing:false};
    const older=owner.refresh(reviewFirst),latest=owner.refresh(!reviewFirst);
    first.resolve(status());await settle();assert.equal(owner.pending.id,'unknown');
    second.resolve(status());assert.deepEqual(await Promise.all([older,latest]),[true,true]);
    assert.equal(owner.pending,null);assert.deepEqual(forgotten,['unknown']);
    owner.pending={id:'later',kind:'index',busy:false,reviewing:false};
    await owner.refresh();assert.equal(owner.pending.id,'later');assert.deepEqual(forgotten,['unknown']);
  }
});

test('an active document review does not unlock a later uncertain write',async()=>{
  const document=deferred();
  const {owner,forgotten}=fixture({readGitDiff:()=>document.promise,completeRequest:async()=>({Err:{code:'outcome_unknown'}})});
  owner.selected='notes';owner.documents.set('notes',{request:{kind:'file',path:'notes',scope:'all'}});
  owner.pending={id:'earlier',kind:'index',busy:false,reviewing:false};
  const review=owner.refresh(true);await settle();
  assert.equal(owner.pending,null);assert.equal(owner.writable(),true);
  assert.equal(await owner.index(['a.txt'],'stage'),false);assert.equal(owner.pending.id,'stable-request');
  owner.selected=null;await owner.refresh();
  assert.equal(owner.pending.id,'stable-request');assert.deepEqual(forgotten,['earlier']);
  document.resolve({path:'notes',text:'late'});assert.equal(await review,true);
});

test('mutation invalidation does not follow its own refresh promise',async()=>{
  const document=deferred(),receipt=deferred();
  const {owner,reported}=fixture({readGitDiff:()=>document.promise,completeRequest:()=>receipt.promise});
  owner.selected='notes';owner.documents.set('notes',{request:{kind:'file',path:'notes',scope:'all'}});
  const refresh=owner.refresh();await settle();assert.equal(owner.writable(),true);
  const write=owner.index(['a.txt'],'stage');
  document.resolve({path:'notes',text:'late'});assert.equal(await refresh,false);
  receipt.resolve({Err:{code:'outcome_unknown'}});assert.equal(await write,false);
  assert.equal(owner.pending.id,'stable-request');assert.deepEqual(reported,['git_index_unknown']);
});

test('binary and bounded diff states do not produce error notifications',async()=>{
  for (const result of [{binary:true},{truncated:true}]) {
    const {owner,reported}=fixture({readGitDiff:async path=>({path,text:'',...result})});
    await owner.open({kind:'file',path:'a.txt'});
    assert.equal(owner.error,result.binary ? 'git_diff_binary' : 'git_diff_partial');assert.deepEqual(reported,[]);
  }
});

test('unknown writes retain one request until explicit recovery',async()=>{
  const {owner,prepared,completed}=fixture();let attempt=0;
  owner.api.completeRequest=async id=>{completed.push(id);if (++attempt===1) throw new Error('connection lost');return {Ok:{kind:'git_index',data:{revision:'index-2'}}};};
  assert.equal(await owner.index(['a.txt'],'stage'),false);
  assert.equal(owner.pending.id,'stable-request');assert.equal(owner.writable(),false);
  assert.equal(await owner.index(['a.txt'],'stage'),false);assert.equal(prepared.length,1);
  assert.equal(await owner.complete(),true);assert.deepEqual(completed,['stable-request','stable-request']);
  assert.equal(prepared.length,1);assert.equal(owner.pending,null);
});

test('latest read intent opens a tab only when its result is accepted',async()=>{
  const first=deferred(),second=deferred();const {owner}=fixture({readGitDiff:path=>(path==='a'?first:second).promise});
  const a=owner.open({kind:'file',path:'a'}),b=owner.open({kind:'file',path:'b'});
  second.resolve({path:'b',text:'b'});await b;first.resolve({path:'a',text:'a'});await a;
  assert.deepEqual(owner.tabs,['b']);assert.equal(owner.selected,'b');assert.equal(owner.documents.has('a'),false);
});

test('history replaces native pages using the original immutable head',async()=>{
  const reads=[];const {owner}=fixture({readGitLog:async(limit,cursor)=>{reads.push({limit,cursor});return {head:'immutable',offset:cursor?.offset??0,entries:[{id:String(cursor?.offset??0)}],next:cursor?null:{head:'immutable',offset:100}};}});
  await owner.loadHistory();await owner.historyPage(2);assert.deepEqual(reads,[{limit:100,cursor:null},{limit:100,cursor:{head:'immutable',offset:100}}]);
  assert.deepEqual(owner.history.entries,[{id:'100'}]);await owner.historyPage(1);assert.equal(reads[2].cursor.head,'immutable');
});

test('branch creation does not acquire unrelated checkout revisions',async()=>{
  const {owner,prepared}=fixture();await owner.branch('create',{name:'topic',commit:'original'});
  assert.deepEqual(prepared[0],{kind:'create',name:'topic',commit:'original'});
});

test('change groups retain staged additions and mixed index selection',()=>{
  const modified={path:'src/a',staged:'modified',unstaged:'modified',untracked:false,conflicted:false};
  const added={path:'new',staged:'added',unstaged:null,untracked:false,conflicted:false};
  const tree=rows({entries:[modified,added]},{grouping:'tracked',hierarchical:false,sort:'path'},{git_tracked:'Tracked',git_untracked:'New'});
  assert.equal(tree[0].children[0].id,'tracked/src/a');assert.equal(tree[1].children[0].id,'untracked/new');
  assert.deepEqual(checkbox([modified,added]),{checked:false,mixed:true,operation:'stage',paths:['src/a']});
  assert.equal(selectedId(added,'tracked','all'),'untracked/new');
});

test('tracking conflicts retain the original action for explicit stash-and-switch',async()=>{
  const {owner,prepared,forgotten}=fixture({completeRequest:async()=>({Err:{code:'conflict'}})});
  const action={switch_tracking:{remote:'origin/topic',local:'topic',stash:false}};
  assert.equal(await owner.action(action),false);
  assert.equal(owner.pending.kind,'switch');assert.equal(owner.pending.error,'git_branch_conflict');
  assert.deepEqual(owner.pending.action,action);assert.deepEqual(forgotten,[]);
  assert.equal(await owner.index(['a.txt'],'stage'),false);
  assert.equal(prepared.length,1);
});

test('known staging conflicts refresh without replay and preserve visible diff',async()=>{
  const {owner,completed}=fixture({completeRequest:async()=>({Err:{code:'revision_conflict'}}),readGitDiff:async path=>({path,text:'updated',additions:1,deletions:0})});
  await owner.open({kind:'file',path:'a.txt',scope:'staged'});
  assert.equal(await owner.index(['a.txt'],'unstage'),false);
  assert.equal(owner.pending,null);assert.equal(owner.selected,'a.txt');
  assert.equal(owner.documents.get('a.txt').text,undefined);
  assert.equal(owner.documents.get('a.txt').files[0].text,'updated');assert.deepEqual(completed,[]);
});

test('explicit review unlocks an unknown result without replaying it',async()=>{
  let attempts=0;const {owner,forgotten}=fixture({completeRequest:async()=>{attempts++;throw Error('lost response');}});
  await owner.index(['a.txt'],'stage');await owner.refresh();
  assert.equal(owner.pending.id,'stable-request');assert.equal(owner.writable(),false);
  await owner.refresh(true);assert.equal(owner.pending,null);assert.equal(attempts,1);assert.deepEqual(forgotten,['stable-request']);
});

test('closing or replacing a controller rejects a late completion',async()=>{
  const receipt=deferred();const {owner,forgotten}=fixture({completeRequest:()=>receipt.promise});
  owner.message='captured';const pending=owner.commit();owner.stop();
  receipt.resolve({Ok:{kind:'git_commit_created',data:{}}});
  assert.equal(await pending,false);assert.equal(owner.message,'captured');assert.deepEqual(forgotten,[]);
});

test('amend toggles restore the draft and commit scope remains explicit',()=>{
  const {owner}=fixture();owner.branches={sync:{head_message:'Previous'}};owner.message='New draft';
  owner.toggleAmend();assert.equal(owner.message,'Previous');owner.toggleAmend();assert.equal(owner.message,'New draft');
  owner.status.entries=[{path:'tracked',staged:null,unstaged:'modified',untracked:false,conflicted:false},{path:'new',staged:null,unstaged:null,untracked:true,conflicted:false}];
  assert.deepEqual(owner.commitChoices(),['tracked','all']);assert.equal(owner.commitChoices('staged'),null);
  owner.status.entries.shift();assert.deepEqual(owner.commitChoices(),['all']);
});
