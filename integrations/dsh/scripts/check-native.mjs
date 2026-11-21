import { accessSync, constants, readFileSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

// A changed pin or a partial restore must never produce a mixed-version package.
const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url)));
const native = JSON.parse(readFileSync(new URL('../native/manifest.json', import.meta.url)));
if (native.version !== pkg.agentContext.cliVersion) throw new Error('CLI pin changed; run npm run fetch:native');
for (const platform of ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64']) {
  const windows = platform.startsWith('win32');
  const file = fileURLToPath(new URL('../native/' + platform + '/agent-context' + (windows ? '.exe' : ''), import.meta.url));
  const stat = statSync(file, { throwIfNoEntry: false });
  if (!stat?.isFile() || stat.size === 0) throw new Error('Missing native executable: ' + file);
  accessSync(file, windows ? constants.R_OK : constants.X_OK);
  if (createHash('sha256').update(readFileSync(file)).digest('hex') !== native.sha256[platform]) {
    throw new Error('Native executable checksum mismatch: ' + platform);
  }
}
