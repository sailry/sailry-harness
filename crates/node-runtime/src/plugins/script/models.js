// Model registration and installation remain ordinary prepared Node requests.
import {modelCall as bridge} from 'sailry/sdk';
export async function search(input) { return bridge('read', {kind:'search_models',data:input}).data; }
export async function versions(input) { return bridge('read', {kind:'read_model_versions',data:input}).data; }
export async function resolve(input) { return bridge('read', {kind:'resolve_model',data:input}).data; }
export function register(input) { return bridge('models.register', input); }
export function prepare(input) { return bridge('models.prepare', input); }
