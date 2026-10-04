import {readTurnState,stageTurnState} from 'sailry/sdk';
import {parameters} from './tools.js';

export function initialize({tools,connections,resources}) {
  const profiles = connections.ssh;
  stageTurnState({connections:profiles.map(profile=>profile.id)});
  const selected = profiles.length ? tools : [];
  return {tools:selected,instruction:'',
    parameters:Object.fromEntries(selected.map(name=>[name,parameters(name,profiles)]))};
}
const invalid = () => ({isError:true,error:{code:'invalid_request',message:'Invalid SSH tool arguments'}});
function target(args,fields) {
  return args && typeof args === 'object' && !Array.isArray(args)
    && !Object.keys(args).some(key=>!fields.includes(key))
    && typeof args.connection === 'string' && readTurnState().connections.includes(args.connection);
}
export function run(args) {
  if (!target(args,['connection','command','timeout_ms']) || typeof args.command !== 'string'
      || (args.timeout_ms != null && (!Number.isSafeInteger(args.timeout_ms) || args.timeout_ms < 1 || args.timeout_ms > 900000))) return invalid();
  return {...args,timeout_ms:args.timeout_ms ?? 120000};
}
export function transfer(args) {
  if (!target(args,['connection','path','remote_path','direction'])
      || typeof args.path !== 'string' || typeof args.remote_path !== 'string'
      || !['upload','download'].includes(args.direction)) return invalid();
  return {...args,timeout_ms:120000};
}
