"""Select releases by comparing both manifests at the ends of a main-branch push."""
import json
import re
import subprocess
import sys
import tomllib


def precedence(value):
    number = r'(0|[1-9][0-9]*)'
    part = r'[0-9A-Za-z-]+'
    match = re.fullmatch(number + r'\.' + number + r'\.' + number
                         + rf'(?:-({part}(?:\.{part})*))?(?:\+{part}(?:\.{part})*)?', value)
    if not match:
        raise ValueError(f"Invalid SemVer: {value}")
    pre = match[4]
    identifiers = pre.split('.') if pre else []
    if any(i.isdigit() and len(i) > 1 and i.startswith('0') for i in identifiers):
        raise ValueError(f"Invalid numeric prerelease identifier: {value}")
    return (*map(int, match.groups()[:3]), pre is None,
            tuple((0, int(i)) if i.isdigit() else (1, i) for i in identifiers))


def read(ref):
    def file(path):
        return subprocess.check_output(['git', 'show', f'{ref}:{path}'], text=True)
    return {
        'cli': tomllib.loads(file('Cargo.toml'))['package']['version'],
        'plugin': json.loads(file('integrations/dsh/package.json'))['version'],
    }


if __name__ == '__main__':
    before = sys.argv[1]
    if not re.fullmatch('[0-9a-f]{40}', before):
        raise ValueError('Invalid push base')
    old = read(before) if before.strip('0') else {}
    for key, value in read('HEAD').items():
        greater = key not in old or precedence(value) > precedence(old[key])
        print(f'{key}={str(greater).lower()}')
        print(f'{key}_version={value}')
