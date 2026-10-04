import test from 'node:test';
import assert from 'node:assert/strict';
import {readdir, readFile} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const namespace = 'dev.sailry.platform';
const chinese = /[\u3400-\u9fff]/;
const placeholders = text => [...text.matchAll(/%\{([^}]+)\}/g)].map(match => match[1]).sort();

async function packages(parent) {
  const result = [];
  for (const directory of await readdir(resolve(root, parent), {withFileTypes: true})) {
    if (!directory.isDirectory()) continue;
    const path = resolve(root, parent, directory.name);
    try {
      result.push({path, manifest: JSON.parse(await readFile(resolve(path, 'plugin.json'), 'utf8'))});
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  return result;
}

function labels(value, context) {
  if (!value || typeof value !== 'object') return;
  if (typeof value.label === 'string') {
    assert.ok(value.label.trim(), context);
    assert.equal(chinese.test(value.label), false, `${context}: English label`);
    assert.ok(value.locales?.['zh-CN']?.trim(), `${context}: Chinese label`);
    assert.deepEqual(placeholders(value.label), placeholders(value.locales['zh-CN']), context);
  }
  for (const [key, child] of Object.entries(value)) labels(child, `${context}.${key}`);
}

test('all packaged labels and descriptions cover both application languages', async () => {
  const entries = [...await packages('plugins'), ...await packages('examples/plugins')];
  assert.deepEqual(entries.map(({manifest}) => manifest.name).sort(), [
    'browser', 'city-trader', 'code-review', 'commands', 'computer', 'context7',
    'databases', 'delegation', 'doudizhu', 'external-browser', 'files',
    'git', 'github', 'goals', 'gomoku', 'liars-dice', 'media', 'memory', 'office',
    'poker', 'progress', 'project-summary', 'reminders', 'reversi', 'scheduled-tasks',
    'ssh', 'statistics', 'task-notes', 'tool-content', 'web-search', 'worktrees', 'xiangqi',
  ].sort());
  for (const {manifest} of entries) {
    const extension = manifest.extensions?.[namespace];
    assert.ok(extension?.description, `${manifest.name}: description`);
    labels(extension, manifest.name);
  }
});

test('desktop dictionaries have matching keys and placeholders without cross-language fallback', async () => {
  for (const {path, manifest} of [...await packages('plugins'), ...await packages('examples/plugins')]) {
    const resource = manifest.extensions?.[namespace]?.desktop?.resources.find(path => path.endsWith('/desktop/locales.js'));
    if (!resource) continue;
    const source = await readFile(resolve(path, resource), 'utf8');
    const dictionaries = vm.runInNewContext(source.replace(/^export /gm, '') +
      '\n;typeof messages === "function" ? {en:messages("en"),zh:messages("zh-CN")} : ' +
      'typeof locales === "undefined" ? {en, zh} : {en:locales.en, zh:locales["zh-CN"]}');
    const en = dictionaries.en, zh = dictionaries.zh;
    assert.deepEqual(Object.keys(en).sort(), Object.keys(zh).sort(), manifest.name);
    for (const [key, english] of Object.entries(en)) {
      if (typeof english !== 'string') continue;
      assert.equal(chinese.test(english), false, `${manifest.name}.${key}: English copy`);
      assert.ok(typeof zh[key] === 'string' && zh[key].trim(), `${manifest.name}.${key}: Chinese copy`);
      assert.deepEqual(placeholders(english), placeholders(zh[key]), `${manifest.name}.${key}`);
    }
  }
});

test('schema-generated settings localize field and tab titles', async () => {
  for (const {path, manifest} of await packages('plugins')) {
    const resource = manifest.extensions?.[namespace]?.settings_schema;
    if (!resource) continue;
    const schema = JSON.parse(await readFile(resolve(path, resource), 'utf8'));
    const localized = schema['x-sailry-locales']?.['zh-CN'];
    for (const [key, field] of Object.entries(schema.properties)) {
      assert.ok(field.title?.trim(), `${manifest.name}.${key}: field title`);
      assert.ok(localized?.[key]?.trim(), `${manifest.name}.${key}: Chinese field title`);
      assert.equal(chinese.test(field.title), false, `${manifest.name}.${key}: English field title`);
    }
    for (const tab of schema['x-sailry-tabs'] ?? []) {
      assert.ok(localized?.[tab.id]?.trim(), `${manifest.name}.${tab.id}: Chinese tab title`);
    }
  }
});

test('host-authored captions carry both languages', async () => {
  for (const {path, manifest} of await packages('plugins')) {
    const resources = manifest.extensions?.[namespace]?.host?.resources ?? [];
    for (const resource of resources.filter(path => /\/host\/(locales|labels)\.js$/.test(path))) {
      const source = await readFile(resolve(path, resource), 'utf8');
      const names = [...source.matchAll(/^export const (\w+)/gm)].map(match => match[1]);
      const expressions = names.map(name => `${name}:${name}`);
      if (/export function present\(/.test(source)) {
        expressions.push('blocked:present({state:"blocked",token_budget:1000})',
          'failed:present({state:"failed",token_budget:null})',
          'cancelled:present({state:"cancelled",token_budget:0})');
      }
      const values = vm.runInNewContext(source.replace(/^export /gm, '') + `\n;({${expressions.join(',')}})`);
      labels(values, `${manifest.name}.${resource}`);
    }
  }
});
