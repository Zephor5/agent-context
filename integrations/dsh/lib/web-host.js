// One route owner per bundle, with one lazily started stdio worker per enabled vault.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const hosts = new WeakMap();
function vaults(ctx) {
  const root = ctx.root ?? ctx;
  if (!hosts.has(root)) hosts.set(root, new Map());
  return hosts.get(root);
}

function backend(root) {
  const executable = fileURLToPath(
    new URL(
      `../native/${process.platform}-${process.arch}/agent-context${process.platform === 'win32' ? '.exe' : ''}`,
      import.meta.url,
    ),
  );
  let active;
  function stop(session, error) {
    if (active === session) active = undefined;
    session.child.kill();
    for (const request of session.pending.splice(0)) request.reject(error);
  }
  function start() {
    const child = spawn(executable, ['--root', root, 'web', '--stdio'], {
      env: { ...process.env, AGENT_CONTEXT_ROOT: root },
      stdio: ['pipe', 'pipe', 'ignore'],
      windowsHide: true,
    });
    const session = { child, pending: [] };
    active = session;
    createInterface({ input: child.stdout }).on('line', (line) => {
      const request = session.pending[0];
      if (!request) return;
      try {
        const reply = JSON.parse(line);
        session.pending.shift();
        request.resolve(reply);
      } catch (error) {
        stop(session, error);
      }
    });
    child.stdin.on('error', (error) => stop(session, error));
    child.on('error', (error) => stop(session, error));
    child.once('close', () => stop(session, new Error('Agent Context web backend closed')));
    return session;
  }
  return {
    request(input) {
      const session = active ?? start();
      return new Promise((resolve, reject) => {
        session.pending.push({ resolve, reject });
        session.child.stdin.write(JSON.stringify(input) + '\n');
      });
    },
    close() {
      if (active) stop(active, new Error('Vault disabled'));
    },
  };
}

export function attachVault(ctx, registration) {
  const list = vaults(ctx);
  const server = backend(registration.vaultRoot);
  list.set(registration.serverName, server);
  return () => {
    if (list.get(registration.serverName) === server) list.delete(registration.serverName);
    server.close();
  };
}

export const inject = ['connection'];
export function apply(ctx) {
  const list = vaults(ctx);
  const headers = { 'cache-control': 'no-store' };
  const json = (body, status = 200) => Response.json(body, { status, headers });
  ctx.effect(() =>
    ctx.connection.fetch.register({
      path: '/api/agent-context/vaults',
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async () => json([...list.keys()]),
    }),
  );
  ctx.effect(() =>
    ctx.connection.fetch.register({
      path: '/api/agent-context/request',
      methods: ['POST'],
      requestBody: 'buffered',
      async fetch(req) {
        if (
          req.headers.get('content-type')?.split(';')[0] !== 'application/json' ||
          req.headers.get('x-agent-context') !== '1'
        )
          return json({ error: 'Expected an Agent Context request' }, 415);
        const input = await req.json().catch(() => null);
        if (
          !input ||
          typeof input.path !== 'string' ||
          !/^\/api\/(catalog|document|board|preview|git)(\?|$)/.test(input.path) ||
          !['GET', 'POST', 'PUT', 'DELETE'].includes(input.method)
        )
          return json({ error: 'Invalid Web API request' }, 400);
        const server = list.get(input.vault);
        if (!server) return json({ error: 'Knowledge base unavailable' }, 404);
        try {
          const reply = await server.request({ path: input.path, method: input.method, body: input.body });
          return json(reply.body, reply.status);
        } catch (error) {
          return json({ error: error.message }, 503);
        }
      },
    }),
  );
}
