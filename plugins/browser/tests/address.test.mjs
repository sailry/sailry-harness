import assert from 'node:assert/strict';
import test from 'node:test';
import {destination} from '../dev.sailry.platform/desktop/address.js';

test('prepares addresses for native URL validation and normalization', () => {
  for (const [input, expected] of [
    ['example.com', 'https://example.com'],
    ['localhost:8080/a', 'http://localhost:8080/a'],
    ['https://example.com/path?q=a', 'https://example.com/path?q=a'],
    ['  example.com  ', 'https://example.com'],
    ['127.0.0.1:3000', 'http://127.0.0.1:3000'],
    ['[::1]:3000/a', 'http://[::1]:3000/a'],
    ['[2001:db8::1]/a', 'https://[2001:db8::1]/a']
  ]) assert.equal(destination(input),expected);
});

test('searches non-address text with form-encoded query values', () => {
  for (const query of ['two words', 'a & b = c', "symbols !'()~*", '中文 🙂']) {
    const url = new URL(destination(` ${query} `));
    assert.equal(url.origin,'https://www.google.com');
    assert.equal(url.pathname,'/search');
    assert.equal(url.searchParams.get('q'),query);
  }
  assert.equal(destination('two words'),'https://www.google.com/search?q=two+words');
});

test('rejects empty input and schemes that must not become searches', () => {
  for (const input of ['', '  ', 'javascript:alert(1)', 'file:///tmp/file', 'data:text/html,test']) {
    assert.equal(destination(input),null);
  }
  // URL syntax and protocol acceptance remain the native capability's checks.
  assert.equal(destination('https://'),'https://');
  assert.equal(destination('ftp://example.com'),'ftp://example.com');
});
