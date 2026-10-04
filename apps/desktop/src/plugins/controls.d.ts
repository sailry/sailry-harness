export const IconButton: {new(id:string,props:{icon:string;label:string;disabled?:boolean;selected?:boolean;tone?:'success'|'muted';variant?:'primary';circular?:boolean;size?:'small'|'medium';show_label?:boolean;full_width?:boolean;dropdown_caret?:boolean}):import('gpui-kit').Element};
export const SelectableRow: {new(id:string,props:{selected:boolean;variant?:'list'|'card';label?:string;disabled?:boolean;stripe?:boolean}):import('gpui-kit').Element};
export const SelectableText: {new(id:string,props:{text:string}):import('gpui-kit').Element};
export const Toggle: {new(id:string,props:{label:string;checked:boolean;disabled?:boolean;variant?:'button'|'checkbox';text?:string}):import('gpui-kit').Element};
export function nextControlEvent():Promise<{id:string;value?:boolean|string|{icon:string;color:string}|{model:string;effort:import('sailry/sdk').Json};click_count?:number}>;
export const ActionScope: {new(id:string,props:{close?:string}):import('gpui-kit').Element};
/** Returns an instance-local key context for native on_action handlers; replaces this view's bindings and removes them on release. Optional context matches descendants within this instance, never global contexts. */
export function registerShortcuts(bindings:{keystroke:string;action:string;context?:string}[]):string;
export const NavigationTabs: {new(id:string,props:{items:{id:string;label:string;icon?:string;loading?:boolean;closable?:boolean;close_disabled?:boolean;dirty?:boolean;close_label?:string}[];selected:string|null;max_width?:number;close_label:string}):import('gpui-kit').Element};
export function nextNavigationTabEvent():Promise<{bar:string;id:string;kind:'select'|'close'}>;

export const SelectField: {new(id:string,props:{label:string;items:{id:string;label:string}[];selected:string|null;placeholder?:string;disabled?:boolean}):import('gpui-kit').Element};
/** Provider-only model and reasoning picker; package confirmation stays draft-local. */
export const ModelPopup: {new(id:string,props:{label:string;placeholder:string;catalog:{models:import('sailry/sdk').Json[]};selected:string|null;effort:import('sailry/sdk').Json;disabled?:boolean}):import('gpui-kit').Element};
export const MenuButton: {new(id:string,props:{label:string;items:{id:string;label:string;disabled?:boolean}[];selected:string|null;disabled?:boolean}):import('gpui-kit').Element};
export const SegmentedTabs: {new(id:string,props:{items:{id:string;label:string}[];selected:string|null;disabled?:boolean}):import('gpui-kit').Element};
export const Appearance: {new(id:string,props:{value:{icon:string;color:string}}):import('gpui-kit').Element};
export const AppearancePicker: {new(id:string,props:{value:{icon:string;color:string};label:string;disabled?:boolean}):import('gpui-kit').Element};

/** Select a known settings host through the application controller. */
export function selectSettingsHost(node:string):void;
