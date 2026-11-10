// Public state and login drafts are scoped to the invoking package by the Node.
import {context, modelCall as bridge, prepareRequest} from 'sailry/sdk';
export async function list() { return bridge('read', {kind:'list_connections'}).data; }
export async function types() { return bridge('read', {kind:'list_connector_types'}).data; }
export async function status(connector) { return (await list()).find(row => row.connector === connector) ?? null; }
export function login(input) {
  const {redirect, ...data} = input;
  return prepareRequest({kind:'start_connector_login', data:{...data, package:context().package, input:{redirect:redirect ?? null}}});
}
export function cancel(attempt) { return prepareRequest({kind:'cancel_connector_login', data:{attempt}}); }
