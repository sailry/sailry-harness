import {context,newId,getConversationValue,prepareTransaction,completeRequest,forgetRequest,readSession,readConversationRuns,readSettings,readTurnState,stageTurnState} from 'sailry/sdk';
import {labels,reasons,heading,phase,duration,count,present} from '../labels.js';
import {objectivePart,stateInstruction,continuePrompt,auditPrompt} from './prompts.js';

// One goal per session lives under `goal`; goals the session finished or
// dropped live under `history`. A queued continuation is fenced on `goal`,
// so every write to it withdraws a continuation that has not started.
const key='goal',log='history',kept=20;
const now=()=>Date.now();
// The user's own pause and a goal restored by a fork or rewind wait for
// Resume. Every other pause ends with the user's next message.
const sticky=reason=>reason==='user' || reason==='restored';
const unfinished=goal=>!!goal && goal.status!=='completed';

function fail(message,code='invalid_request') {const error=new Error(message);error.code=code;throw error;}
function bytes(text) {
  let size=0;
  for (const character of text) {const point=character.codePointAt(0);size+=point<=0x7f ? 1 : point<=0x7ff ? 2 : point<=0xffff ? 3 : 4;}
  return size;
}
function clip(text,limit) {
  let size=0,result='';
  for (const character of text) {
    const point=character.codePointAt(0);
    size+=point<=0x7f ? 1 : point<=0x7ff ? 2 : point<=0xffff ? 3 : 4;
    if (size>limit) break;
    result+=character;
  }
  return result;
}
function objective(value) {
  if (typeof value!=='string' || !value.trim()) fail('Describe the goal');
  if (bytes(value.trim())>16384) fail('The goal is longer than 16 KiB');
  return value.trim();
}
function note(value,message) {
  if (typeof value!=='string' || !value.trim()) fail(message);
  return clip(value.trim(),4096);
}
function argumentsOf(args,required,optional=[]) {
  if (!args || typeof args!=='object' || Array.isArray(args) || required.some(name=>!Object.hasOwn(args,name))
    || Object.keys(args).some(name=>!required.includes(name) && !optional.includes(name))) fail('Invalid goal arguments');
}

const blank=(id,text,t)=>({id,objective:text,version:1,status:'active',reason:null,detail:null,seq:0,held_through:0,waiting:false,claim:null,
  evidence:null,edited:false,wrap:null,idle:0,notice:null,turns:0,chain:0,tokens:0,window_tokens:0,elapsed_ms:0,window_ms:0,
  since_ms:t,created_ms:t,finished_ms:null});
function stored(entry) {
  if (!entry.present || !entry.value) return null;
  const value=entry.value;
  if (typeof value.objective==='string') return value;
  // A goal saved by 0.1.x keeps its text and waits for the user.
  const status=value.state==='completed' ? 'completed' : value.state==='blocked' ? 'blocked' : 'paused';
  return {...blank(value.id,value.description,0),status,reason:status==='paused' ? 'stopped' : null,since_ms:null};
}
// Time counts while the goal is active. Every stored value passes through
// here, which closes the open stretch and opens the next one.
function timed(goal,t) {
  const spent=goal.since_ms==null ? 0 : Math.max(0,t-goal.since_ms);
  return {...goal,elapsed_ms:goal.elapsed_ms+spent,window_ms:goal.window_ms+spent,since_ms:goal.status==='active' ? t : null};
}
const open=(goal,t)=>goal.since_ms==null ? 0 : Math.max(0,t-goal.since_ms);
const paused=(goal,reason,detail=null)=>({...goal,status:'paused',reason,detail,waiting:false,claim:null,wrap:null,idle:0});
// Resuming opens a new unattended window; a goal paused at a budget gets
// that budget again, because the user chose to go on.
const resumed=goal=>({...goal,status:'active',reason:null,detail:null,waiting:false,claim:null,wrap:null,idle:0,evidence:null,
  finished_ms:null,notice:null,window_tokens:goal.reason==='tokens' ? 0 : goal.window_tokens,window_ms:goal.reason==='time' ? 0 : goal.window_ms});

