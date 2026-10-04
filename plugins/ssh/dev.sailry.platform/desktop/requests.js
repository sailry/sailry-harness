// A pending command keeps its exact durable identity; checking never dispatches it again.
import {prepareRequest,completeRequest,forgetRequest} from 'sailry/sdk';
import {requestOutcome} from 'sailry/connections';

export class Request {
  constructor(command,id) { this.command=command; this.id=id ?? prepareRequest(command); this.running=false; this.unknown=false; this.output=null; this.error=null; }
  async start(cx) {
    if (this.running || this.unknown || this.output || this.error) return;
    this.running=true; cx.notify();
    try { this.accept(await completeRequest(this.id)); }
    catch (_) { this.unknown=true; }
    finally { this.running=false; cx.notify(); }
  }
  accept(result) {
    if (Object.hasOwn(result,'Ok')) { this.output=result.Ok; this.unknown=false; }
    else if (result.Err?.code === 'outcome_unknown') { this.uncertainty=result.Err; this.unknown=true; }
    else if (result.Err) { this.error=result.Err; this.unknown=false; }
    else this.unknown=true;
  }
  async check(cx) {
    if (this.running || !this.unknown) return;
    this.running=true; cx.notify();
    try {
      const outcome=await requestOutcome(this.id);
      if (outcome.kind === 'completed') this.accept(outcome.data);
      else if (outcome.kind === 'not_admitted') this.accept({Err:{code:'unavailable',message:'Request was not admitted'}});
    } finally { this.running=false; cx.notify(); }
  }
  async cancel() {
    if (!['run_ssh','check_ssh','open_ssh_terminal','transfer_ssh','install_host'].includes(this.command?.kind)) return;
    const id=prepareRequest({kind:'cancel_ssh',data:{request:this.id}});
    try { await completeRequest(id); } finally { forgetRequest(id); }
  }
  get done() { return !!this.output || !!this.error; }
  release() { if (this.done) forgetRequest(this.id); }
}

export function failure(error) {
  return error?.code === 'outcome_unknown' ? 'ssh_unknown'
    : ['revision_conflict','not_found'].includes(error?.code) ? 'ssh_conflict'
    : error?.code === 'cancelled' ? 'ssh_cancelled' : error?.code === 'unavailable' ? 'ssh_unavailable'
    : error?.code === 'permission_denied' ? 'ssh_auth_failed' : error?.code === 'busy' ? 'ssh_busy' : 'ssh_failed';
}
