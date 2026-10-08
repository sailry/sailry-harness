"""Exercise the page adapter with browser-like clamping and queued scroll events."""
import json
from pathlib import Path
import subprocess
import unittest


HARNESS = r"""
function fixture(heights, viewport = 600) {
  const frames = [];
  const listeners = new Map();
  let position = 0;
  let queued = false;
  const container = {
    clientWidth: 1032,
    clientHeight: viewport,
    get scrollHeight() {
      return 32 + frames.reduce((height, frame) => height + parseFloat(frame.style.height || 0) + 16, 0);
    },
    get scrollTop() { return position; },
    set scrollTop(value) {
      const next = Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight));
      if (next !== position) queued = true;
      position = next;
    },
    addEventListener(name, callback) { listeners.set(name, callback); },
  };
  globalThis.document = {
    querySelector() { return container; },
    addEventListener() {},
    createElement() {
      const frame = {
        className: '',
        style: {},
        append() {},
        get offsetTop() {
          return 16 + frames.slice(0, frames.indexOf(frame))
            .reduce((height, item) => height + parseFloat(item.style.height || 0) + 16, 0);
        },
      };
      return frame;
    },
  };
  globalThis.window = {
    getSelection() { return { toString: () => '' }; },
    addEventListener() {},
  };
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
  };
  const reports = [];
  const pages = heights.map(height => ({
    classList: { add() {} },
    style: {},
    offsetWidth: 1000,
    offsetHeight: height,
    replaceWith(frame) { frames.push(frame); },
  }));
  mount(pages, value => reports.push(value));
  return {
    container,
    action: value => window.previewAction(value),
    status: () => reports.at(-1),
    flush() {
      if (!queued) return;
      queued = false;
      listeners.get('scroll')();
    },
    scroll(value) { container.scrollTop = value; },
  };
}
"""


class Navigation(unittest.TestCase):
    def run_case(self, body):
        module = Path(__file__).with_name("pages.mjs").resolve().as_uri()
        script = f"import {{ mount }} from {json.dumps(module)};\n" + HARNESS + body
        result = subprocess.run(
            ["node", "--input-type=module"], input=script, text=True,
            capture_output=True, check=True, timeout=5,
        )
        return json.loads(result.stdout)

    def test_preserves_clamped_page(self):
        result = self.run_case("""
const view = fixture([900, 300]);
view.action('next');
const requested = view.status().page;
view.flush();
console.log(JSON.stringify([requested, view.status().page, view.container.scrollTop]));
""")
        self.assertEqual(result, [2, 2, 664])

    def test_navigates_without_overflow(self):
        result = self.run_case("""
const view = fixture([100, 100, 100]);
const pages = [];
for (const action of ['next', 'next', 'previous']) {
  view.action(action);
  view.flush();
  pages.push(view.status().page);
}
console.log(JSON.stringify([pages, view.container.scrollTop]));
""")
        self.assertEqual(result, [[2, 3, 2], 0])

    def test_reports_manual_bottom(self):
        result = self.run_case("""
const view = fixture([900, 100, 100]);
view.scroll(view.container.scrollHeight);
view.flush();
const last = view.status().page;
view.scroll(200);
view.flush();
console.log(JSON.stringify([last, view.status().page]));
""")
        self.assertEqual(result, [3, 1])

    def test_preserves_latest_request(self):
        result = self.run_case("""
const view = fixture([900, 100, 100]);
view.action('next');
view.action('next');
view.action('previous');
view.flush();
console.log(JSON.stringify(view.status().page));
""")
        self.assertEqual(result, 2)

    def test_allows_manual_interruption(self):
        result = self.run_case("""
const view = fixture([900, 300]);
view.action('next');
view.scroll(200);
view.flush();
console.log(JSON.stringify(view.status().page));
""")
        self.assertEqual(result, 1)


if __name__ == "__main__":
    unittest.main()
