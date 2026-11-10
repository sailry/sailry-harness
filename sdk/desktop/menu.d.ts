export type MenuItem = {id?:string;label?:string;icon?:string;separator?:boolean;header?:boolean;enabled?:boolean;checked?:boolean;children?:MenuItem[];shortcut?:string;value?:unknown};
export const Menu: {new(id:string,props:{label:string;icon?:string;disabled?:boolean;form?:boolean;small?:boolean;caret?:boolean;size?:'small'|'medium';items:MenuItem[];primary?:MenuItem}):import("gpui-kit").Element};
export function nextMenuEvent():Promise<{menu:string;id:string;value?:unknown}>;
export type NativeMenuItem = {id?: string; label?: string; enabled?: boolean; separator?: boolean; value?: unknown};
export const NativeContextMenu: {new(id: string, props: {items: NativeMenuItem[]; fill?: boolean}): import("gpui-kit").Element};
export function nextContextMenuEvent(): Promise<{menu: string; id: string; value?: unknown}>;
