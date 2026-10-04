import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import test from 'node:test';
const source = readFileSync(new URL('./shortcuts.js', import.meta.url), 'utf8');
const stroke = (key, meta = false) => ({ key, meta, ctrl: false, alt: false, shift: false });
function fixture() {
    const listeners = new Set();
    const messages = [];
    const context = { window: { ipc: { postMessage: value => messages.push(value) } },
        performance: { now: () => 100 },
        addEventListener: (_, listener) => listeners.add(listener),
        removeEventListener: (_, listener) => listeners.delete(listener) };
    return {
        apply(bindings) { runInNewContext(`(() => {const bindings = ${JSON.stringify(bindings)}; ${source}})()`, context); },
        press(key, meta = false) {
            let consumed = false;
            for (const listener of listeners) listener({ key, metaKey: meta, ctrlKey: false, altKey: false, shiftKey: false,
                preventDefault() { consumed = true; }, stopPropagation() {} });
            return consumed;
        }, messages, listeners,
    };
}
test('replaces bindings and supports disabling', () => {
    const view = fixture();
    view.apply([{ strokes: [stroke('w', true)], action: 'close-tab' }]);
    assert.equal(view.press('w', true), true);
    view.apply([{ strokes: [stroke('j', true)], action: 'close-tab' }]);
    assert.equal(view.listeners.size, 1);
    assert.equal(view.press('w', true), false);
    assert.equal(view.press('j', true), true);
    view.apply([]);
    assert.equal(view.press('j', true), false);
    assert.deepEqual(view.messages, ['close-tab', 'close-tab']);
});
test('matches sequences and browser key aliases', () => {
    const view = fixture();
    view.apply([{ strokes: [stroke('g', true), stroke('down')], action: 'inspect' }]);
    assert.equal(view.press('g', true), true);
    assert.deepEqual(view.messages, []);
    assert.equal(view.press('ArrowDown'), true);
    assert.deepEqual(view.messages, ['inspect']);
    assert.equal(view.press('ArrowDown'), false);
});
