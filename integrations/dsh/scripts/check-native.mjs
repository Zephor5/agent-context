import { accessSync, constants, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// npm pack for distribution must include the executables built by package-dsh.yml.
for (const platform of ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64']) {
  const windows = platform.startsWith('win32');
  const file = fileURLToPath(new URL('../native/' + platform + '/agent-context' + (windows ? '.exe' : ''), import.meta.url));
  const stat = statSync(file, { throwIfNoEntry: false });
  if (!stat?.isFile() || stat.size === 0) throw new Error('Missing native executable: ' + file);
  accessSync(file, windows ? constants.R_OK : constants.X_OK);
}
