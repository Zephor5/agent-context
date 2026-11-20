/**
 * Registration behaviour against a fake cordis context: the exact register
 * payload, disposal through ctx.effect, staying inert when unconfigured, and
 * never writing anything.
 *
 * Run with `node --test test/` — this file imports nothing outside node:*.
 */

import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';

import { ConfigError } from '../lib/resolve.js';
import { mountAgentContext, SERVE_ARGS } from '../lib/mount.js';

const workspace = mkdtempSync(join(tmpdir(), 'dsh-agent-context-mount-'));
after(() => rmSync(workspace, { recursive: true, force: true }));

let counter = 0;
function tempDir(label) {
  const dir = join(workspace, `${label}-${counter++}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function vaultDir(label) {
  const dir = tempDir(label);
  mkdirSync(join(dir, 'raw'));
  mkdirSync(join(dir, 'wiki'));
  return dir;
}

function binary(label) {
  const dir = tempDir(label);
  const file = join(dir, 'agent-context');
  writeFileSync(file, '#!/bin/sh\nexit 0\n');
  chmodSync(file, 0o755);
  return file;
}

/**
 * Minimal stand-in for the host: records registrations, holds the disposers
 * ctx.effect() collects, and captures log lines.
 */
function createCtx() {
  const state = { registrations: [], effects: [], unregistered: [], info: [], warn: [] };
  const ctx = {
    logger: {
      info: message => state.info.push(String(message)),
      warn: message => state.warn.push(String(message)),
    },
    effect(callback) {
      const disposer = callback();
      state.effects.push(disposer);
      return disposer;
    },
    mcpManager: {
      register(serverName, options) {
        state.registrations.push({ serverName, options });
        return () => state.unregistered.push(serverName);
      },
    },
  };
  return { ctx, state };
}

/** Isolation: no PATH, and a home directory without ~/.cargo/bin. */
function searchNothing() {
  return { env: { PATH: '' }, homedir: tempDir('home') };
}

describe('mountAgentContext', () => {
  it('registers one stdio server with exactly the documented payload', () => {
    const vault = vaultDir('vault');
    const command = binary('bin');
    const { ctx, state } = createCtx();

    const outcome = mountAgentContext(
      ctx,
      { serverName: 'agent-context', vaultRoot: vault, binaryPath: command },
      searchNothing(),
    );

    assert.equal(state.registrations.length, 1);
    const [{ serverName, options }] = state.registrations;
    assert.equal(serverName, 'agent-context');
    assert.deepEqual(options, {
      transport: 'stdio',
      command,
      args: ['serve'],
      env: { AGENT_CONTEXT_ROOT: vault },
    });
    // Only the vault variable is injected; nothing else leaks into the child.
    assert.deepEqual(Object.keys(options.env), ['AGENT_CONTEXT_ROOT']);
    assert.deepEqual(outcome, {
      registered: true,
      serverName: 'agent-context',
      command,
      vaultRoot: vault,
      source: 'config',
    });
  });

  it('hands a copy of the argument vector to the host', () => {
    const { ctx, state } = createCtx();
    mountAgentContext(
      ctx,
      { serverName: 'agent-context', vaultRoot: vaultDir('vault-args'), binaryPath: binary('bin-args') },
      searchNothing(),
    );
    const { args } = state.registrations[0].options;
    args.push('mutated');
    assert.deepEqual(SERVE_ARGS, ['serve']);
  });

  it('unregisters the server when the plugin fiber is disposed', () => {
    const { ctx, state } = createCtx();
    mountAgentContext(
      ctx,
      { serverName: 'wiki', vaultRoot: vaultDir('vault-dispose'), binaryPath: binary('bin-dispose') },
      searchNothing(),
    );

    // ctx.effect() received the disposer that register() returned.
    assert.equal(state.effects.length, 1);
    assert.equal(typeof state.effects[0], 'function');
    assert.deepEqual(state.unregistered, []);

    state.effects[0]();
    assert.deepEqual(state.unregistered, ['wiki']);
  });

  it('accepts a custom server name', () => {
    const { ctx, state } = createCtx();
    const outcome = mountAgentContext(
      ctx,
      { serverName: ' my-wiki ', vaultRoot: vaultDir('vault-name'), binaryPath: binary('bin-name') },
      searchNothing(),
    );
    assert.equal(outcome.serverName, 'my-wiki');
    assert.equal(state.registrations[0].serverName, 'my-wiki');
  });

  it('resolves the binary from PATH when none is configured', () => {
    const command = binary('bin-path');
    const { ctx, state } = createCtx();
    const outcome = mountAgentContext(
      ctx,
      { serverName: 'agent-context', vaultRoot: vaultDir('vault-path'), binaryPath: '' },
      { env: { PATH: join(command, '..') }, homedir: tempDir('home') },
    );
    assert.equal(outcome.source, 'path');
    assert.equal(state.registrations[0].options.command, command);
  });
});

describe('mountAgentContext when it should stay inert', () => {
  it('does nothing without a vaultRoot', () => {
    const { ctx, state } = createCtx();
    const outcome = mountAgentContext(
      ctx,
      { serverName: 'agent-context', vaultRoot: '', binaryPath: binary('bin-inert') },
      searchNothing(),
    );

    assert.deepEqual(outcome, { registered: false, reason: 'vault-not-configured' });
    assert.deepEqual(state.registrations, []);
    assert.deepEqual(state.effects, []);
    assert.equal(state.warn.length, 0);
    assert.match(state.info.join('\n'), /vaultRoot/);
  });

  it('does nothing when the executable cannot be found', () => {
    const { ctx, state } = createCtx();
    const outcome = mountAgentContext(
      ctx,
      { serverName: 'agent-context', vaultRoot: vaultDir('vault-no-bin'), binaryPath: '' },
      searchNothing(),
    );

    assert.deepEqual(outcome, { registered: false, reason: 'binary-not-found' });
    assert.deepEqual(state.registrations, []);
    assert.deepEqual(state.effects, []);
    assert.match(state.warn.join('\n'), /not found/);
  });

  it('writes nothing into the vault and creates no directories', () => {
    const vault = vaultDir('vault-readonly');
    const before = readdirSync(vault).sort();
    const rawBefore = readdirSync(join(vault, 'raw')).sort();
    const { ctx } = createCtx();

    mountAgentContext(
      ctx,
      { serverName: 'agent-context', vaultRoot: vault, binaryPath: binary('bin-readonly') },
      searchNothing(),
    );

    assert.deepEqual(readdirSync(vault).sort(), before);
    assert.deepEqual(readdirSync(join(vault, 'raw')).sort(), rawBefore);
  });
});

describe('mountAgentContext configuration errors', () => {
  it('rejects an unusable server name without registering', () => {
    for (const serverName of ['', '   ', 'has space', 'bad/name', undefined, 'x'.repeat(65)]) {
      const { ctx, state } = createCtx();
      assert.throws(
        () =>
          mountAgentContext(
            ctx,
            { serverName, vaultRoot: vaultDir('vault-bad-name'), binaryPath: binary('bin-bad-name') },
            searchNothing(),
          ),
        ConfigError,
      );
      assert.deepEqual(state.registrations, []);
    }
  });

  it('rejects a bad vaultRoot without registering', () => {
    for (const vaultRoot of ['relative/vault', '~/vault', join(workspace, 'missing'), tempDir('not-a-vault')]) {
      const { ctx, state } = createCtx();
      assert.throws(
        () =>
          mountAgentContext(
            ctx,
            { serverName: 'agent-context', vaultRoot, binaryPath: binary('bin-bad-vault') },
            searchNothing(),
          ),
        ConfigError,
      );
      assert.deepEqual(state.registrations, []);
    }
  });

  it('rejects a bad binaryPath without falling back or registering', () => {
    const onPath = binary('bin-available');
    const options = { env: { PATH: join(onPath, '..') }, homedir: tempDir('home') };
    const { ctx, state } = createCtx();

    assert.throws(
      () =>
        mountAgentContext(
          ctx,
          {
            serverName: 'agent-context',
            vaultRoot: vaultDir('vault-bad-bin'),
            binaryPath: join(workspace, 'nowhere', 'agent-context'),
          },
          options,
        ),
      ConfigError,
    );
    assert.deepEqual(state.registrations, []);
    assert.deepEqual(state.effects, []);
  });

  it('works without a logger', () => {
    const { ctx } = createCtx();
    delete ctx.logger;
    const outcome = mountAgentContext(ctx, { serverName: 'agent-context', vaultRoot: '' }, searchNothing());
    assert.deepEqual(outcome, { registered: false, reason: 'vault-not-configured' });
  });
});
