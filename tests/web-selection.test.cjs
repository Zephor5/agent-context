const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { test } = require('node:test');
const vm = require('node:vm');

const source = readFileSync(new URL('../src/web/app.js', `file://${__filename}`), 'utf8');
const row = (version = 1) => ({ key: 'repo:test', value: `正文 ${version}`, version, updated_at: '2026-10-27T10:00:00Z' });
const catalog = (rows = [row()]) => ({ root: 'test', board: rows, wiki: [], raw: [] });

// Run the actual app with a minimal DOM adapter; writes count node replacements.
async function app() {
  const elements = new Map(), events = new Map();
  const element = id => {
    if (!elements.has(id)) elements.set(id, {
      value: '', textContent: '', hidden: false, writes: 0,
      set innerHTML(value) { this.html = value; this.writes++; },
      get innerHTML() { return this.html; },
      addEventListener() {}, querySelectorAll() { return []; },
    });
    return elements.get(id);
  };
  const host = {
    selected: false, data: catalog(), requests: 0,
    async respond(path, options) {
      if (path === '/api/catalog') return this.data;
      if (path === '/api/preview') return { html: `<p>${JSON.parse(options.body).text}</p>` };
      throw new Error(`Unexpected request: ${path}`);
    },
    event(name) { events.get(name)?.(); },
  };
  const context = vm.createContext({
    document: {
      getElementById: element, querySelectorAll: () => [],
      createTreeWalker: () => ({ nextNode: () => false }),
    },
    window: {
      getSelection: () => ({ isCollapsed: !host.selected }),
      addEventListener: (name, callback) => events.set(name, callback),
    },
    location: { search: '', origin: 'http://localhost' },
    NodeFilter: { SHOW_TEXT: 4 }, URLSearchParams, URL,
    setTimeout() {}, clearTimeout() {}, setInterval(callback) { host.poll = callback; },
    async fetch(path, options) {
      host.requests++;
      const data = await host.respond(path, options);
      return { ok: true, text: async () => JSON.stringify(data) };
    },
  });
  host.run = code => vm.runInContext(code, context);
  host.element = element;
  await vm.runInContext(source, context);
  await host.run("select('repo:test')");
  return host;
}

function holdRequest(host, path) {
  const respond = host.respond.bind(host);
  let release, started;
  const ready = new Promise(resolve => { started = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  host.respond = async (url, options) => {
    if (url === path) { started(); await gate; }
    return respond(url, options);
  };
  return { ready, release };
}

test('unchanged polls keep list and reader nodes intact', async () => {
  const host = await app();
  const listWrites = host.element('items').writes, readerWrites = host.element('reader').writes;
  await host.poll();
  await host.poll();
  assert.equal(host.element('items').writes, listWrites);
  assert.equal(host.element('reader').writes, readerWrites);
});

test('dragging and selected text defer changes until the selection is cleared', async () => {
  const host = await app();
  const requests = host.requests;
  host.data = catalog([row(2)]);
  host.event('pointerdown');
  await host.poll();
  assert.equal(host.requests, requests);
  host.selected = true;
  host.event('pointerup');
  await host.poll();
  assert.equal(host.requests, requests);
  assert.equal(host.run('state.doc.version'), 1);
  host.selected = false;
  await host.poll();
  assert.equal(host.run('state.doc.version'), 2);
  assert.match(host.element('reader').innerHTML, /正文 2/);
});

test('selection started while catalog is loading prevents DOM replacement', async () => {
  const host = await app(), pending = holdRequest(host, '/api/catalog');
  const writes = host.element('items').writes;
  host.data = catalog([row(2)]);
  const polling = host.poll();
  await pending.ready;
  host.selected = true;
  pending.release();
  await polling;
  assert.equal(host.element('items').writes, writes);
  assert.equal(host.run('state.doc.version'), 1);
  host.selected = false;
  await host.poll();
  assert.equal(host.run('state.doc.version'), 2);
});

test('selection during document loading defers the update and retries next poll', async () => {
  const host = await app(), pending = holdRequest(host, '/api/preview');
  const writes = host.element('reader').writes;
  host.data = catalog([row(2)]);
  const polling = host.poll();
  await pending.ready;
  host.selected = true;
  pending.release();
  await polling;
  assert.equal(host.element('reader').writes, writes);
  assert.equal(host.run('state.doc.version'), 1);
  host.selected = false;
  await host.poll();
  assert.equal(host.run('state.doc.version'), 2);
});

test('external deletion waits for copying to finish', async () => {
  const host = await app();
  host.data = catalog([]);
  host.selected = true;
  await host.poll();
  assert.equal(host.run('state.doc.version'), 1);
  host.selected = false;
  await host.poll();
  assert.equal(host.run('state.doc'), null);
  assert.equal(host.run('state.selected'), null);
});

test('manual refresh remains explicit and edited drafts remain intact', async () => {
  const host = await app();
  host.selected = true;
  host.data = catalog([row(2)]);
  await host.run('refresh(true)');
  assert.equal(host.run('state.doc.version'), 2);
  host.selected = false;
  host.run('state.editing = true');
  host.element('source').value = '未保存的草稿';
  host.data = catalog([row(3)]);
  await host.poll();
  assert.equal(host.run('state.external'), true);
  assert.equal(host.run('state.doc.version'), 2);
  assert.equal(host.element('source').value, '未保存的草稿');
});
