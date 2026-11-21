/**
 * Configuration resolution for the agent-context DSH Host plugin.
 *
 * This module deliberately imports nothing from DSH or cordis, so the rules it
 * encodes can be exercised with plain `node --test` in a checkout that has no
 * dependencies installed.
 *
 * Two rules matter here, and both fail closed:
 *
 * - An explicitly configured `binaryPath` is validated and never falls back to
 *   a search. A user who names an executable gets that executable or an error,
 *   not a different binary found somewhere on PATH.
 * - A vault root must be an absolute, existing directory that really is a
 *   vault (`raw/` and `wiki/`, the same two directories `Vault::open` requires
 *   in `src/vault.rs`). Explicit roots are validated without modification.
 *
 * Resolution never spawns a shell: `command -v` would pull in a login shell's
 * PATH and arbitrary profile scripts. Candidates are probed with `fs` only.
 *
 * @module dsh-agent-context/resolve
 */

import { accessSync, constants, statSync } from 'node:fs';
import { homedir as osHomedir } from 'node:os';
import { delimiter, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Executable installed by `cargo install --locked --path .`. */
export const BINARY_NAME = 'agent-context';

/** Directories that make a directory a vault, per `Vault::open` in `src/vault.rs`. */
export const VAULT_SUBDIRS = ['raw', 'wiki'];

/** Last-resort install location searched after PATH: `~/.cargo/bin`. */
export const CARGO_BIN_SEGMENTS = ['.cargo', 'bin'];

/**
 * A configuration value the user has to fix. Thrown rather than logged: a
 * misconfigured plugin should be visibly broken, not quietly inactive.
 */
export class ConfigError extends Error {
  constructor(message) {
    super(message.startsWith('agent-context:') ? message : `agent-context: ${message}`);
    this.name = 'ConfigError';
  }
}

/** Trim a possibly absent string config field down to a plain string. */
function text(value) {
  return typeof value === 'string' ? value.trim() : '';
}

/**
 * File names to probe for one directory. Windows keeps the bare name as well,
 * so a shim without an extension is still found.
 *
 * @param {NodeJS.Platform} platform
 */
function executableNames(platform) {
  return platform === 'win32' ? [`${BINARY_NAME}.exe`, BINARY_NAME] : [BINARY_NAME];
}

/**
 * True when `candidate` is a regular file this process may execute.
 *
 * `access(X_OK)` is used instead of inspecting `mode` bits so the answer
 * accounts for the effective user. (As root, X_OK is permissive: it reports
 * success when any execute bit is set.) On Windows the execute bit carries no
 * meaning, so being a regular file is all that is checked.
 *
 * @param {string} candidate absolute path
 * @param {NodeJS.Platform} platform
 */
export function isExecutableFile(candidate, platform = process.platform) {
  let stats;
  try {
    stats = statSync(candidate);
  } catch {
    return false;
  }
  if (!stats.isFile()) return false;
  if (platform === 'win32') return true;
  try {
    accessSync(candidate, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Locate the `agent-context` executable.
 *
 * @param {string | undefined} binaryPath the configured path; empty means "search"
 * @param {object} [options]
 * @param {NodeJS.ProcessEnv} [options.env] environment to read PATH from
 * @param {string} [options.homedir] home directory used for `~/.cargo/bin`
 * @param {NodeJS.Platform} [options.platform]
 * @returns {{ path: string, source: 'config' | 'path' | 'cargo' | 'bundled' } | null}
 *   `null` when nothing was configured and no candidate was found; the caller
 *   then stays inactive instead of guessing.
 * @throws {ConfigError} when `binaryPath` is set but is not an absolute path to
 *   an executable file. No search is attempted in that case.
 */
export function resolveBinary(binaryPath, options = {}) {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const home = options.homedir ?? osHomedir();
  const names = executableNames(platform);

  const configured = text(binaryPath);
  if (configured !== '') {
    if (!isAbsolute(configured)) {
      throw new ConfigError(
        `binaryPath must be an absolute path to the agent-context executable, got ${JSON.stringify(configured)}`,
      );
    }
    if (!isExecutableFile(configured, platform)) {
      throw new ConfigError(
        `binaryPath is not an executable file: ${configured} — fix the path or clear it to search PATH and ~/.cargo/bin`,
      );
    }
    return { path: configured, source: 'config' };
  }

  // PATH, absolute entries only. A relative entry (including the empty string,
  // which means "current directory") resolves against whatever working
  // directory the host happens to have, so it is skipped rather than searched.
  const rawPath = typeof env.PATH === 'string' ? env.PATH : (env.Path ?? '');
  for (const entry of String(rawPath).split(delimiter)) {
    const dir = entry.trim();
    if (dir === '' || !isAbsolute(dir)) continue;
    for (const name of names) {
      const candidate = join(dir, name);
      if (isExecutableFile(candidate, platform)) {
        return { path: candidate, source: 'path' };
      }
    }
  }

  // The documented install location, for a GUI host that did not inherit a
  // shell PATH (see docs/clients.md).
  const cargoDirs = [];
  if (env.CARGO_HOME && isAbsolute(env.CARGO_HOME)) cargoDirs.push(join(env.CARGO_HOME, 'bin'));
  if (home && isAbsolute(home)) cargoDirs.push(join(home, ...CARGO_BIN_SEGMENTS));
  for (const dir of new Set(cargoDirs)) {
    for (const name of names) {
      const candidate = join(dir, name);
      if (isExecutableFile(candidate, platform)) return { path: candidate, source: 'cargo' };
    }
  }
  const nativeRoot = options.nativeRoot ?? fileURLToPath(new URL('../native/', import.meta.url));
  const bundled = join(nativeRoot, platform + '-' + (options.arch ?? process.arch), names[0]);
  if (isExecutableFile(bundled, platform)) return { path: bundled, source: 'bundled' };
  return null;
}

/**
 * Validate the configured vault root.
 *
 * @param {string | undefined} vaultRoot
 * @returns {{ root: string } | null} `null` when nothing is configured
 * @throws {ConfigError} when the value is set but is not an absolute path to an
 *   existing vault directory
 */
export function resolveVault(vaultRoot) {
  const configured = text(vaultRoot);
  if (configured === '') return null;

  if (!isAbsolute(configured)) {
    throw new ConfigError(
      `vaultRoot must be an absolute path (agent-context does not expand "~"), got ${JSON.stringify(configured)}`,
    );
  }

  let stats;
  try {
    stats = statSync(configured);
  } catch {
    throw new ConfigError(
      `vaultRoot does not exist: ${configured} — create a vault with \`agent-context init <dir>\` first`,
    );
  }
  if (!stats.isDirectory()) {
    throw new ConfigError(`vaultRoot is not a directory: ${configured}`);
  }

  for (const sub of VAULT_SUBDIRS) {
    const dir = join(configured, sub);
    let subStats;
    try {
      subStats = statSync(dir);
    } catch {
      subStats = undefined;
    }
    if (subStats === undefined || !subStats.isDirectory()) {
      throw new ConfigError(
        `vaultRoot is not an agent-context vault (missing ${sub}/): ${configured} — create one with \`agent-context init\``,
      );
    }
  }

  return { root: configured };
}
