// Package the standalone Web assets into an isolated DSH frame from their canonical sources.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const web = '../../../src/web/';
const js = read(web + 'app.js'),
  css = read(web + 'app.css'),
  bridge = read('../client/frame.js');
const hash = (value) => "'sha256-" + createHash('sha256').update(value).digest('base64') + "'";
const csp = `default-src 'none'; script-src ${hash(bridge)} ${hash(js)}; style-src ${hash(css)}; img-src data:; form-action 'none'; base-uri 'none'`;
const html = read(web + 'index.html')
  .replace(
    /<link rel="stylesheet"[^>]+>/,
    () =>
      `<meta http-equiv="Content-Security-Policy" content="${csp}"><meta name="bridge" content="__BRIDGE_TOKEN__"><style>${css}</style>`,
  )
  .replace('<script src="/app.js" defer></script>', '')
  .replace('</body>', () => `<script>${bridge}</script><script>${js}</script></body>`);
const pkg = JSON.parse(read('../package.json'));
const client = read('../client/panel.js')
  .replace("'__PACKAGE_ID__'", () => JSON.stringify(pkg.name))
  .replace("'__FRAME_HTML__'", () => JSON.stringify(html));
writeFileSync(new URL('../lib/client.js', import.meta.url), client);
