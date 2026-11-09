/** Pure v1 translations. Node owns HTTP framing, credentials and cancellation. */
declare module 'sailry/adapter' {
  export const version: 'v1';
  /** Parses one already separated payload; it does not read or frame a stream. */
  export function parseJson(data: string): Json;
  export type Json = null | boolean | number | string | Json[] | {[name: string]: Json};
  export type Effort = 'default' | 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max' | {budget: number};
  export type Framing = 'sse' | 'ndjson' | 'json';
  export type Image = {kind: 'inline'; data_base64: string} | {kind: 'url'; url: string};
  export type Part =
    | {kind: 'text'; text: string}
    | {kind: 'image'; media_type: string; data: Image}
    | {kind: 'file'; media_type: string; name?: string | null; data_base64: string}
    | {kind: 'reasoning'; text?: string | null; replay?: string | null}
    | {kind: 'tool_call'; id: string; name: string; arguments: Json}
    | {kind: 'json'; value: Json};
  export type Message =
    | {role: 'user' | 'assistant'; parts: Part[]}
    | {role: 'tool'; call_id: string; name: string; parts: Part[]; is_error: boolean};
  export interface Tool {name: string; description?: string | null; parameters: Json}
  export type Choice = {kind: 'auto' | 'none' | 'required'} | {kind: 'named'; name: string};
  export interface Request {
    version: 'v1'; model: string; messages: Message[]; system?: string | null;
    tools: Tool[]; tool_choice: Choice; reasoning: {effort: Effort; replay: boolean};
    features: Record<string, string>;
    max_output_tokens?: number | null; temperature?: number | null; top_p?: number | null;
    stop: string[]; stream: boolean; session: string; endpoint: string; options: Record<string, Json>;
  }
  export interface Prepared {
    method: 'GET' | 'POST'; url: string; headers?: Record<string, string>; body?: string | null;
    /** Node replaces placeholders without giving their values to the module. */
    credential?: {header: string; template: string} | null; framing?: Framing | null;
  }
  export interface Event {event?: string | null; data: string}
  export interface Decode {
    state: Json; events: Event[]; done: boolean;
    status?: number | null; headers?: Record<string, string[]> | null;
  }
  export type Finish = 'stop' | 'length' | 'tool_calls' | 'content_filter' | 'other';
  export type Class = 'authentication' | 'permission' | 'rate_limited' | 'quota'
    | 'context_overflow' | 'invalid_request' | 'unavailable' | 'unsupported' | 'unknown';
  export interface Classified {class: Class; message: string; retry_after_ms?: number | null; code?: string | null; error_type?: string | null; event_type?: string | null; incomplete_reason?: string | null}
  export type Delta =
    | {kind: 'text'; text: string}
    | {kind: 'reasoning'; text?: string | null; replay?: string | null}
    | {kind: 'tool_call'; index: number; id?: string | null; name?: string | null; arguments_delta?: string | null}
    | {kind: 'usage'; input_tokens?: number | null; output_tokens?: number | null; cached_input_tokens?: number | null; reasoning_tokens?: number | null}
    | {kind: 'finish'; reason: Finish}
    | ({kind: 'error'} & Classified);
  export interface Decoded {state: Json; deltas: Delta[]}
}
