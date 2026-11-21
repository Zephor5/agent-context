import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { apply, attachVault } from '../lib/web-host.js';

const binary = fileURLToPath(
  new URL(
    `../native/${process.platform}-${process.arch}/agent-context${process.platform === 'win32' ? '.exe' : ''}`,
    import.meta.url,
  ),
);
test(
  'the DSH page serves each registered vault through the Web API and preserves conflict checks',
  {
    skip: !existsSync(binary) && !process.env.REQUIRE_NATIVE,
  },
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'context-page-'));
    const routes = new Map(),
      root = {};
    const ctx = {
      root,
      effect: (fn) => fn(),
      connection: {
        fetch: {
          register(route) {
            routes.set(route.path, route);
            return () => routes.delete(route.path);
          },
        },
      },
    };
    const disposers = [];
    try {
      for (const name of ['one', 'two']) {
        const vaultRoot = join(dir, name);
        for (const sub of ['wiki', 'raw']) mkdirSync(join(vaultRoot, sub), { recursive: true });
        disposers.push(attachVault({ root }, { serverName: name, vaultRoot }));
      }
      apply(ctx);
      const listing = routes.get('/api/agent-context/vaults');
      assert.deepEqual(await (await listing.fetch()).json(), ['one', 'two']);
      const api = (vault, path, method = 'GET', body) =>
        routes.get('/api/agent-context/request').fetch(
          new Request('http://localhost/api/agent-context/request', {
            method: 'POST',
            headers: { 'content-type': 'application/json', 'x-agent-context': '1' },
            body: JSON.stringify({ vault, path, method, body }),
          }),
        );
      assert.equal(
        (
          await api('one', '/api/document', 'PUT', {
            path: 'wiki/page.md',
            text: '# Shared',
            expected_revision: null,
          })
        ).status,
        200,
      );
      assert.equal(readFileSync(join(dir, 'one/wiki/page.md'), 'utf8'), '# Shared');
      assert.equal(
        (
          await api('one', '/api/document', 'PUT', {
            path: 'wiki/page.md',
            text: 'stale',
            expected_revision: null,
          })
        ).status,
        409,
      );
      assert.equal((await api('two', '/api/catalog')).status, 200);
      assert.equal(existsSync(join(dir, 'two/wiki/page.md')), false);
      assert.notEqual(
        (
          await api('one', '/api/document', 'PUT', {
            path: 'raw/one.md',
            text: 'denied',
            expected_revision: null,
          })
        ).status,
        200,
      );
      assert.equal(
        (await api('one', '/api/board', 'POST', { key: 'test', value: 'hello', expect_version: 0 })).status,
        200,
      );
      assert.equal(
        (await api('one', '/api/board', 'DELETE', { key: 'test', expect_version: 1 })).status,
        200,
      );
      assert.equal((await api('one', 'https://example.com')).status, 400);
      disposers[0]();
      assert.equal((await api('one', '/api/catalog')).status, 404);
      assert.deepEqual(await (await listing.fetch()).json(), ['two']);
    } finally {
      for (const dispose of disposers) dispose();
      rmSync(dir, { recursive: true, force: true });
    }
  },
);
