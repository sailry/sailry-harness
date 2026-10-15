/** Opens the existing native schema form for this settings page's own package.
 * No fields or credentials cross the script boundary. Rendering has no side effects. */
export const ConfigurationButton: {
  new(id:string, props:{label:string;disabled?:boolean}): import("gpui-kit").Element;
};
