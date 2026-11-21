import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { promisify } from 'node:util';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { after, test } from 'node:test';
import { mountAgentContext } from '../lib/mount.js';
import { selectVault } from '../lib/vault.js';

const root = mkdtempSync(join(tmpdir(), 'dsh-oneclick-'));
after(() => rmSync(root, { recursive: true, force: true }));
const options = () => {
  const home = mkdtempSync(join(root, 'home-'));
  const env = { AGENT_CONTEXT_ROOT: '', HOME: home, USERPROFILE: home, PATH: join(home, 'no-tools'), XDG_DATA_HOME: join(home, 'data'), XDG_CONFIG_HOME: join(home, 'config') };
  if (process.env.SystemRoot) env.SystemRoot = process.env.SystemRoot;
  return { homedir: home, env };
};
const vault = dir => {
  for (const sub of ['wiki', 'raw']) mkdirSync(join(dir, sub), { recursive: true });
  return dir;
};

test('prefers configured, environment, remembered and conventional vaults in order', () => {
  const opts = options();
  const saved = vault(join(opts.homedir, 'saved'));
  const conventional = vault(join(opts.homedir, 'agent-context'));
  assert.equal(selectVault('', '', opts).root, conventional);
  const config = join(opts.env.XDG_CONFIG_HOME, 'agent-context', 'web.json');
  mkdirSync(dirname(config), { recursive: true });
  writeFileSync(config, JSON.stringify({ root: saved }));
  assert.equal(selectVault('', '', opts).root, saved);
  opts.env.AGENT_CONTEXT_ROOT = conventional;
  assert.equal(selectVault('', '', opts).root, conventional);
  assert.equal(selectVault(saved, '', opts).root, saved);
  assert.throws(() => selectVault(join(saved, 'absent'), '', opts), /does not exist/);
  delete opts.env.AGENT_CONTEXT_ROOT;
  writeFileSync(config, JSON.stringify({ root: join(saved, 'absent') }));
  assert.throws(() => selectVault('', '', opts), /does not exist/);
});

test('leaves unrelated data untouched after failed initialization', () => {
  const opts = options();
  const dest = join(opts.env.XDG_DATA_HOME, 'agent-context', 'vault');
  assert.throws(() => selectVault('', join(root, 'absent-cli'), opts));
  assert.equal(existsSync(dest), false);
  mkdirSync(dest, { recursive: true });
  writeFileSync(join(dest, 'mine'), 'existing');
  assert.throws(() => selectVault('', '', opts), /not an agent-context vault/);
  assert.equal(existsSync(join(dest, 'mine')), true);
});

const exe = process.platform === 'win32' ? 'agent-context.exe' : 'agent-context';
const bundled = fileURLToPath(new URL(`../native/${process.platform}-${process.arch}/${exe}`, import.meta.url));

test('a clean host with no CLI or Git registers a working MCP server and reuses its vault', {
  timeout: 30000, skip: !existsSync(bundled) && !process.env.REQUIRE_NATIVE,
}, async () => {
  const opts = options();
  let server;
  const ctx = { effect: fn => fn(), mcpManager: { register(name, config) { server = config; return () => {}; } } };
  const config = { serverName: 'agent-context', binaryPath: '', vaultRoot: '' };
  const first = mountAgentContext(ctx, config, opts);
  assert.equal(first.source, 'bundled');
  assert.equal(server.command, bundled);
  assert.ok(existsSync(join(first.vaultRoot, 'AGENTS.md')));
  writeFileSync(join(first.vaultRoot, 'wiki', 'existing.md'), 'Keep me');
  assert.equal(mountAgentContext(ctx, config, opts).vaultRoot, first.vaultRoot);
  assert.ok(existsSync(join(first.vaultRoot, 'wiki', 'existing.md')));

  const child = spawn(server.command, server.args, { env: { ...opts.env, ...server.env }, stdio: ['pipe', 'pipe', 'inherit'] });
  const closed = once(child, 'close');
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  const send = msg => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...msg }) + '\n');
  try {
    send({ id: 1, method: 'initialize', params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'smoke', version: '1' } } });
    assert.ok(JSON.parse((await lines.next()).value).result);
    send({ method: 'notifications/initialized' });
    send({ id: 2, method: 'tools/list', params: {} });
    const names = JSON.parse((await lines.next()).value).result.tools.map(t => t.name);
    assert.ok(names.includes('wiki_search') && names.includes('board_put'));
  } finally {
    child.stdin.end();
    child.kill();
    await closed;
  }
});

test('concurrent first launches share one complete vault', { skip: !existsSync(bundled) && !process.env.REQUIRE_NATIVE }, async () => {
  const opts = options();
  const moduleUrl = new URL('../lib/vault.js', import.meta.url).href;
  const child = `import {selectVault} from ${JSON.stringify(moduleUrl)}; console.log(selectVault('',process.argv[1]).root)`;
  const resolve = () => promisify(execFile)(process.execPath, ['--input-type=module', '-e', child, bundled], { env: opts.env });
  const [a, b] = await Promise.all([resolve(), resolve()]);
  assert.equal(a.stdout.trim(), b.stdout.trim());
  assert.ok(existsSync(join(a.stdout.trim(), 'wiki', 'index.md')));
});
