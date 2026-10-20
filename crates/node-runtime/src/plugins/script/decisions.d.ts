/** sailry/decisions: bounded, read-only local inference on the selected Node. */
export type Question = {type:'noul'; instructions:string; criteria?:Partial<Record<'false'|'true',string>>}
  | {type:'choice'; instructions:string; criteria:Record<string,string|null>}
  | {type:'score'; instructions:string; criteria:string[]};
export type Request = {model?:string|null; state:string; questions:Record<string,Question>};
export type Answer = {type:'noul'; noul:number}
  | {type:'choice'; choice:string; probabilities:Record<string,number>; confidence:number}
  | {type:'score'; score:number; legend:string[]; probabilities:number[]; confidence:number};
export type Response = {answers:Record<string,Answer>; usage:{input_tokens:number; output_tokens:number}};
/** Requires decisions.run; available to read-only tools and callbacks. */
export function decide(input:Request): Promise<Response>;
