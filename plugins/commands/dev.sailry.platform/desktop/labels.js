// Title rules adapted from Sailry 8488e511 plugins/builtin/commands/desktop/terminal/title.rs.
function basename(value) {
  return value.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || value;
}

function identity(value) {
  return /^[\p{L}\p{N}._-]+@[\p{L}\p{N}._-]+$/u.test(value);
}

export function title(value) {
  value = value?.trim() ?? '';
  const colon = value.indexOf(':');
  if (colon >= 0 && identity(value.slice(0, colon))) value = value.slice(colon + 1).trim();
  else if (identity(value)) value = '';
  if (!value) return null;
  return /^(?:\/|~\/|\\\\|.:[\\/])/.test(value) ? basename(value) : value;
}

export function directory(value) {
  if (value?.startsWith('file:')) {
    // OSC 7 can name a remote host; only the displayed path is needed.
    const path = /^file:(?:\/\/[^/\\?#]*)?([^?#]*)/.exec(value)?.[1];
    if (!path?.startsWith('/')) return null;
    try { return basename(decodeURIComponent(path)); }
    catch (_) { return null; }
  }
  value = value?.trim();
  return value ? basename(value) : null;
}