async function limits() {
  let values={};
  try {values=(await readSettings()).values ?? {};} catch (_) {}
  const whole=(name,fallback,least,most)=>Number.isInteger(values[name]) && values[name]>=least && values[name]<=most ? values[name] : fallback;
  return {turns:whole('turn_limit',25,1,100),tokens:whole('token_budget',0,0,1000000000)*1000,minutes:whole('time_limit',0,0,100000)};
}
// The session's own latest runs, newest first.
async function recent() {
  const page=await readConversationRuns(null,8);
  return {queue:page.queue_revision,runs:page.runs.filter(run=>run.origin==null),background:page.background ?? null};
}
// The goal as it stands. An active goal whose loop cannot be running is
// shown paused with the cause, so a dead loop is never shown as running:
// history restored by a fork or rewind, a last turn that ended without
// this package hearing of it (a restart, a withdrawn or failed turn), a
// last turn that ended before the Node started, or background work the goal
// waits on that ended with no notice turn to follow (stopped by the user,
// or held at the automatic turn limit).
async function shown(entry,inspect=true) {
  const goal=stored(entry);
  if (!goal) return null;
  if (entry.restored && unfinished(goal) && !(goal.status==='paused' && sticky(goal.reason))) return {...paused(goal,'restored'),since_ms:null};
  if (goal.status!=='active' || !inspect) return goal;
  const page=await recent(),last=page.runs[0];
  let cause=!last ? null : last.status==='interrupted' ? 'restart' : last.status==='failed' ? 'error'
    : last.status==='cancelled' ? (last.error_code==='outcome_unknown' ? 'restart' : 'stopped')
    : last.recovered ? 'restart' : null;
  // Nothing is left to wake a waiting goal: its last turn was handled and
  // no background work will start a notice turn.
  if (!cause && goal.waiting && last && last.sequence<=goal.seq && page.background
    && page.background.running===0 && page.background.pending===0) {
    cause=goal.chain>=(await limits()).turns ? 'turns' : 'stopped';
  }
  return cause ? {...paused(goal,cause),since_ms:null} : goal;
}
function view(goal,revision,lim,t) {
  return {id:goal.id,revision,objective:goal.objective,status:goal.status,state:phase(goal),reason:goal.status==='paused' ? goal.reason : null,
    detail:goal.detail,claim:goal.claim?.note ?? null,evidence:goal.evidence,turns:goal.turns,automatic_turns:goal.chain,turn_limit:lim.turns,
    tokens:goal.tokens,token_budget:lim.tokens,elapsed_ms:goal.elapsed_ms+open(goal,t),time_limit_ms:lim.minutes*60000};
}

async function execute(id) {
  const result=await completeRequest(id);
  if (result.Err) {const error=new Error(result.Err.message);error.code=result.Err.code;error.details=result.Err.details ?? null;throw error;}
  forgetRequest(id);return result.Ok;
}
const commit=operations=>execute(prepareTransaction(operations));
const write=(entry,value)=>({kind:'conversation_write',data:{key,value,expected_revision:entry.revision}});
// The revision one write leaves behind.
const after=entry=>String(BigInt(entry.revision)+1n);
// The continuation is fenced on the goal as the same transaction leaves it.
// `user` marks one the user asked for, which starts a new automatic chain.
const advance=(entry,turn,text,marker,limit,user=false)=>({kind:'continue',data:{session:context().session,after:turn,key,scope:'conversation',
  expected_revision:after(entry),message:{text,references:[],attachments:[]},limit,
  ...(marker ? {marker} : {}),...(user ? {user:true} : {})}});
