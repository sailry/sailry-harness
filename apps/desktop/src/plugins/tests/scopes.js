import { View, div } from "gpui-kit";
import * as fs from "fs/promises";
import process from "process";
import { context, prepare, execute, forget, next_change } from "sailry";

export default class Probe extends View {
  init(_props, cx) {
    this.results = [];
    cx.spawn(async (cx) => {
      try {
      const record = async (name, action) => {
        try { await action(); this.results.push(`${name}-ALLOWED`); }
        catch (error) {
          if (!error.message.includes("grant") && !error.message.includes("declare")) throw error;
          this.results.push(`${name}-denied`);
        }
      };
      const source = await fs.readFile("resources/dev.sailry.platform/desktop/main.js", "utf8");
      if (source.includes("class Probe")) this.results.push("own-source");
      await record("read", () => fs.readFile("../outside.txt", "utf8"));
      await record("write", () => fs.writeFile("forbidden.txt", "Unexpected write"));
      await record("process", () => process.run("sailry_missing_fixture_process", []));
      await record("network", () => fetch("http://127.0.0.1:9/"));
      await record("clipboard", () => cx.read_from_clipboard());
      await record("storage", () => localStorage.getItem("fixture"));
      await record("watch", () => next_change(""));
      const scope = JSON.parse(context());
      const id = prepare(JSON.stringify({kind: "read_file", data: {worktree: scope.worktree, path: "notes.txt"}}));
      const result = JSON.parse(await execute(id));
      forget(id);
      this.results.push(`node-${result.Err?.code || "ALLOWED"}`);
      const settings = prepare(JSON.stringify({kind: "read_plugin_settings", data: {package: scope.package}}));
      const values = JSON.parse(await execute(settings));
      forget(settings);
      if (values.Ok?.kind === "plugin_settings" && values.Ok.data.values.include_untracked === true) {
        this.results.push("settings-public");
      }
      } catch (error) {
        this.results.push(`failed: ${error.message}`);
      }
      this.results.push("done");
      cx.notify();
    });
  }
  render() { return div().child(this.results.join("\n")); }
}
