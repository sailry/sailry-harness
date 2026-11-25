// Model-facing text. The objective is a pinned context part; the per-state
// instruction is fixed text, so neither changes between turns of one state.
// Counters that change every turn go only into continuation messages.

export function objectivePart(goal) {
  return `Goal set by the user for this conversation . Only the user can replace or end it.

<objective>
${goal.objective}
</objective>`;
}

const active=`Goal mode is active. The user set a goal for this conversation; its objective is in the context and stays authoritative.
- Keep working until the objective is fully achieved and verified. A final answer does not end the goal: when a turn ends, the next one starts automatically.
- A message from the user during the goal is guidance inside the goal. Follow it, then carry on. It never changes the objective.
- Do not redefine success around a smaller or easier task. You cannot replace or end the goal; only the user can.
- Stay within the authorized scope and the current permissions. The goal grants no extra authority.
- When the objective is achieved, call update_goal with status "complete" and a note stating the evidence: what was done, how it was checked and the results. A separate verification turn audits the claim before the goal closes.
- If the same obstacle has stopped progress for three turns in a row and only the user can remove it, call update_goal with status "blocked" and a note saying exactly what is needed.
- Use status "paused" or "active" only when the user asks, in their own message, to pause or resume.
- If update_goal is not among your tools, load it with tool_search.`;

const waiting=`A goal is set for this conversation and is on hold. Answer the user's message first and treat it as guidance for the goal unless it is clearly unrelated. The goal resumes automatically after this turn. The message does not change the objective.`;

const held=`A goal is set for this conversation and is on hold. Do not work on it now. Resume it only when the user asks to in their own message, by calling update_goal with status "active".`;

const planning=`A goal is set for this conversation. The session is in Plan mode: plan how to achieve the objective and do not start the work. Pursuit starts when the plan is accepted.`;

const finished=`The goal of this conversation is completed. Do not continue it unless the user reopens it.`;

/** Fixed guidance for the goal's state in this turn. */
export function stateInstruction(goal,plan,hold) {
  if (goal.status==='completed') return finished;
  if (plan) return planning;
  if (goal.status==='active') return active;
  return hold ? held : waiting;
}

const reasons={turns:'the automatic turn limit is reached',tokens:'the token budget is reached',time:'the time limit is reached'};

function budget(facts) {
  const lines=[`This is automatic turn ${facts.count+1} of at most ${facts.limit} since the user's last message.`];
  if (facts.token_budget) lines.push(`Tokens used: ${facts.tokens} of ${facts.token_budget}.`);
  if (facts.minutes) lines.push(`Time used: ${facts.elapsed} of ${facts.minutes} minutes.`);
  return lines;
}

function closing(wrap) {
  return `This is the last automatic turn before the goal pauses, because ${reasons[wrap]}. Do not start new work. Bring the work to a safe state, then report what is done, what remains and the next step, so the user can resume.`;
}

/** The input of an ordinary continuation turn. */
export function continuePrompt(facts) {
  const lines=['Continue working toward the goal.',...budget(facts)];
  if (facts.resumed) lines.push('The user resumed the goal.');
  if (facts.unconfirmed) lines.push('The verification turn did not confirm completion. Finish what remains, and claim completion again only with new evidence.');
  if (facts.wrap) lines.push(closing(facts.wrap));
  else lines.push('Choose the next concrete step from the current state of the work, do it and check the result. Do not repeat finished work or restate the plan.',
    'When the objective is achieved and verified, call update_goal with status "complete" and the evidence. If the same obstacle stops you for the third turn in a row, call update_goal with status "blocked".');
  return lines.join('\n');
}

/** The input of the verification turn that follows a completion claim. */
export function auditPrompt(facts) {
  const lines=[`Verify the goal before it closes. The previous turn claimed completion with this evidence:

<claim>
${facts.claim}
</claim>

Treat the claim as unproven. Re-read the objective and check every requirement against the current state: inspect the files, run the checks or tests and look at real output. Do not rely on the claim or on memory of earlier turns.
- If every requirement is met, call update_goal with status "complete" and a note stating the evidence you verified in this turn.
- If anything is missing, wrong or unverified, do not call update_goal. Say what is missing and continue the work.`,...budget(facts)];
  if (facts.wrap) lines.push(`This is the last automatic turn before the goal pauses, because ${reasons[facts.wrap]}. If the goal is not complete, report what remains and the next step.`);
  return lines.join('\n');
}