const refusal=error=>error.details?.kind==='continuation' ? error.details.reason : null;
function notify(goal,kind) {
  const first=goal.objective.split('\n').find(line=>line.trim()) ?? goal.objective;
  return {kind:'notify',data:{title:clip(first.trim(),256),message:clip((kind==='completed' ? goal.evidence : goal.detail) ?? '',4096),
    kind:kind==='completed' ? 'success' : 'warning',session:context().session,presentation:heading(kind,kind==='paused' ? goal.reason : null)}};
}
async function archive(goal,outcome,t) {
  const entry=await getConversationValue(log);
  const item={id:goal.id,objective:goal.objective,outcome,turns:goal.turns,elapsed_ms:goal.elapsed_ms,tokens:goal.tokens,evidence:goal.evidence,
    finished_ms:goal.finished_ms ?? t};
  const items=[item,...(entry.present ? entry.value.items : []).filter(old=>old.id!==goal.id)].slice(0,kept);
  return {kind:'conversation_write',data:{key:log,value:{items},expected_revision:entry.revision}};
}
const outcome=(goal,other)=>goal.status==='completed' ? 'completed' : other;
// A pause the user did not cause is announced once, with its reason.
async function settle(entry,goal,t,announce=false) {
  const value=timed(goal,t);
  await commit([write(entry,value),...(announce ? [notify(value,value.status==='blocked' ? 'blocked' : 'paused')] : [])]);
  return {};
}

export async function read() {
  const entry=await getConversationValue(key);
  const goal=await shown(entry);
  return present(goal && view(goal,entry.revision,await limits(),now()));
}
/** The panel's read: the goal as shown and the session's earlier goals. */
export async function inspect() {
  if (!context().session) return {goal:null,history:[]};
  const entry=await getConversationValue(key),past=await getConversationValue(log);
  const goal=await shown(entry);
  return {goal:goal && view(goal,entry.revision,await limits(),now()),history:past.present ? past.value.items : []};
}
export async function badge() {
  const hidden=[{id:'status',visible:false}];
  if (!context().session) return hidden;
  const entry=await getConversationValue(key);
  const goal=await shown(entry);
  if (!goal) return hidden;
  const value=view(goal,entry.revision,await limits(),now());
  const tone={running:'accent',verifying:'accent',waiting:'muted',paused:'warning',blocked:'warning',completed:'success'}[value.state];
  const tail=value.status==='active' ? [{label:{label:duration(value.elapsed_ms)},tone:'muted'}]
    : value.reason ? [{label:reasons[value.reason],tone:'muted'}] : [];
  return [{id:'status',visible:true,enabled:true,label:labels.goal,
    // The icon and the badge's label say Goal; the capsule holds the state
    // and the time, or the reason once paused.
    segments:[{label:labels[value.state],tone},...tail],
    details:[{label:labels.goal,value:clip(value.objective,4096)},
      {label:labels.turns,value:`${value.automatic_turns} / ${value.turn_limit}`,progress:Math.min(100,value.automatic_turns*100/value.turn_limit)},
      {label:labels.time,value:duration(value.elapsed_ms)+(value.time_limit_ms ? ` / ${duration(value.time_limit_ms)}` : '')},
      {label:labels.tokens,value:count(value.tokens)+(value.token_budget ? ` / ${count(value.token_budget)}` : '')}]}];
}

