// The UI interprets the owning assistant's canonical calls; it never writes history.
const later=(value,prior)=>value != null && (prior == null || value.length>prior.length || value.length===prior.length && value>prior);
export function updates(snapshot,previous) {
  const live=previous?.connected && snapshot.session === previous.session && snapshot.revision === previous.revision;
  const prior=new Map((previous?.calls ?? []).map(call=>[call.key,call]));
  return (snapshot.calls ?? []).filter(call=>['database_query','database_execute'].includes(call.name))
    .filter(call=>JSON.stringify(prior.get(call.key)) !== JSON.stringify(call))
    .map(call=>({...call,live:!!live && (call.sequence ? later(call.sequence,previous.latest) : prior.has(call.key))}));
}
export function result(call) {
  if (call.result?.kind === 'database_outcome' && call.result.data?.kind === 'query') return {rows:call.result.data.data};
  if (call.result?.error) return {error:call.result.error};
  if (['cancelled','not_executed','interrupted'].includes(call.state)) return {error:{code:'cancelled'}};
  return {};
}
