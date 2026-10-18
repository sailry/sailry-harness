// Pure translation helpers. No SDK, host services or I/O are installed.
export const version = 'v1';

/** Parse one JSON payload after the Node has separated its transport frames. */
export function parseJson(data) {
  return JSON.parse(data);
}
