export interface PublicProps { [key: string]: string | number | boolean | null }
export interface Fallback { label: string; locales?: Record<string, string> }
export interface CardDraft { kind: "card"; key: string; props: PublicProps; fallback: Fallback }
export function card(key: string, props: PublicProps, fallback: Fallback): CardDraft;
export type ConnectionStatus = {
  package: {name: string; digest: string; settings_revision: number};
  card: string;
  server: string;
  configuration: "ready" | "missing" | "changed" | "unsupported";
  authorization: "not_declared" | "missing" | "configured";
  credential: "not_declared" | "absent" | "present";
};
/** Reads declaration and stored-presence metadata without probing the provider. */
export function connectionStatus(card: string): Promise<ConnectionStatus>;
