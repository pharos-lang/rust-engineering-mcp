#!/usr/bin/env python3
"""Deterministic local-review archives; no publication or license authorization."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

OUT = Path(__file__).resolve().parent


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    installation = json.loads((OUT / 'installation-receipt.json').read_text())
    if installation['status'] != 'passed':
        raise RuntimeError('Successful private installation receipt required')
    manifest = json.loads((OUT / 'manifest.json').read_text())
    archives = []
    for feature in ('core', 'local'):
        files = [row for row in manifest['files']
                 if row['path'].startswith(feature + '/')
                 or row['path'].startswith('assets/')
                 and (feature == 'local' or not row['path'].startswith('assets/model'))]
        package_manifest = {'scope': 'local-review-only-not-distribution', 'feature': feature,
                            'product_license': 'owner_pending',
                            'fixture_trust': 'public_test_key_no_publisher_authority', 'files': files}
        archive = OUT / (feature + '.local-review.tar.gz')
        with archive.open('wb') as raw:
            with gzip.GzipFile(filename='', mode='wb', compresslevel=1, fileobj=raw, mtime=0) as compressed:
                with tarfile.open(mode='w|', fileobj=compressed, format=tarfile.PAX_FORMAT) as tar:
                    for row in files:
                        path = OUT / row['path']
                        if path.is_symlink() or not path.is_file() or sha(path) != row['sha256']:
                            raise RuntimeError('Candidate changed before packaging: ' + row['path'])
                        info = tarfile.TarInfo(row['path'])
                        info.size = row['bytes']
                        info.mode = int(row['mode'], 8)
                        with path.open('rb') as stream:
                            tar.addfile(info, stream)
                    data = (json.dumps(package_manifest, indent=2) + '\n').encode()
                    info = tarfile.TarInfo('MANIFEST.local-review.json')
                    info.size = len(data)
                    info.mode = 0o600
                    tar.addfile(info, io.BytesIO(data))
        archive.chmod(0o600)
        expected = {row['path']: (row['bytes'], row['sha256']) for row in files}
        expected['MANIFEST.local-review.json'] = (len(data), hashlib.sha256(data).hexdigest())
        seen = set()
        with tarfile.open(archive, mode='r|gz') as tar:
            for member in tar:
                if not member.isfile() or member.name not in expected or member.name in seen:
                    raise RuntimeError('Unexpected archive member')
                length, digest = expected[member.name]
                if member.size != length:
                    raise RuntimeError('Archive member size mismatch')
                with tar.extractfile(member) as stream:
                    if hashlib.file_digest(stream, 'sha256').hexdigest() != digest:
                        raise RuntimeError('Archive payload hash mismatch')
                seen.add(member.name)
        if seen != set(expected):
            raise RuntimeError('Archive is incomplete')
        archives.append({'feature': feature, 'path': archive.name,
                         'compressed_bytes': archive.stat().st_size,
                         'payload_bytes': sum(row['bytes'] for row in files), 'sha256': sha(archive),
                         'archive_payload_hashes_verified': True})
    (OUT / 'archive-receipt.json').write_text(json.dumps({'scope': 'local-review-only-not-distribution',
        'archives': archives, 'installation_source': 'hashed unpacked candidate directory; archive bytes separately verified'}, indent=2) + '\n')
    print('PASS local-review archives and hashes', flush=True)


if __name__ == '__main__':
    main()
