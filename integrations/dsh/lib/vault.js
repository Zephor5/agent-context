import { lstatSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { ConfigError, resolveVault } from './resolve.js';

function exists(path) {
  return lstatSync(path, { throwIfNoEntry: false }) !== undefined;
}

export function selectVault(configured, command, options = {}) {
  const env = options.env ?? process.env;
  const home = options.homedir ?? homedir();
  const explicit = resolveVault(configured) ?? resolveVault(env.AGENT_CONTEXT_ROOT);
  if (explicit) return explicit;
  if (!home || !isAbsolute(home)) throw new ConfigError('cannot determine home directory; set vaultRoot');

  const settings = join(env.XDG_CONFIG_HOME || join(home, '.config'), 'agent-context', 'web.json');
  if (isAbsolute(settings) && exists(settings)) {
    let saved;
    try {
      saved = JSON.parse(readFileSync(settings, 'utf8'));
    } catch (error) {
      throw new ConfigError(`cannot read ${settings}: ${error.message}; set vaultRoot`);
    }
    const vault = resolveVault(saved?.root);
    if (!vault) throw new ConfigError(`missing vault root in ${settings}; set vaultRoot`);
    return vault;
  }

  // Only adopt a conventional location if it is already a vault. It may be a source checkout.
  const conventional = join(home, 'agent-context');
  if (exists(join(conventional, 'wiki')) && exists(join(conventional, 'raw'))) return resolveVault(conventional);

  const data = env.XDG_DATA_HOME || (process.platform === 'win32' && env.LOCALAPPDATA) || join(home, '.local', 'share');
  if (!isAbsolute(data)) throw new ConfigError('user data directory must be absolute; set vaultRoot');
  const parent = join(data, 'agent-context');
  const dest = join(parent, 'vault');
  if (exists(dest)) return resolveVault(dest);

  // Publish a complete vault in one rename. A failed init never leaves a half-built default.
  mkdirSync(parent, { recursive: true, mode: 0o700 });
  const staging = mkdtempSync(join(parent, '.init-'));
  try {
    const initEnv = { ...env, AGENT_CONTEXT_ROOT: staging };
    for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR']) delete initEnv[key];
    execFileSync(command, ['init', staging], { env: initEnv, stdio: 'pipe', timeout: 15000, windowsHide: true });
    resolveVault(staging);
    try {
      renameSync(staging, dest);
    } catch (error) {
      if (!exists(dest)) throw error;
      // Another DSH profile can win the first-start race.
    }
    return resolveVault(dest);
  } finally {
    rmSync(staging, { force: true, recursive: true });
  }
}
