/** View-local Kit editing state; release when the draft closes. No Node writes. */
export function createText(value: string, options?: {
  multiline?: boolean; rows?: [number, number]; placeholder?: string; label?: string;
}): string;
export function readText(handle: string): string;
export function releaseText(handle: string): void;
/** The handle is the component ID. Renders native Kit Input or Textarea. */
export const TextField: {
  new(handle: string, props?: { disabled?: boolean; readonly?: boolean; appearance?: boolean; bordered?: boolean; height?: number; size?: "small"; adornment?: "prefix"|"suffix" }): import("gpui-kit").Element;
};
/** Native Kit NumberInput over the same exact text handle. Step events never round the value. */
export const NumberField: {
  new(handle:string, props?:{disabled?:boolean;readonly?:boolean;size?:"small"}):import("gpui-kit").Element;
};
/** Milliseconds since epoch, edited in the controller's local timezone. */
export function createDateTime(value: number, label: string): string;
/** Rejects missing, invalid and ambiguous local times; preserves unchanged milliseconds. */
export function readDateTime(handle: string): number;
export function releaseDateTime(handle: string): void;
export const DateTimeField: {
  new(handle: string, props?: { disabled?: boolean; size?: "small" }): import("gpui-kit").Element;
};

export function setText(handle:string, value:string): void;
export function focusText(handle:string): void;
export function isTextFocused(handle:string): boolean;
export function nextTextEvent(): Promise<{id:string;kind:'enter'|'change'|'focus'|'blur'|'increment'|'decrement'}>;

/** View-local styled Kit Slider. Programmatic values do not emit user events. */
export function createSlider(value:number, options:{min:number;max:number;step:number;label?:string}):string;
export function readSlider(handle:string):number;
export function setSlider(handle:string,value:number):void;
export function releaseSlider(handle:string):void;
export function nextSliderEvent():Promise<{id:string;kind:'change'|'release';value:number}>;
export const SliderField:{new(handle:string,props?:{disabled?:boolean}):import("gpui-kit").Element};
