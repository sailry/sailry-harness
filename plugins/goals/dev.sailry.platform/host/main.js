import {context,newId,getConversationValue,setConversationValue,prepareTransaction,completeRequest,forgetRequest,readSession,readConversation,readTurn,readTurnState,stageTurnState} from 'sailry/sdk';
import {present} from './labels.js';

const key='goal';
const continuation='Continue working on the active goal. Verify completion before marking it completed. If you cannot make progress, mark it blocked and explain what is needed. Do not expand the authorized scope.';
const unfinished=goal=>goal && ['active','paused','blocked','failed'].includes(goal.state);
const goal=entry=>entry.present && entry.value ? {...entry.value,revision:entry.revision,
  ...(entry.restored && entry.value.state==='active' ? {state:'paused'} : {})} : null;
async function current(entry=undefined) {
  entry ??= await getConversationValue(key);
  const value=goal(entry);
  if (value?.state!=='active') return {entry,value};
  let history=await readConversation();
  let anchor=history.page.runs.find(run=>run.turn===value.after);
  if (value.after && !anchor) {
    try {anchor=(await readTurn(value.after)).run;}
    catch(error) {
      if (!['not_found','wrong_target'].includes(error.code)) throw error;
      value.state='paused';return {entry,value};
    }
  }
  while (true) {
    if (history.page.runs.some(run=>run.turn!==context().turn && (!anchor || run.sequence>anchor.sequence)
      && ['interrupted','cancelled','failed'].includes(run.status)
      && !(run.status==='cancelled' && run.error?.code==='conflict'))) {
      value.state='paused';break;
    }
    if (history.next_before==null || anchor && history.page.runs.some(run=>run.sequence<=anchor.sequence)) break;
    history=await readConversation(history.next_before);
  }
  return {entry,value};
}
async function anchor() {
  const history=await readConversation();
  return history.page.runs.filter(run=>run.turn!==context().turn).at(-1)?.turn ?? null;
}
function argumentsOf(args,keys) {
  if (!args || typeof args!=='object' || Array.isArray(args) || keys.some(key=>!Object.hasOwn(args,key)) || Object.keys(args).some(key=>!keys.includes(key))) throw new Error('Invalid goal arguments');
}
function description(value) {
  if (typeof value!=='string' || !value.trim() || value.length>8192) throw new Error('Invalid goal description');
  return value.trim();
}
async function execute(id) {
  const result=await completeRequest(id);
  if (result.Err) {const error=new Error(result.Err.message);error.code=result.Err.code;throw error;}
  forgetRequest(id);return result.Ok;
}
const write=(entry,value)=>({kind:'conversation_write',data:{key,value,expected_revision:entry.revision}});
const next=(entry,after=null)=>({kind:'continue',data:{session:context().session,after,key,scope:'conversation',expected_revision:entry.revision,message:{text:continuation,references:[],attachments:[]}}});
export async function read() {return present((await current()).value);}
export async function create(args) {
  argumentsOf(args,['description']);
  const entry=await getConversationValue(key);
  if (unfinished(goal(entry))) throw new Error('Clear the current goal before creating another');
  const value={id:newId(),description:description(args.description),state:'active',after:await anchor()};
  const output=await execute(setConversationValue(key,value,entry.revision));
  stageTurnState({...readTurnState(),executingGoal:true});
  return present(goal(output.data));
}
export async function update(args) {
  argumentsOf(args,['goal_id','expected_revision','description','state']);
  const {entry,value}=await current();
  if (!value || value.state!=='active') throw new Error('No active goal exists');
  if (args.goal_id!==value.id || args.expected_revision!==entry.revision) throw new Error('Goal changed; read it before updating');
  if (!['active','completed','blocked','failed'].includes(args.state)) throw new Error('Invalid goal state');
  const output=await execute(setConversationValue(key,{...entry.value,description:description(args.description),state:args.state},entry.revision));
  return present(goal(output.data));
}
export async function initialize(args) {
  const {value}=await current();
  const executingGoal=value?.state==='active' && args.mode!=='plan';
  stageTurnState({executingGoal});
  return {tools:args.mode==='plan' ? args.tools.filter(name=>name==='get_goal') : args.tools,
    instruction:executingGoal ? `\n\nActive goal: ${value.description}\nKeep working within the authorized scope. A final answer does not complete the goal. Use update_goal only after verifying completion, or mark blocked/failed when work cannot continue.` : ''};
}
export async function observe() {
  const state=(await current()).value?.state;
  return {stop:readTurnState()?.executingGoal===true && state!=='active' && state!=='completed'};
}
export async function completed(args) {
  const {entry,value}=await current();
  if (value?.state!=='active') return {};
  const {run}=await readTurn(args.turn);
  const session=await readSession();
  if (run.status!=='completed' || session.config.mode==='plan') {
    await execute(setConversationValue(key,{...entry.value,state:'paused'},entry.revision));
    return {};
  }
  try {await execute(prepareTransaction([next(entry,args.turn)]));}
  catch(error) {if (!['busy','conflict','revision_conflict','not_configured','not_found'].includes(error.code)) throw error;}
  return {};
}
export async function control(args) {
  const {entry,value}=await current();
  if (!value || args.expected_revision!==entry.revision) throw new Error('Goal changed; read it before updating');
  const operations=[];
  if (args.kind==='pause' || args.kind==='clear') {
    operations.push(args.kind==='clear' ? {kind:'conversation_remove',data:{key,expected_revision:entry.revision}} : write(entry,{...entry.value,state:'paused'}));
    const history=await readConversation();
    if (value.state==='active') for (const run of history.page.runs) if (run.origin==null && ['running','stopping'].includes(run.status)) operations.push({kind:'stop',data:{turn:run.turn}});
  } else if (args.kind==='resume' && ['paused','blocked','failed'].includes(value.state)) {
    const session=await readSession();
    if (session.config.mode==='plan') throw new Error('Goal execution requires Code mode');
    operations.push(write(entry,{...entry.value,state:'active',after:await anchor()}));
    operations.push({kind:'submit',data:{session:context().session,expected_revision:session.revision,message:{text:continuation,references:[],attachments:[]}}});
  } else throw new Error('Invalid goal action');
  await execute(prepareTransaction(operations));
  return read();
}
export async function command(args) {
  const text=args.arguments;
  if (['pause','stop','resume','clear'].includes(text)) {
    if (args.message.references?.length || args.message.attachments?.length) throw new Error('Goal controls do not accept references or attachments');
    const entry=await getConversationValue(key);
    return control({kind:text==='stop'?'pause':text,expected_revision:entry.revision});
  }
  if (!text) return {...await read(),ui_intent:'goal'};
  const session=await readSession();
  if (session.config.mode==='plan') throw new Error('Goal execution requires Code mode');
  const entry=await getConversationValue(key);
  if (unfinished(goal(entry))) throw new Error('Clear the current goal before creating another');
  const value={id:newId(),description:description(text),state:'active',after:await anchor()};
  const output=await execute(prepareTransaction([write(entry,value),{kind:'submit',data:{session:context().session,expected_revision:session.revision,message:{...args.message,text}}}]));
  return output.data[1];
}