export async function create(args) {
  argumentsOf(args,['objective']);
  if (readTurnState()?.automatic!==false) fail('Only the user can start a goal. Ask them first','permission_denied');
  const entry=await getConversationValue(key),old=stored(entry),t=now();
  if (unfinished(old)) fail('A goal is already set. Only the user can replace it, with /goal <text>','conflict');
  const goal=blank(newId(),objective(args.objective),t);
  await commit([...(old ? [await archive(old,'completed',t)] : []),write(entry,goal)]);
  return present(view(goal,after(entry),await limits(),t));
}
export async function update(args) {
  argumentsOf(args,['status'],['note']);
  const turn=readTurnState() ?? {},entry=await getConversationValue(key),t=now();
  const goal=await shown(entry,false);
  if (!goal) fail('No goal is set','not_found');
  let next;
  switch (args.status) {
  case 'complete': {
    if (goal.status!=='active') fail('The goal is not active','conflict');
    const evidence=note(args.note,'State the evidence in note');
    // A claim closes the goal only in the verification turn that audits it.
    next=turn.marker==='verify' && goal.claim
      ? {...goal,status:'completed',reason:null,detail:null,waiting:false,claim:null,wrap:null,idle:0,evidence,finished_ms:t,notice:'completed'}
      : {...goal,claim:{note:evidence}};
    break;
  }
  case 'blocked':
    if (goal.status!=='active') fail('The goal is not active','conflict');
    next={...goal,status:'blocked',reason:null,detail:note(args.note,'Say in note what is needed'),waiting:false,claim:null,wrap:null,idle:0,notice:'blocked'};
    break;
  case 'paused':
    if (turn.automatic!==false) fail('Only the user can pause the goal','permission_denied');
    if (goal.status!=='active') fail('The goal is not active','conflict');
    next=paused(goal,'user');
    break;
  case 'active':
    if (turn.automatic!==false) fail('Only the user can resume the goal','permission_denied');
    if (goal.status==='completed') fail('Only the user can reopen a completed goal','permission_denied');
    if (goal.status==='active') fail('The goal is already active','conflict');
    next=resumed(goal);
    break;
  default: fail('Invalid goal status');
  }
  const value=timed(next,t);
  await commit([write(entry,value)]);
  const result=present(view(value,after(entry),await limits(),t));
  if (args.status==='complete' && value.status==='active') result.next='The claim is recorded. End this turn with a short summary; a verification turn follows.';
  return result;
}

export async function initialize(args) {
  stageTurnState({automatic:args.automatic===true,marker:args.continuation?.marker ?? null});
  const plan=args.mode==='plan';
  const tools=plan ? args.tools.filter(name=>name==='get_goal') : args.tools;
  const goal=await shown(await getConversationValue(key),false);
  if (!goal) return {tools,instruction:''};
  const hold=args.automatic===true || goal.status==='paused' && sticky(goal.reason);
  if (goal.status==='completed') return {tools,instruction:stateInstruction(goal,plan,hold)};
  return {tools,instruction:stateInstruction(goal,plan,hold),parts:[objectivePart(goal)]};
}

