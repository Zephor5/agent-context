// Materialize exactly the CLI requested by the plugin; packaging never builds Rust.
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, chmodSync, copyFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const { agentContext: { cliVersion: version } } = JSON.parse(readFileSync(join(root, 'package.json')));
if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(version)) throw new Error('Expected an exact CLI version');
const platforms = ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64'];

function executable(platform) {
  return platform.startsWith('win32') ? 'agent-context.exe' : 'agent-context';
}

async function download(url, headers = {}) {
  const res = await fetch(url, { headers, signal: AbortSignal.timeout(120000) });
  if (!res.ok) throw new Error(`Download failed: ${url} (HTTP ${res.status})`);
  return Buffer.from(await res.arrayBuffer());
}

const staging = mkdtempSync(join(tmpdir(), 'agent-context-native-'));
try {
  // CLI 0.1.0 shipped before versioned CLI Releases existed. Reuse the original tested bytes.
  if (version === '0.1.0') {
    const bytes = await download('https://registry.npmjs.org/@zephor/dsh-agent-context/-/dsh-agent-context-0.2.1.tgz');
    const sha512 = 'Ggh4wPlVd9mj44UMD3fxZ8J+ElHYtEUC+FpSB/eDTSqt8+jTvz7ZCeAQPxoyI3QL4qmBHwJ6ms8T1HvqSZJ3Ww==';
    if (createHash('sha512').update(bytes).digest('base64') !== sha512) throw new Error('Original CLI archive integrity mismatch');
    const file = join(staging, 'bootstrap.tgz');
    writeFileSync(file, bytes);
    const members = platforms.map(platform => 'package/native/' + platform + '/' + executable(platform));
    execFileSync('tar', ['-xzf', file, '-C', staging, '--strip-components=1', ...members]);
  } else {
    const headers = process.env.GH_TOKEN ? { authorization: 'Bearer ' + process.env.GH_TOKEN } : {};
    const release = JSON.parse(await download(`https://api.github.com/repos/Zephor5/agent-context/releases/tags/cli-v${version}`, headers));
    if (release.draft || release.tag_name !== 'cli-v' + version) throw new Error('Unexpected CLI release');
    for (const platform of platforms) {
      const exe = executable(platform);
      const name = 'agent-context-' + platform + (platform.startsWith('win32') ? '.exe' : '');
      const asset = release.assets.find(a => a.name === name);
      if (!asset) throw new Error(`CLI ${version} lacks ${name}`);
      const bytes = await download(asset.browser_download_url);
      if ('sha256:' + createHash('sha256').update(bytes).digest('hex') !== asset.digest) throw new Error(`Invalid digest for ${name}`);
      const directory = join(staging, 'native', platform);
      mkdirSync(directory, { recursive: true });
      writeFileSync(join(directory, exe), bytes);
    }
  }
  const sha256 = {};
  for (const platform of platforms) {
    const exe = executable(platform);
    const directory = join(root, 'native', platform);
    mkdirSync(directory, { recursive: true });
    const file = join(directory, exe);
    const source = join(staging, 'native', platform, exe);
    sha256[platform] = createHash('sha256').update(readFileSync(source)).digest('hex');
    copyFileSync(source, file);
    chmodSync(file, 0o755);
  }
  writeFileSync(join(root, 'native', 'manifest.json'), JSON.stringify({ version, sha256 }, null, 2) + '\n');
  console.log(`Restored CLI ${version} for ${platforms.join(', ')}`);
} finally {
  rmSync(staging, { recursive: true, force: true });
}
