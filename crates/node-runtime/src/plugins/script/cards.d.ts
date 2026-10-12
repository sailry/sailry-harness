export interface PublicProps { [key: string]: string | number | boolean | null }
export interface Fallback { label: string; locales?: Record<string, string> }
export interface CardDraft { kind: "card"; key: string; props: PublicProps; fallback: Fallback }
export function card(key: string, props: PublicProps, fallback: Fallback): CardDraft;
/** Reads only this captured package's declared HTTP MCP connection state. */
export function status(server: string): Promise<{configured: boolean}>;
