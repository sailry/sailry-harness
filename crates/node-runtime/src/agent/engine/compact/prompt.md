## Task
You are writing a handoff for yourself. The conversation below reached its context budget, and the next model request will see only this handoff in place of the history. Produce the handoff only: do not continue the task, answer questions, or call tools.

**Conversation History:**
{{ messages }}

## What to keep
Keep only what is needed to continue the work correctly:
- Goal: the current request, the user's latest corrections, and every open explicit task
- Constraints: user requirements that must keep being obeyed, in the user's wording when it matters
- State: what is done, in progress, or blocked. Say precisely whether a change was only modified, tested, committed, or verified live
- Decisions: key choices with a short reason, including rejected options the user ruled out
- Next steps: directly actionable steps that follow the user's latest instruction
- Essential context: exact paths, symbols, commands, error strings, commit, task or child session IDs, and where to retrieve original records again

## Rules
- If the history starts with an earlier summary, merge it with the newer progress into one current handoff. Drop details that are resolved, stale, or duplicated. A merged handoff must not grow just because more work happened: reduce finished work to one line and drop it once it no longer affects the next steps
- Do not copy full source files, diffs, long logs, tool argument lists, or documentation that can be read again. Reference them by path or command instead
- Do not repeat system rules, environment details, skill bodies, or other instructions that are re-injected automatically. Name a skill that is still in use instead of copying it
- Keep key identifiers exact. Never state an unverified guess as a fact; mark uncertainty explicitly
- List only the files needed to continue, usually no more than 15, not every file touched during the session
- Order every list most important first
- Keep the whole handoff under 3,000 tokens, and much less for a simple task. Anything beyond 4,000 tokens is cut from the end of the least essential sections

## Output
Output exactly one ```json code block and nothing else, matching this schema:

```json
{
  "user_intent": ["Goal: the current request and latest corrections, most important first"],
  "pending_tasks": ["open explicit tasks that are not finished"],
  "user_messages": ["constraints and requirements the user expects to keep being followed"],
  "current_work": "State: done, in progress, and blocked work, with modified/tested/committed/verified distinguished",
  "problem_solving": ["Decisions: choice made and the short reason"],
  "next_step": "directly actionable next steps",
  "files": [
    {
      "path": "path needed to continue",
      "summary": "why it matters and its current state"
    }
  ],
  "errors_and_fixes": ["exact unresolved or recently fixed error text and its status"],
  "technical_concepts": ["exact symbols, commands, commit, task or child session IDs, and pointers to original records"]
}
```

Every entry is a plain string except `files`. Omit a field rather than inventing content for it. Only the JSON survives, so it must be self-contained.
