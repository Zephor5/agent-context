"""Wait for the published version on the same public registry endpoint used by the DSH index."""
import base64
import hashlib
import hmac
import json
import socket
import sys
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen

from versions import precedence


def integrity(stream):
    digest = hashlib.file_digest(stream, 'sha512').digest()
    return 'sha512-' + base64.b64encode(digest).decode('ascii')


def public_copy_ready(name, version, expected, stable):
    url = 'https://registry.npmjs.org/' + quote(name, safe='')
    with urlopen(Request(url, headers={'Cache-Control': 'no-cache'}), timeout=25) as response:
        metadata = json.load(response)
    record = metadata.get('versions', {}).get(version)
    if not record:
        return False
    if record.get('name') != name or record.get('version') != version:
        raise ValueError('Unexpected npm package identity')
    latest = metadata.get('dist-tags', {}).get('latest')
    if stable and (not latest or precedence(latest) < precedence(version)):
        return False
    dist = record['dist']
    if not hmac.compare_digest(expected, dist['integrity']):
        raise ValueError('The npm integrity differs from the tested package')
    target = urlsplit(dist['tarball'])
    if target.scheme != 'https' or target.netloc != 'registry.npmjs.org':
        raise ValueError('Unexpected npm tarball location')
    with urlopen(dist['tarball'], timeout=60) as response:
        if not hmac.compare_digest(integrity(response), expected):
            raise ValueError('The downloaded archive differs from the tested package')
    return True


def main():
    manifest = json.loads(Path('integrations/dsh/package.json').read_text())
    name, version = manifest['name'], manifest['version']
    if name != '@zephor/dsh-agent-context':
        raise ValueError(f'Unexpected package: {name}')
    stable = '-' not in version.split('+')[0]
    with Path(f'release/zephor-dsh-agent-context-{version}.tgz').open('rb') as archive:
        expected = integrity(archive)
    deadline = time.monotonic() + 18 * 60
    while True:
        try:
            if public_copy_ready(name, version, expected, stable):
                print(f'Verified {name}@{version}', file=sys.stderr)
                print('stable=' + str(stable).lower())
                return
        except HTTPError as error:
            if error.code not in (404, 408, 429) and error.code < 500:
                raise
        except (URLError, socket.timeout, ConnectionError):
            pass
        if time.monotonic() >= deadline:
            raise TimeoutError(f'{name}@{version} not visible yet; rerun the refresh-index job')
        print(f'Waiting for public npm: {name}@{version}', file=sys.stderr)
        time.sleep(20)


if __name__ == '__main__':
    main()
