#!/usr/bin/env python3
"""Explicit trusted-host provisioning; never imported by the MCP runtime."""
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument('--docker', required=True, help='Absolute trusted Docker executable')
parser.add_argument('--host', required=True, help='Explicit trusted local Unix socket URI')
parser.add_argument('--output', required=True, type=pathlib.Path)
args = parser.parse_args()
if not pathlib.Path(args.docker).is_absolute() or not args.host.startswith('unix:///'):
    parser.error('Absolute Docker path and local Unix socket required')
here = pathlib.Path(__file__).resolve().parent
sources = json.loads((here / 'sources.json').read_text())
context = args.output / 'build-context'
context.mkdir(parents=True, exist_ok=True)
# A replay must not extract unrelated archives left in the build context.
expected = {item['url'].rsplit('/', 1)[1] for item in sources['components']}
expected.update({'Dockerfile', 'SHA256SUMS'})
if any(item.name not in expected or not item.is_file() or item.is_symlink()
       for item in context.iterdir()):
    raise SystemExit('Unexpected or linked build-context entry; refusing provisioning')
checks = []
for component in sources['components']:
    target = context / component['url'].rsplit('/', 1)[1]
    if not target.exists():
        with urllib.request.urlopen(component['url'], timeout=60) as response, target.open('wb') as output:
            shutil.copyfileobj(response, output)
    with target.open('rb') as archive:
        digest = hashlib.file_digest(archive, 'sha256').hexdigest()
    if digest != component['sha256']:
        raise SystemExit(f'Checksum mismatch: {target}')
    checks.append(f"{component['sha256']}  {target.name}\n")
(context / 'SHA256SUMS').write_text(''.join(checks))
shutil.copyfile(here / 'Dockerfile', context / 'Dockerfile')
docker = [args.docker, '--host', args.host]
subprocess.run(docker + ['pull', '--platform', 'linux/arm64', sources['base']], check=True)
with (args.output / 'base-inspect.json').open('w') as output:
    subprocess.run(docker + ['image', 'inspect', sources['base']], stdout=output, check=True)
with (args.output / 'build.log').open('w') as output:
    subprocess.run(docker + ['build', '--platform', 'linux/arm64', '--progress', 'plain', '--tag',
                            'rust-engineering-runtime:1.98.1-arm64', '--iidfile',
                            str(args.output / 'image-id'), str(context)],
                   stdout=output, stderr=subprocess.STDOUT, check=True)
print((args.output / 'image-id').read_text().strip())
