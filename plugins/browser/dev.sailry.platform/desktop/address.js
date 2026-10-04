// Address-bar policy from Sailry c4bb60a2 browser desktop/workspace/mod.rs.
// The native browser validates and normalizes the resulting HTTP(S) candidate.
export function destination(input) {
  const value = input.trim();
  if (!value || ['javascript:', 'file:', 'data:'].some(scheme => value.startsWith(scheme))) return null;
  if (value.includes('://')) return value;
  if (!/\s/u.test(value) && (value.includes('.') || value.startsWith('localhost') || value.startsWith('['))) {
    const local = value.startsWith('localhost') || value.startsWith('127.') || value.startsWith('[::1]');
    return `${local ? 'http' : 'https'}://${value}`;
  }
  const query = encodeURIComponent(value)
    .replace(/[!'()~]/g, char => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)
    .replace(/%20/g, '+');
  return `https://www.google.com/search?q=${query}`;
}