// after_turn: the one place the loop advances. Each ended main turn either
// continues the goal, leaves it waiting for a turn that is still to end, or
// pauses it with a reason. A queued turn starts as soon as its predecessor
// ends, so two callbacks can overlap, and the user can act at any moment:
// each attempt decides from the goal as stored and commits against that
// revision, and a lost race decides again from the newer goal.
export async function ended(args) {
  for (let attempt=0;;attempt++) {
    try {return await advanceGoal(args);}
    catch (error) {if (error.code!=='revision_conflict' || attempt===3) throw error;}
  }
}
async function advanceGoal(args) {
  const entry=await getConversationValue(key),t=now();
  let goal=stored(entry);
  // A turn older than one already handled must not write: any write would
  // withdraw the continuation the newer turn's callback queued.
  if (!goal || args.sequence<=goal.seq) return {};
  // A reply that finished during a sticky pause stays outside the goal,
  // even when its callback arrives after Resume made the goal active.
  if (!args.automatic && args.sequence<=(goal.held_through ?? 0)) return {};
  if (entry.restored) {
    // Restored history carries the goal but not the right to run it.
    const restored=await shown(entry,false);
    return restored.status===goal.status && restored.reason===goal.reason ? {} : settle(entry,restored,t);
  }
  // A goal turn: the goal was active or closed in it, or it was this
  // package's own continuation, which a Pause may have stopped.
  const pursued=goal.status==='active' || goal.notice!=null || args.continuation!=null;
  const spent=args.usage.input+args.usage.output;
  goal={...goal,seq:args.sequence,chain:args.chain.count,...(pursued ? {tokens:goal.tokens+spent,window_tokens:goal.window_tokens+spent,
    turns:goal.turns+(args.status==='continued' ? 0 : 1)} : {})};
  // The model closed or blocked the goal in this turn.
  if (goal.notice) {
    const kind=goal.notice,value=timed({...goal,notice:null},t);
    await commit([write(entry,value),notify(value,kind)]);
    return {};
  }
  // A turn that hands over to its successor is not an end.
  if (args.status==='continued') return pursued ? settle(entry,goal,t) : {};
  if (goal.status==='completed') return {};
  if (goal.status!=='active') {
    // The user's next message ends every pause but their own; that turn
    // then ran inside the goal.
    if (args.automatic || args.status!=='completed' || args.mode==='plan' || goal.status==='paused' && sticky(goal.reason)) {
      return pursued ? settle(entry,goal,t) : {};
    }
    goal={...resumed(goal),turns:goal.turns+1,tokens:goal.tokens+spent,window_tokens:goal.window_tokens+spent};
  }
  if (args.status==='cancelled') return settle(entry,paused(goal,'stopped'),t);
  if (args.status==='interrupted') return settle(entry,paused(goal,'restart'),t);
  // The Node raises its own notification for a failed automatic turn; the
  // goal shows the reason without a second one.
  if (args.status!=='completed') return settle(entry,paused(goal,'error',args.error ?? null),t);
  if (args.mode==='plan') return settle(entry,paused(goal,'plan'),t);
  if (args.expired>0) return settle(entry,paused(goal,'needs_you'),t,true);
  // Progress is a tool call other than this package's own.
  const waits=args.background.running>0 || args.background.pending>0;
  const own=args.names.reduce((sum,name)=>sum+(args.tools[name] ?? 0),0);
  goal={...goal,idle:!args.automatic || waits || args.tool_calls>own ? 0 : goal.idle+1};
  if (goal.idle>=2) return settle(entry,paused(goal,'no_progress'),t,true);
  // The turn that just ended was the wrap-up turn of a reached limit.
  if (goal.wrap && args.automatic) return settle(entry,paused(goal,goal.wrap),t,true);
  // Background work that will report by itself: wait instead of spending turns.
  if (args.background.running>0) return settle(entry,{...goal,waiting:true,wrap:null},t);
  const lim=await limits();
  if (args.chain.count>=lim.turns) return settle(entry,paused(goal,'turns'),t,true);
  // A claim is audited by the next turn; an audit that ended without
  // confirming drops the claim and work goes on.
  const audited=args.continuation?.marker==='verify',claim=audited ? null : goal.claim;
  const elapsed=goal.window_ms+open(goal,t);
  const wrap=args.chain.count+1>=lim.turns ? 'turns' : lim.tokens && goal.window_tokens>=lim.tokens ? 'tokens'
    : lim.minutes && elapsed>=lim.minutes*60000 ? 'time' : null;
  const facts={count:args.chain.count,limit:lim.turns,tokens:goal.window_tokens,token_budget:lim.tokens,elapsed:Math.floor(elapsed/60000),
    minutes:lim.minutes,unconfirmed:audited,wrap,claim:claim?.note};
  const next=timed({...goal,claim,waiting:false,wrap},t);
  try {
    await commit([write(entry,next),advance(entry,args.turn,claim ? auditPrompt(facts) : continuePrompt(facts),claim ? 'verify' : null,lim.turns)]);
  } catch (error) {
    switch (refusal(error)) {
    // Another turn is queued, running or waiting for the user; its end comes here again.
    case 'queued_input': case 'running': case 'interaction': return settle(entry,{...goal,waiting:true,wrap:null},t);
    // A later turn already ended; its callback owns the next step.
    case 'stale': return {};
    case 'plan': return settle(entry,paused(goal,'plan'),t);
    case 'limit': return settle(entry,paused(goal,'turns'),t,true);
    case 'queue_paused': return settle(entry,paused(goal,'stopped'),t);
    case 'child': case 'archived': case 'worktree': return settle(entry,paused(goal,'error',refusal(error)),t,true);
    }
    // The package was disabled or changed under the callback.
    if (['not_configured','not_found'].includes(error.code)) return {};
    throw error;
  }
  return {};
}

