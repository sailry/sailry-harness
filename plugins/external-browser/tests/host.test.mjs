import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';

const manifest = JSON.parse(await readFile(new URL('../plugin.json', import.meta.url), 'utf8'));
const extension = manifest.extensions['dev.sailry.platform'];
const source = await readFile(new URL('../dev.sailry.platform/host/main.js', import.meta.url), 'utf8');
const host = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

// Preserve the native adapter's approval groups, including cookie reads and captures.
const read = new Set(`navigate back forward refresh extract_text extract_attribute extract_links
page_info page_source wait_for_element wait wait_for_page_load wait_for_text list_windows
switch_window switch_to_frame switch_to_parent_frame switch_to_default_content downloads`.split(/\s+/));

test('all browser tools dispatch through their declared approval group', () => {
  assert.equal(extension.tools.length, 49);
  assert.equal(new Set(extension.tools.map(tool => tool.name)).size, 49);
  assert.deepEqual(new Set(extension.host.handlers), new Set(Object.keys(host)));
  for (const tool of extension.tools) {
    const action = tool.name.slice('browser_'.length);
    const args = {seconds:0.25, action:'dismiss', text:'页面 🙂', extra:{value:42}};
    const before = structuredClone(args);
    assert.match(tool.name, /^browser_[a-z_]+$/);
    assert.equal(tool.handler.name, action);
    assert.equal(tool.handler.operation, `external_browser.${read.has(action) ? 'read' : 'control'}`);
    assert.equal(tool.handler.result, 'result');
    assert.deepEqual(host[tool.handler.name](args), {action, arguments:before});
    assert.deepEqual(args, before, tool.name);
  }
});

test('wait accepts fractional durations and rejects invalid values before execution', () => {
  for (const seconds of [0, 0.25, 30]) {
    assert.deepEqual(host.wait({seconds}), {action:'wait', arguments:{seconds}});
  }
  for (const seconds of [undefined, null, true, '1', -0.01, 30.01, NaN, Infinity, -Infinity]) {
    assert.deepEqual(host.wait({seconds}), {
      isError:true,
      error:{code:'invalid_request',message:'browser wait must be between 0 and 30 seconds'}
    });
  }
  for (const args of [undefined, null, [], '', 0, false]) {
    assert.equal(host.navigate(args).error.code, 'invalid_request');
  }
});

test('results retain Node ownership, artifacts, truncation and special output shapes', () => {
  for (const data of [
    {execution_node:'node-a',result:{text:'<script>literal content</script>\n页面 🙂'},truncated:false},
    {execution_node:'node-a',result:{artifact:'.generated/browser-image.png'},truncated:true},
    {execution_node:'node-a',closed:true},
    {execution_node:'node-a',files:[{name:'download.bin',size:10}],limit:128},
    {execution_node:'node-a',path:'.generated/browser-download.bin'}
  ]) {
    const output = {kind:'external_browser',data};
    assert.deepEqual(host.result({output}), data);
    assert.equal(host.result({output}), data);
  }
});

test('settings retain the existing navigation without a desktop renderer', () => {
  assert.equal(extension.settings_page.navigation.label, 'External Browser');
  assert.equal(extension.settings_page.entry, undefined);
  assert.equal(extension.desktop, undefined);
});
