// Pure HTTP flow helpers. Package handlers retain service validation and pagination.
export function text(value, name) {
  if (typeof value !== 'string' || !value.length || /[\u0000-\u001f]/.test(value))
    throw new Error(`Invalid ${name}`);
  return value;
}
export function encoded(value) {
  return encodeURIComponent(value).replace(/[!'()*]/g, char => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
}
export function id(value, name = 'identifier') {
  const segment = text(value, name);
  if (segment === '.' || segment === '..') throw new Error(`Invalid ${name}`);
  return encoded(segment);
}
export function query(values) {
  return Object.entries(values).filter(([, value]) => value !== undefined && value !== null && value !== '')
    .map(([key, value]) => `${encoded(key)}=${encoded(String(value))}`).join('&');
}
export function limit(value = 50, maximum = 100) {
  if (!Number.isSafeInteger(value) || value < 1) throw new Error('limit must be a positive safe integer');
  return {requested_limit:value,page_size:Math.min(value,maximum)};
}
export function failure(message, status = null) {
  return {isError:true,error:{code:'unavailable',message},status};
}
export function response(outcome) {
  if (outcome?.isError) return {error:outcome};
  const data = outcome?.data;
  if (!data || !Number.isInteger(data.status) || typeof data.body !== 'string')
    return {error:failure('Invalid HTTP response')};
  if (data.status < 200 || data.status >= 300)
    return {error:{...failure(`Service request failed (${data.status})`, data.status),
      retry_after:data.headers?.['retry-after']?.[0] ?? null}};
  try {
    const value = JSON.parse(data.body);
    if (!value || typeof value !== 'object' || Array.isArray(value))
      return {error:failure('Service returned an invalid response', data.status)};
    return {value};
  }
  catch { return {error:failure('Service returned invalid JSON', data.status)}; }
}
export function call(url, connector, state = null, options = {}) {
  return {call:{operation:'http.request',arguments:{method:options.method ?? 'GET',url,
    headers:options.headers ?? {Accept:'application/json'},body:options.body ?? null,
    connector,timeout_ms:30000,
    ...(options.connection === undefined ? {} : {connection:options.connection}),
    ...(options.idempotent === undefined ? {} : {idempotent:options.idempotent})}},state};
}
