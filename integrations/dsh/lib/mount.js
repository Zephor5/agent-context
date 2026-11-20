/**
 * Mounts one stdio MCP server — `agent-context serve` — on the host's
 * `mcpManager`.
 *
 * Kept apart from `./index.js` so the wiring can be driven by `node --test`
 * with a fake context: this module imports no DSH package, while `./index.js`
 * owns the schemastery `Config` and the loader metadata.
 *
 * Lifecycle: `mcpManager.register()` returns a disposer, and it is handed
 * straight to `ctx.effect()`. The server is therefore unregistered when the
 * plugin is disabled, reloaded, or reconfigured — the same pattern the host's
 * own plugins use for service registrations.
 *
 * @module dsh-agent-context/mount
 */

import { ConfigError, resolveBinary, resolveVault } from './resolve.js';

/**
 * Accepted MCP server names. Conservative on purpose: the name becomes a tool
 * namespace in the host, so punctuation and whitespace are kept out.
 */
export const SERVER_NAME_PATTERN = /^[A-Za-z0-9_-]{1,32}$/;

/** Argument vector for the server. `serve` speaks MCP over stdio. */
export const SERVE_ARGS = ['serve'];

const NOOP_LOGGER = { info() {}, warn() {} };

/**
 * Register the MCP server, if and only if it is fully configured.
 *
 * Nothing here creates, initialises, builds, downloads, or writes anything:
 * when the plugin is not configured it stays inactive and says so in the log.
 *
 * @param {object} ctx cordis context; needs `effect` and `mcpManager`
 * @param {{ serverName?: string, vaultRoot?: string, binaryPath?: string }} config
 * @param {object} [options] resolution overrides, for tests
 * @returns {{ registered: boolean, reason?: 'vault-not-configured' | 'binary-not-found',
 *            serverName?: string, command?: string, vaultRoot?: string,
 *            source?: 'config' | 'path' | 'cargo' }}
 * @throws {ConfigError} on a value the user must fix
 */
export function mountAgentContext(ctx, config = {}, options = {}) {
  const logger = ctx.logger ?? NOOP_LOGGER;

  const serverName = typeof config.serverName === 'string' ? config.serverName.trim() : '';
  if (!SERVER_NAME_PATTERN.test(serverName)) {
    throw new ConfigError(
      `serverName must match ${SERVER_NAME_PATTERN.source}, got ${JSON.stringify(config.serverName ?? '')}`,
    );
  }

  // Unconfigured is a normal state, not an error: the plugin ships enabled with
  // an empty vaultRoot, and a vault path is personal — it cannot be guessed.
  const vault = resolveVault(config.vaultRoot);
  if (vault === null) {
    logger.info(
      'agent-context: no vaultRoot configured, MCP server not registered — set it to an existing vault (created by `agent-context init <dir>`)',
    );
    return { registered: false, reason: 'vault-not-configured' };
  }

  // Throws when binaryPath is set but wrong; returns null when the search found
  // nothing. Not finding an installed binary is not something to fix for the
  // user: the plugin never builds or downloads it.
  const binary = resolveBinary(config.binaryPath, options);
  if (binary === null) {
    logger.warn(
      'agent-context: the agent-context executable was not found on PATH or in ~/.cargo/bin, MCP server not registered — install it (`cargo install --locked --path .`) or set binaryPath to its absolute path',
    );
    return { registered: false, reason: 'binary-not-found' };
  }

  // Exactly the registration documented in docs/clients.md: stdio transport,
  // `serve`, and the vault passed through AGENT_CONTEXT_ROOT. The env is this
  // single variable; the host supplies the rest of the child environment, and
  // nothing else from this plugin's own environment is forwarded.
  ctx.effect(() =>
    ctx.mcpManager.register(serverName, {
      transport: 'stdio',
      command: binary.path,
      args: [...SERVE_ARGS],
      env: { AGENT_CONTEXT_ROOT: vault.root },
    }),
  );

  logger.info(
    `agent-context: submitted MCP server "${serverName}" for connection (binary resolved from ${binary.source})`,
  );

  return {
    registered: true,
    serverName,
    command: binary.path,
    vaultRoot: vault.root,
    source: binary.source,
  };
}

export { ConfigError };
