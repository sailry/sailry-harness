// Connection editor policy from builtin SSH at ef5e82ea (GPL-3.0-only).
// Newly entered secrets and chosen key files remain native opaque draft handles.
const copy = value => JSON.parse(JSON.stringify(value));
export function draft(profile,id) {
  const original = profile ? copy(profile) : {id,revision:0,name:'',host:'',port:22,username:'',authentication:'password',host_key:null,sharing:null};
  return {original,authentication:original.authentication,sharing:copy(original.sharing ?? null),projects:[...(original.sharing?.projects ?? [])],fields:[original.name,original.host,String(original.port),original.username]};
}
export function setSharing(editor,scope) {
  editor.sharing = scope === 'private' ? null : scope === 'global' ? {scope} : {scope,projects:[...editor.projects]};
}
export function prepare(editor,credential) {
  const [name,host,portText,username] = editor.fields;
  const port = /^\d+$/.test(portText) ? Number(portText) : NaN;
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('invalid port');
  const sharing = editor.sharing;
  if (sharing !== null && sharing?.scope !== 'global' && !(sharing?.scope === 'projects' && Array.isArray(sharing.projects) && sharing.projects.length > 0 && sharing.projects.length <= 128 && new Set(sharing.projects).size === sharing.projects.length)) throw new Error('invalid sharing');
  if (editor.authentication === 'agent') throw new Error('unsupported authentication');
  if (editor.authentication === 'key_path' && !credential.file && (editor.original.revision === 0 || editor.original.authentication !== 'key_path')) throw new Error('key file required');
  const changed = editor.authentication === 'password' ? credential.password
    : editor.authentication === 'private_key' ? credential.key : credential.file;
  const replace = editor.original.revision === 0 || editor.original.authentication !== editor.authentication || !!changed;
  const authentication = editor.authentication === 'key_path' && credential.file ? 'private_key' : editor.authentication;
  return {profile:{...copy(editor.original),name,host,port,username,authentication,sharing:copy(sharing)},replace,source:editor.authentication};
}
