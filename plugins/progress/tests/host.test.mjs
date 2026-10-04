import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const root = new URL('../',import.meta.url);
const source = (await readFile(new URL('dev.sailry.platform/host/main.js',root),'utf8')).replace(/^export /gm,'');
const {update} = vm.runInNewContext(`${source}\n({update})`);
const plain = value => JSON.parse(JSON.stringify(value));
const step = (description,state = 'pending') => ({description,state});

test('a complete snapshot retains parallel work and literal descriptions', () => {
  const plan = {title:'Release',steps:[step(' First\n','in_progress'),step('Second','in_progress'),step('Verified','completed'),step('Unneeded','skipped')]};
  assert.deepEqual(plain(update(plan)),{progress:plan});
  assert.deepEqual(plain(update({title:null,steps:[step('New plan')]})),{progress:{title:null,steps:[step('New plan')]}});
});

test('invalid or oversized plans cannot become authoritative progress', () => {
  for (const args of [null,{}, {title:' ',steps:[step('Work')]},{title:null,steps:[]},
    {title:null,steps:[step('','completed')]},{title:null,steps:[step('Work','done')]},
    {title:null,steps:Array.from({length:257},() => step('Work'))},
    {title:null,steps:[step('界'.repeat(2731))]},
    {title:null,steps:Array.from({length:9},() => step('a'.repeat(8192)))},
    {title:'a'.repeat(513),steps:[step('Work')]},
    {title:null,steps:[{...step('Work'),approved:true}]},
    {title:null,steps:[step('Work')],session:'other'}]) {
    assert.equal(update(args).error.code,'invalid_request');
    assert.equal(update(args).progress,undefined);
  }
});

test('schema and execution preserve the original UTF-8 byte limits', () => {
  assert.ok(update({title:'界'.repeat(170),steps:[step('😀'.repeat(2048))]}).progress);
  assert.equal(update({title:'界'.repeat(171),steps:[step('Work')]}).isError,true);
  assert.ok(update({title:null,steps:Array.from({length:8},() => step('a'.repeat(8192)))}).progress);
});

test('the ordinary package uses canonical progress presentation without session mutation authority', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json',root),'utf8'));
  const extension = manifest.extensions['dev.sailry.platform'], tool = extension.tools[0];
  assert.equal(manifest.name,'progress'); assert.deepEqual(extension.actions,['activity.read']);
  assert.equal(tool.presentation,'progress'); assert.equal(tool.grouping,'standalone');
  assert.deepEqual(tool.contexts,['plugin','workspace']);
  assert.equal(tool.handler.operation,undefined); assert.equal(tool.handler.name,'update');
  assert.equal(tool.handler.parameters.properties.steps.maxItems,256);
});
