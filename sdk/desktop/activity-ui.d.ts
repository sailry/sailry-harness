export const CardButton:{new(id:string,props:{label:string;tooltip:string}):import('gpui-kit').Element};
export const ComposedButton:{new(id:string,props:{label:string;tooltip?:string;size?:number}):import('gpui-kit').Element};
export function nextCardEvent():Promise<string>;
export const ClampedText:{new(id:string,props:{text:string;lines?:number;danger?:boolean}):import('gpui-kit').Element};
/** Kit's group hover is not exposed to scripts; children are default and hovered content. */
export const HoverSwap:{new(id:string,props:{group:string}):import('gpui-kit').Element};
/** The same animated indicator used by the conversation's execution phase. */
export const ThinkingIcon:{new(id:string,props:{phase:string}):import('gpui-kit').Element};
/** An elliptical outline filling its parent; running rings carry a travelling highlight. */
export const StatusRing:{new(id:string,props:{color:string;opacity?:number;line?:number;running?:boolean}):import('gpui-kit').Element};
