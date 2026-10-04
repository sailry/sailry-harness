// Package policy uses the admitted role roster; core owns child execution.
import {readTurnState,stageTurnState} from 'sailry/sdk';
const invalid = message => ({isError:true,error:{code:'invalid_request',message}});

export function initialize({tools,roles}) {
  stageTurnState({roles:roles.map(role => role.key)});
  const roster = roles.map(role => `${role.key}: ${role.description.trim() || role.name}`).join('\n');
  const parameters = Object.fromEntries(tools.map(name => [name, {
    type:'object',additionalProperties:false,required:['role','task'],properties:{
      role:roles.length ? {type:'string',enum:roles.map(role => role.key)} : {type:'null'},
      task:{type:'string'},title:{type:'string',minLength:1,maxLength:80},
      worktree:{type:['string','null']},
    },
  }]));
  return {tools,parameters,instruction:roster ? `Available delegation roles:\n${roster}` : ''};
}

export function prepare(args) {
  if (!args || typeof args !== 'object' || Array.isArray(args)
      || Object.keys(args).some(key => !['role','task','title','worktree'].includes(key))) {
    return invalid('Invalid delegation arguments');
  }
  const roles = readTurnState().roles;
  if (roles.length ? !roles.includes(args.role) : args.role !== null) return invalid('Select a role from the admitted roster');
  if (typeof args.task !== 'string' || !args.task.trim()) return invalid('A delegation task is required');
  if (args.title != null && (typeof args.title !== 'string' || !args.title.trim() || [...args.title].length > 80 || /[\n\r]/.test(args.title))) return invalid('Invalid delegation title');
  if (args.worktree != null && (typeof args.worktree !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(args.worktree))) return invalid('Invalid worktree ID');
  return args;
}

export function result({output}) {
  return output.data ?? output;
}
