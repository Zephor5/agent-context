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

import { ConfigError, resolveBinary } from './resolve.js';
import { selectVault } from './vault.js';

/**
 * Accepted MCP server names. Conservative on purpose: the name becomes a tool
 * namespace in the host, so punctuation and whitespace are kept out.
 */
export const SERVER_NAME_PATTERN = /^[A-Za-z0-9_-]{1,32}$/;

/** Argument vector for the server. `serve` speaks MCP over stdio. */
export const SERVE_ARGS = ['serve'];

const NOOP_LOGGER = { info() {} };

/** Resolve the executable and vault before registering one server for this plugin's lifetime. */
export function mountAgentContext(ctx, config = {}, options = {}) {
  const logger = ctx.logger ?? NOOP_LOGGER;

  const serverName = typeof config.serverName === 'string' ? config.serverName.trim() : '';
  if (!SERVER_NAME_PATTERN.test(serverName)) {
    throw new ConfigError(
      `serverName must match ${SERVER_NAME_PATTERN.source}, got ${JSON.stringify(config.serverName ?? '')}`,
    );
  }

  const binary = resolveBinary(config.binaryPath, options);
  if (!binary) {
    throw new ConfigError(`no executable for ${process.platform}-${process.arch}; install the packaged plugin or set binaryPath to your agent-context executable`);
  }
  const vault = selectVault(config.vaultRoot, binary.path, options);

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
