export const path = "project-summary.md";

function byteLength(text) {
  let length = 0;
  for (const character of text) {
    const point = character.codePointAt(0);
    length += point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4;
  }
  return length;
}

export function report(status, include, text) {
  const entries = status.entries.filter((entry) => entry.path !== path && (include || !entry.untracked));
  let body = `# ${text.title}\n\n`;
  if (status.kind === "directory") body += `${text.directory}\n`;
  else if (entries.length === 0) body += `${text.empty}\n`;
  let bytes = byteLength(body);
  let partial = status.truncated;
  for (const entry of entries) {
    // JSON quoting keeps filenames with line breaks unambiguous in the report.
    const line = `- ${JSON.stringify(entry.path)}\n`;
    const length = byteLength(line);
    if (bytes + length > 120 * 1024) {
      partial = true;
      break;
    }
    body += line;
    bytes += length;
  }
  if (partial) body += `\n${text.partial}\n`;
  return body;
}
