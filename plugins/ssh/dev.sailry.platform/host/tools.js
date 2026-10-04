// Tool definitions belong to the ordinary package; sessions and SSH stay native.
export const definitions = {
  ssh_run:{
    description:"Run a command on an allowed saved SSH connection after runtime approval. The connection must already have a trusted host key. Do not request credentials or bypass host-key verification.",
    handler:'run',operation:'ssh.run',presentation:'details',
  },
  ssh_transfer:{
    description:"Upload or download a file using an allowed SSH connection after runtime approval. The local path is relative to the session worktree. Do not request credentials.",
    handler:'transfer',operation:'ssh.transfer',presentation:'details',
  },
};
export function parameters(name,profiles) {
  const choices = profiles.map(profile=>[profile.id,profile.name]);
  const properties = {connection:{type:'string',enum:choices.map(([id])=>id),description:JSON.stringify(choices)}};
  const required = ['connection'];
  if (name === 'ssh_run') {
    properties.command={type:'string'};
    properties.timeout_ms={type:'integer',minimum:1,maximum:900000};
    required.push('command');
  } else {
    properties.path={type:'string'};properties.remote_path={type:'string'};
    properties.direction={type:'string',enum:['upload','download']};
    required.push('path','remote_path','direction');
  }
  return {type:'object',properties,required,additionalProperties:false};
}