/** The user's controls: the panel's buttons and the /goal subcommands. */
export async function control(args) {
  argumentsOf(args,['kind','goal_id']);
  const entry=await getConversationValue(key),stored_=stored(entry),t=now();
  if (!stored_ || args.goal_id!==stored_.id) fail('The goal changed','conflict');
  const goal=await shown(entry);
  // A loop found dead has no open stretch of time to count.
  const base=goal.status===stored_.status ? stored_ : {...stored_,since_ms:null};
  const stops=async()=>stored_.status!=='active' ? [] : (await recent()).runs.filter(run=>run.turn!==context().turn && ['running','stopping'].includes(run.status))
    .map(run=>({kind:'stop',data:{turn:run.turn}}));
  switch (args.kind) {
  case 'pause':
    if (goal.status!=='active') fail('The goal is not active','conflict');
    await commit([write(entry,timed(paused(base,'user'),t)),...await stops()]);
    break;
  // History keeps the outcome under its stored name, `cleared`.
  case 'end':
    await commit([await archive(timed(base,t),outcome(base,'cleared'),t),{kind:'conversation_remove',data:{key,expected_revision:entry.revision}},...await stops()]);
    break;
  case 'resume': {
    if (goal.status==='active') fail('The goal is already active','conflict');
    let next=resumed(base);
    // A goal does not run in Plan mode; it starts when a turn runs outside it.
    if ((await readSession()).config.mode==='plan') {await commit([write(entry,timed(paused(next,'plan'),t))]);break;}
    const page=await recent(),lim=await limits();
    if (goal.status==='paused' && sticky(goal.reason)) {
      const finished=page.runs.filter(run=>['completed','continued','cancelled','interrupted','failed'].includes(run.status));
      next={...next,held_through:finished.reduce((latest,run)=>Math.max(latest,run.sequence),goal.held_through ?? 0)};
    }
    const unpause={kind:'queue_pause',data:{session:context().session,expected_revision:page.queue,paused:false}};
    const facts={count:0,limit:lim.turns,tokens:next.window_tokens,token_budget:lim.tokens,elapsed:Math.floor(next.window_ms/60000),
      minutes:lim.minutes,resumed:true,wrap:null};
    try {
      await commit([unpause,write(entry,timed(next,t)),advance(entry,null,continuePrompt(facts),null,lim.turns,true)]);
    } catch (error) {
      // A turn is in flight: the goal continues when it ends.
      if (!['queued_input','running','interaction'].includes(refusal(error))) throw error;
      await commit([unpause,write(entry,timed({...next,waiting:true},t))]);
    }
    break;
  }
  default: fail('Invalid goal action');
  }
  return inspect();
}
export async function command(args) {
  const text=args.arguments;
  if (!text) return {...await inspect(),ui_intent:'goal'};
  const [word,...rest]=text.split(/\s+/);
  // `clear` is the earlier word for `end`.
  const action={pause:'pause',stop:'pause',resume:'resume',end:'end',clear:'end'}[word];
  // A control is its word alone, so a new goal may still begin with one.
  if (action && rest.length===0) {
    if (args.message.references?.length || args.message.attachments?.length) fail('Goal controls do not accept references or attachments');
    const existing=stored(await getConversationValue(key));
    if (!existing) fail('No goal is set','not_found');
    return control({kind:action,goal_id:existing.id});
  }
  // Any other text sets the goal, replacing an unfinished one, and is sent
  // as the first turn. In Plan mode that turn plans; pursuit starts after it.
  const session=await readSession(),entry=await getConversationValue(key),old=stored(entry),t=now();
  const fresh=blank(newId(),objective(text),t);
  const goal=session.config.mode==='plan' ? timed(paused(fresh,'plan'),t) : fresh;
  const output=await commit([...(old ? [await archive(timed(old,t),outcome(old,'replaced'),t)] : []),write(entry,goal),
    {kind:'submit',data:{session:context().session,expected_revision:session.revision,message:{...args.message,text}}}]);
  return output.data.at(-1);
}
