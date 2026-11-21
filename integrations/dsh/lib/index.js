/**
 * agent-context for DeepSeek Harness.
 *
 * Registers the `agent-context` MCP server (stdio, `agent-context serve`) on
 * the host's `mcpManager`, so a DSH session reads and writes the same personal
 * wiki vault and blackboard as every other agent tool on the machine.
 *
 * Named exports only. A default export would hide the loader metadata (`name`,
 * `inject`, `Config`) from the cordis loader, which is why the host's own
 * plugins avoid one.
 *
 * `inject: ['mcpManager']` means `apply` runs while the MCP manager service is
 * present and is torn down with it; the registration itself is held by
 * `ctx.effect`, so disabling or reconfiguring the plugin unregisters the
 * server.
 *
 * @module dsh-agent-context
 */

import z from '@deepseek-ai/schemastery';

import { attachVault } from './web-host.js';
import { mountAgentContext } from './mount.js';

export const name = 'agent-context';

export const inject = ['mcpManager'];

export const Config = z.object({
  serverName: z
    .string()
    .default('agent-context')
    .description('Name the MCP server is registered under in the host.'),
  vaultRoot: z
    .string()
    .default('')
    .description(
      'Absolute path of the vault, passed to the server as AGENT_CONTEXT_ROOT. It must already exist and contain raw/ and wiki/ (create it with `agent-context init <dir>`). "~" is not expanded. Empty: reuse an existing vault or initialize the managed user vault.',
    ),
  binaryPath: z
    .string()
    .default('')
    .description(
      'Absolute path of the agent-context executable. Empty: absolute PATH entries are searched, then Cargo’s bin directory and the bundled executable. When set, the path is used as given and must be an executable file — there is no fallback.',
    ),
});

/**
 * @param {import('@deepseek-ai/cordis').Context} ctx
 * @param {{ serverName: string, vaultRoot: string, binaryPath: string }} config
 */
export function apply(ctx, config) {
  // schemastery has already applied every default, so `config` is complete.
  const registration = mountAgentContext(ctx, config);
  ctx.effect(() => attachVault(ctx, registration));
}
