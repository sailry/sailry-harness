// GitHub device sign-in followed by the Copilot session exchange, executed by
// the shared private auth host. Behaviour follows Sailry's earlier native
// login; current wire values were reviewed against the sources in NOTICE.
const ENDPOINT = 'https://api.githubcopilot.com';
const VERIFICATION = 'https://github.com/login/device';
const MISSES = 5;
const reject = code => ({kind: 'reject', code});
const text = value => typeof value === 'string' && value.length > 0 && value.length <= 16384 && !/[\x00-\x1f\x7f]/.test(value);
const request = (name, fields, state) => ({kind: 'request', name, fields, state});
const seconds = (value, fallback) => Math.min(900, Math.max(1, Number.isSafeInteger(value) ? value : fallback));
const wait = state => ({kind: 'wait', delay_ms: state.interval * 1000, state: {...state, phase: 'waiting'}});
const answered = event => event.kind === 'response' && event.status >= 200 && event.status < 300 && event.body && typeof event.body === 'object' && !Array.isArray(event.body);

function poll(event, state) {
  if (event.kind !== 'response') return reject('invalid_request');
  const body = event.body && typeof event.body === 'object' && !Array.isArray(event.body) ? event.body : {};
  if (text(body.access_token)) return request('session', {github: body.access_token}, {phase: 'session', github: body.access_token});
  switch (body.error) {
    case 'authorization_pending': return wait({...state, misses: 0});
    case 'slow_down': return wait({...state, misses: 0, interval: Math.min(900, Math.max(state.interval + 5, seconds(body.interval, 1)))});
    case 'expired_token': return reject('expired');
    case 'access_denied': return reject('permission_denied');
  }
  // GitHub answers a poll with a JSON document. An answer without one is a
  // passing fault of the service, tolerated a few times in a row.
  if (body.error == null && (event.status === 429 || event.status >= 500) && state.misses + 1 < MISSES) return wait({...state, misses: state.misses + 1});
  return reject('invalid_request');
}

function session(event, state, now) {
  if (event.kind !== 'response') return reject('invalid_request');
  // 401: GitHub no longer accepts the token. 403 and 404: the account has no
  // Copilot subscription this exchange serves.
  if ([401, 403, 404].includes(event.status)) return reject('permission_denied');
  if (!answered(event) || !text(event.body.token) || !Number.isSafeInteger(event.body.expires_at) || !Number.isSafeInteger(event.body.expires_at * 1000)) return reject('invalid_request');
  const expiry = event.body.expires_at * 1000;
  if (expiry <= now) return reject('expired');
  // The service endpoint is fixed: this version serves github.com accounts only.
  return {kind: 'finish', grant: {kind: 'copilot', access: event.body.token, github: state.github, expires_at_ms: expiry, endpoint: ENDPOINT, renewal_pending: false}};
}

export function authorize({event, context, state, now_ms: now}) {
  if (!text(context.fields?.client_id)) return reject('invalid_request');
  if (event.kind === 'refresh') {
    const github = context.previous?.github;
    return text(github) ? request('session', {github}, {phase: 'session', github}) : reject('permission_denied');
  }
  if (event.kind === 'start') return request('device', {scope: context.scopes.join(' ')}, {phase: 'device'});
  switch (state?.phase) {
    case 'device': {
      if (!answered(event)) return reject('invalid_request');
      const {device_code: device, user_code: code, verification_uri: address} = event.body;
      if (!text(device) || !text(code) || code.length > 256 || address !== VERIFICATION) return reject('invalid_request');
      return {kind: 'prompt', url: VERIFICATION, user_code: code, expires_at_ms: now + seconds(event.body.expires_in, 900) * 1000,
        state: {phase: 'presented', device, interval: seconds(event.body.interval, 5), misses: 0}};
    }
    case 'presented': return event.kind === 'presented' ? wait(state) : reject('invalid_request');
    case 'waiting': return event.kind === 'elapsed' ? request('token', {device_code: state.device}, {...state, phase: 'polling'}) : reject('invalid_request');
    case 'polling': return poll(event, state);
    case 'session': return session(event, state, now);
    default: return reject('invalid_request');
  }
}
