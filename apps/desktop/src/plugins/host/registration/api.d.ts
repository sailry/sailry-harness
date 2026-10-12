/** The original registration URL, expiry and credentials remain native-only. */
export type Registration = {
  cursor:string;
  phase:'idle'|'starting'|'pending'|'exchanging'|'connected'|'cancelled'|'failed'|'unconfirmed'|'unavailable';
  configured:boolean; busy:boolean; can_open:boolean; can_replace:boolean;
  fault:string|null;
};
/** Observes this settings page's native owner; it never starts registration. */
export function nextRegistration(cursor?:string):Promise<Registration>;
/** Only a native user click admits an action. Rendering never begins registration.
 * An uncertain original request is inspected, never replayed.
 * replace_pending cancels this owner's exact attempt before a fresh Begin. */
export const RegistrationButton: {
  new(id:string, props:{action:'begin'|'cancel'|'open';label:string;disabled?:boolean;
    replace_pending?:boolean;primary?:boolean}):import('gpui-kit').Element;
};
/** Displays the private original Pending URL as an in-memory QR bitmap. */
export const RegistrationQR: {new(id:string):import('gpui-kit').Element};
