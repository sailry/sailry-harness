import {readTurnState,stageTurnState} from 'sailry/sdk';
import {parameters} from './tools.js';

export function initialize({tools,connections,resources}) {
  const profiles = connections.databases;
  stageTurnState({connections:profiles.map(profile=>profile.id)});
  const selected = profiles.length ? tools : [];
  return {tools:selected,instruction:'',
    parameters:Object.fromEntries(selected.map(name=>[name,parameters(name,profiles)]))};
}

function prepare(args,write) {
  const fields = write === null ? ['connection','database'] : ['connection','database','sql'];
  if (!args || typeof args !== 'object' || Array.isArray(args)
      || Object.keys(args).some(key=>!fields.includes(key))
      || typeof args.connection !== 'string'
      || !readTurnState().connections.includes(args.connection)
      || (args.database != null && typeof args.database !== 'string')
      || (write !== null && typeof args.sql !== 'string')) {
    return {isError:true,error:{code:'invalid_request',message:'Invalid database tool arguments'}};
  }
  return write === null ? args : {...args,row_limit:1000,timeout_ms:30000};
}
export function catalog(args) { return prepare(args,null); }
export function query(args) { return prepare(args,false); }
export function execute(args) { return prepare(args,true); }
