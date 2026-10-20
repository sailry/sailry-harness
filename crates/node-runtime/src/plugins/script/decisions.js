// Inference runs on the captured Node through the ordinary scoped read boundary.
import {modelCall as bridge} from 'sailry/sdk';
export async function decide(input) { return bridge('read', {kind:'decide',data:input}).data; }
