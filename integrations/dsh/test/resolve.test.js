/**
 * Resolution rules: an explicit binaryPath is used or rejected but never
 * replaced by a search result, the search looks only at absolute PATH entries
 * and then ~/.cargo/bin, and a vault root must really be a vault.
 *
 * Run with `node --test test/` — this file imports nothing outside node:*.
 */

import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join, relative } from 'node:path';
import { after, describe, it } from 'node:test';

import {
  BINARY_NAME,
  CARGO_BIN_SEGMENTS,
  ConfigError,
  isExecutableFile,
  resolveBinary,
  resolveVault,
  VAULT_SUBDIRS,
} from '../lib/resolve.js';

const workspace = mkdtempSync(join(tmpdir(), 'dsh-agent-context-resolve-'));
after(() => rmSync(workspace, { recursive: true, force: true }));

let counter = 0;
/** A fresh empty directory inside the workspace. */
function tempDir(label) {
  const dir = join(workspace, `${label}-${counter++}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** A directory holding an executable named like the real binary. */
function binDir(label) {
  const dir = tempDir(label);
  const file = join(dir, BINARY_NAME);
  writeFileSync(file, '#!/bin/sh\nexit 0\n');
  chmodSync(file, 0o755);
  return { dir, file };
}

/** A directory shaped like a vault: `raw/` and `wiki/`, as `Vault::open` requires. */
function vaultDir(label) {
  const dir = tempDir(label);
  for (const sub of VAULT_SUBDIRS) mkdirSync(join(dir, sub));
  return dir;
}

// Running as root makes access(X_OK) permissive, so the "not executable"
// expectations below cannot hold; Windows has no execute bit at all.
const noExecuteBits =
  process.platform === 'win32'
    ? 'POSIX execute bits are not meaningful on Windows'
    : typeof process.getuid === 'function' && process.getuid() === 0
      ? 'running as root: access(X_OK) is permissive'
      : false;

describe('resolveVault', () => {
  it('treats an absent or empty value as unconfigured', () => {
    assert.equal(resolveVault(undefined), null);
    assert.equal(resolveVault(''), null);
    assert.equal(resolveVault('   '), null);
  });

  it('accepts an existing vault and returns it unchanged', () => {
    const dir = vaultDir('vault');
    assert.deepEqual(resolveVault(dir), { root: dir });
    assert.deepEqual(resolveVault(`  ${dir}  `), { root: dir });
  });

  it('rejects a relative path', () => {
    assert.throws(() => resolveVault('relative/vault'), ConfigError);
    assert.throws(() => resolveVault('~/my-context'), ConfigError);
  });

  it('rejects a path that does not exist', () => {
    assert.throws(() => resolveVault(join(workspace, 'missing-vault')), ConfigError);
  });

  it('rejects a file', () => {
    const file = join(tempDir('not-a-dir'), 'vault');
    writeFileSync(file, '');
    assert.throws(() => resolveVault(file), ConfigError);
  });

  it('rejects a directory that is not a vault, naming the missing subdirectory', () => {
    const empty = tempDir('empty');
    assert.throws(() => resolveVault(empty), /missing raw\//);

    const rawOnly = tempDir('raw-only');
    mkdirSync(join(rawOnly, 'raw'));
    assert.throws(() => resolveVault(rawOnly), /missing wiki\//);

    const wikiOnly = tempDir('wiki-only');
    mkdirSync(join(wikiOnly, 'wiki'));
    assert.throws(() => resolveVault(wikiOnly), /missing raw\//);
  });

  it('rejects a vault whose subdirectory is a file', () => {
    const dir = tempDir('raw-is-file');
    writeFileSync(join(dir, 'raw'), '');
    mkdirSync(join(dir, 'wiki'));
    assert.throws(() => resolveVault(dir), /missing raw\//);
  });

  it('creates nothing while validating', () => {
    const dir = join(workspace, 'never-created');
    assert.throws(() => resolveVault(dir), ConfigError);
    assert.equal(isExecutableFile(dir), false);
    assert.throws(() => resolveVault(dir), ConfigError);
  });
});

describe('resolveBinary with an explicit binaryPath', () => {
  it('uses an absolute executable', () => {
    const { dir, file } = binDir('explicit');
    const resolved = resolveBinary(file, { env: { PATH: dir }, homedir: tempDir('home') });
    assert.deepEqual(resolved, { path: file, source: 'config' });
  });

  it('rejects a relative path instead of searching', () => {
    const { dir, file } = binDir('no-fallback-relative');
    const env = { PATH: dir };
    const rel = relative(process.cwd(), file);
    assert.throws(() => resolveBinary(rel, { env, homedir: tempDir('home') }), ConfigError);
    // The search would have succeeded — proof the rejection is not a fallback.
    assert.equal(resolveBinary('', { env, homedir: tempDir('home') }).path, file);
  });

  it('rejects a missing path instead of searching', () => {
    const { dir, file } = binDir('no-fallback-missing');
    const env = { PATH: dir };
    assert.throws(
      () => resolveBinary(join(workspace, 'nowhere', 'agent-context'), { env, homedir: tempDir('home') }),
      ConfigError,
    );
    assert.equal(resolveBinary('', { env, homedir: tempDir('home') }).path, file);
  });

  it('rejects a directory', () => {
    const { dir } = binDir('dir-as-binary');
    assert.throws(() => resolveBinary(dir, { env: { PATH: '' }, homedir: tempDir('home') }), ConfigError);
  });

  it('rejects a file without execute permission', { skip: noExecuteBits }, () => {
    const dir = tempDir('not-executable');
    const file = join(dir, BINARY_NAME);
    writeFileSync(file, 'text\n');
    chmodSync(file, 0o644);
    assert.equal(isExecutableFile(file), false);
    assert.throws(() => resolveBinary(file, { env: { PATH: '' }, homedir: tempDir('home') }), ConfigError);
  });
});

describe('resolveBinary search order', () => {
  const home = () => tempDir('home');

  it('takes the first absolute PATH entry that has the binary', () => {
    const plain = tempDir('path-empty');
    const first = binDir('path-first');
    const second = binDir('path-second');
    const env = { PATH: [plain, first.dir, second.dir].join(delimiter) };
    assert.deepEqual(resolveBinary('', { env, homedir: home() }), {
      path: first.file,
      source: 'path',
    });
  });

  it('skips relative and empty PATH entries', () => {
    const { dir, file } = binDir('path-relative');
    const relEntry = relative(process.cwd(), dir);
    const absolute = binDir('path-absolute');

    // The relative entry points at a real binary, but only via the process
    // working directory, so it must be ignored in favour of the absolute one.
    const env = { PATH: ['', relEntry, '.', absolute.dir].join(delimiter) };
    const resolved = resolveBinary('', { env, homedir: home() });
    assert.equal(resolved.path, absolute.file);
    assert.notEqual(resolved.path, file);

    // With only unusable entries there is no result at all.
    assert.equal(resolveBinary('', { env: { PATH: ['', relEntry, '.'].join(delimiter) }, homedir: home() }), null);
  });

  it('falls back to ~/.cargo/bin when PATH has nothing', () => {
    const fakeHome = tempDir('home-cargo');
    const cargoBin = join(fakeHome, ...CARGO_BIN_SEGMENTS);
    mkdirSync(cargoBin, { recursive: true });
    const file = join(cargoBin, BINARY_NAME);
    writeFileSync(file, '#!/bin/sh\nexit 0\n');
    chmodSync(file, 0o755);

    assert.deepEqual(resolveBinary('', { env: { PATH: '' }, homedir: fakeHome }), {
      path: file,
      source: 'cargo',
    });
    assert.deepEqual(resolveBinary(undefined, { env: {}, homedir: fakeHome }), {
      path: file,
      source: 'cargo',
    });
  });

  it('prefers PATH over ~/.cargo/bin', () => {
    const fakeHome = tempDir('home-cargo-second');
    const cargoBin = join(fakeHome, ...CARGO_BIN_SEGMENTS);
    mkdirSync(cargoBin, { recursive: true });
    const cargoFile = join(cargoBin, BINARY_NAME);
    writeFileSync(cargoFile, '#!/bin/sh\nexit 0\n');
    chmodSync(cargoFile, 0o755);
    const onPath = binDir('path-wins');

    assert.deepEqual(resolveBinary('', { env: { PATH: onPath.dir }, homedir: fakeHome }), {
      path: onPath.file,
      source: 'path',
    });
  });

  it('returns null when nothing is installed', () => {
    const fakeHome = tempDir('home-bare');
    assert.equal(resolveBinary('', { env: { PATH: tempDir('nothing') }, homedir: fakeHome }), null);
    assert.equal(resolveBinary('', { env: {}, homedir: fakeHome }), null);
    assert.equal(resolveBinary('', { env: {}, homedir: '' }), null);
    assert.equal(resolveBinary('', { env: {}, homedir: 'relative-home' }), null);
  });
});
