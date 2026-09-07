#!/usr/bin/env python3
"""Explicit trusted-host provisioning; never imported by the MCP runtime."""
import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import urllib.request


parser = argparse.ArgumentParser()
parser.add_argument('--docker', required=True, help='Absolute trusted Docker executable')
parser.add_argument('--host', required=True, help='Explicit trusted local Unix socket URI')
parser.add_argument('--output', required=True, type=pathlib.Path)
parser.add_argument('--plugins', action='store_true',
                    help='Build the separate M3 image with the exact plugin manifest')
args = parser.parse_args()
if not pathlib.Path(args.docker).is_absolute() or not args.host.startswith('unix:///'):
    parser.error('Absolute Docker path and local Unix socket required')

here = pathlib.Path(__file__).resolve().parent
sources = json.loads((here / 'sources.json').read_text())
plugins = sources.get('plugins', []) if args.plugins else []
if args.plugins and {item['id'] for item in plugins} != {
        'cargo-nextest', 'cargo-llvm-cov', 'llvm-tools', 'cargo-semver-checks', 'cargo-mutants'}:
    raise SystemExit('M3 plugin manifest is not the exact authorized component set')
active = [*sources['components'], *plugins]
archive_components = [item for item in active if item['kind'] != 'source-build']
for item in active:
    if item['kind'] not in {'release-archive', 'rust-component', 'source-build'}:
        raise SystemExit(f"Unsupported source kind: {item.get('id', item.get('package'))}")
    if item['kind'] == 'source-build':
        if item.get('sha256') is not None or item.get('size') is not None:
            raise SystemExit(f"Source build must not claim an archive digest: {item['id']}")
        continue
    if not item.get('url') or not re.fullmatch(r'[0-9a-f]{64}', item.get('sha256', '')):
        raise SystemExit(f"Incomplete archive source entry: {item.get('id', item.get('package'))}")
    if 'size' in item and (not isinstance(item['size'], int) or item['size'] < 0):
        raise SystemExit(f"Invalid archive size: {item.get('id', item.get('package'))}")

args.output.mkdir(parents=True, exist_ok=True)
context = args.output / 'build-context'
context.mkdir(parents=True, exist_ok=True)

# A replay must not extract unrelated archives left in the build context.
expected = {item['url'].rsplit('/', 1)[1] for item in archive_components}
expected.update({'Dockerfile', 'SHA256SUMS'})
for entry in context.iterdir():
    if entry.name not in expected or not entry.is_file() or entry.is_symlink():
        raise SystemExit('Unexpected or linked build-context entry; refusing provisioning')


def digest_and_size(path):
    size = path.stat().st_size
    with path.open('rb') as archive:
        digest = hashlib.file_digest(archive, 'sha256').hexdigest()
    return digest, size


checks = []
archive_receipts = []
for component in archive_components:
    filename = component['url'].rsplit('/', 1)[1]
    target = context / filename
    if target.exists() and (not target.is_file() or target.is_symlink()):
        raise SystemExit(f'Unexpected or linked archive path: {target}')
    if not target.exists():
        with urllib.request.urlopen(component['url'], timeout=60) as response, target.open('wb') as output:
            shutil.copyfileobj(response, output)
    digest, size = digest_and_size(target)
    if digest != component['sha256'] or ('size' in component and size != component['size']):
        raise SystemExit(f'Checksum or size mismatch: {target}')
    checks.append(f"{digest}  {target.name}\n")
    archive_receipts.append(dict(id=component.get('id', component.get('package')),
                                 filename=target.name, sha256=digest, size=size,
                                 url=component['url']))
(context / 'SHA256SUMS').write_text(''.join(checks))
shutil.copyfile(here / 'Dockerfile', context / 'Dockerfile')

docker = [args.docker, '--host', args.host]
tag = sources['provisioning']['m3_tag' if args.plugins else 'm1_tag']
subprocess.run(docker + ['pull', '--platform', 'linux/arm64', sources['base']], check=True)
with (args.output / 'base-inspect.json').open('w') as output:
    subprocess.run(docker + ['image', 'inspect', sources['base']], stdout=output, check=True)
with (args.output / 'build.log').open('w') as output:
    build = docker + ['build', '--platform', 'linux/arm64', '--progress', 'plain',
                      '--build-arg', f"ENABLE_PLUGINS={'1' if args.plugins else '0'}",
                      '--tag', tag, '--iidfile', str(args.output / 'image-id'), str(context)]
    subprocess.run(build, stdout=output, stderr=subprocess.STDOUT, check=True)

build_log_sha256, build_log_size = digest_and_size(args.output / 'build.log')
build_log = (args.output / 'build.log').read_text(errors='replace')


def marker(name):
    match = re.findall(rf'{re.escape(name)}=([0-9a-f]{{64}}|unavailable)', build_log)
    return match[-1] if match else None


source_build = next((item for item in plugins if item['kind'] == 'source-build'), None)
source_receipt = None
if source_build is not None:
    binary_sha256 = marker('cargo-mutants-binary-sha256')
    if binary_sha256 is None:
        raise SystemExit('Builder did not emit the cargo-mutants binary digest marker')
    source_receipt = dict(source_build=source_build['source_build'],
                          registry_index_checksum=marker('cargo-mutants-registry-index-checksum'),
                          crate_sha256=marker('cargo-mutants-crate-sha256'),
                          binary_sha256=binary_sha256,
                          build_log_sha256=build_log_sha256)
receipt = {
    'profile': 'm3' if args.plugins else 'm1',
    'tag': tag,
    'image_id': (args.output / 'image-id').read_text().strip(),
    'archives': archive_receipts,
    'source_build': source_receipt,
    'build_log': {'path': 'build.log', 'sha256': build_log_sha256, 'size': build_log_size},
}
(args.output / 'provisioning-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(receipt['image_id'])
