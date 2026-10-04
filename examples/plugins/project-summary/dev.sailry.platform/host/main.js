import { inspectGit, readSettings, prepareFile, completeRequest, forgetRequest } from "sailry/sdk";
import { messages } from "../desktop/locales.js";
import { path, report } from "../desktop/report.js";

export async function summarize(input = {}) {
  const settings = await readSettings();
  const include = input.include_untracked ?? settings.values.include_untracked;
  if (typeof include !== "boolean") throw new Error("Invalid report selection");
  const status = await inspectGit();
  return {path, text: report(status, include, messages(input.locale ?? "en"))};
}

export async function save(input) {
  if (!Object.hasOwn(input, "expected_revision")) throw new Error("Expected file revision is required");
  const summary = await summarize(input);
  const id = prepareFile(summary.path, summary.text, input.expected_revision);
  const result = await completeRequest(id);
  forgetRequest(id);
  return result;
}
