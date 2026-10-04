// Existing Databases editor policy from ef5e82ea; secret contents remain native handles.
const copy = value => JSON.parse(JSON.stringify(value));
export function draft(profile,id) {
  const original = profile ? copy(profile) : {id,revision:0,name:'',connection:{kind:'sqlite',path:''},read_only:true,sharing:null};
  const connection = original.connection;
  return {original,engine:connection.kind === 'sqlite' ? 'sqlite' : connection.engine,
    method:connection.kind === 'socket' ? 'socket' : connection.kind === 'ssh' ? 'ssh' : 'tcp',
    tls:connection.tls ?? 'disable',ssh:connection.ssh ?? null,read_only:original.read_only,
    sharing:copy(original.sharing ?? null),projects:[...(original.sharing?.projects ?? [])],
    fields:[original.name,connection.path ?? '',connection.host ?? '',String(connection.port ?? 3306),connection.database ?? '',connection.username ?? '']};
}
export function setSharing(editor,scope) {
  editor.sharing = scope === 'private' ? null : scope === 'global' ? {scope} : {scope,projects:[...editor.projects]};
}
export function switchEngine(editor,engine) {
  if (engine !== editor.engine && engine !== 'sqlite' && ['', '3306','5432'].includes(editor.fields[3])) editor.fields[3] = engine === 'postgres' ? '5432' : '3306';
  editor.engine = engine;
}
export function validSharing(sharing) {
  if (sharing === null || sharing?.scope === 'global') return true;
  return sharing?.scope === 'projects' && Array.isArray(sharing.projects) && sharing.projects.length > 0 && sharing.projects.length <= 128 && new Set(sharing.projects).size === sharing.projects.length;
}
export function prepare(editor,sshProfiles) {
  if (!validSharing(editor.sharing)) throw new Error('invalid sharing');
  const [name,path,host,portText,database,username] = editor.fields;
  let connection;
  if (editor.engine === 'sqlite') connection = {kind:'sqlite',path};
  else {
    const port = editor.method === 'socket' && editor.engine === 'mysql' ? 3306 : /^\d+$/.test(portText) ? Number(portText) : NaN;
    if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('invalid port');
    const common = {engine:editor.engine,port,database,username};
    if (editor.method === 'socket') connection = {kind:'socket',...common,path:path.trim()};
    else if (editor.method === 'ssh') {
      if (!sshProfiles.some(profile=>profile.id === editor.ssh)) throw new Error('SSH connection unavailable');
      connection = {kind:'ssh',...common,host:host.trim(),tls:editor.tls,ssh:editor.ssh};
    } else connection = {kind:'network',...common,host:host.trim(),tls:editor.tls};
  }
  return {...copy(editor.original),name,connection,read_only:editor.read_only,sharing:copy(editor.sharing)};
}
