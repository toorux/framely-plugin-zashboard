"""Build and verify an offline Framely ZIP without requiring the host in CI."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import stat
import subprocess
import zipfile
from submit_database import release_url

ROOT = Path(__file__).resolve().parent.parent
PAYLOAD_ROOTS = {
    'backend', 'mihomo', 'page.js', 'bridge.js', 'window.js', 'icon.png',
    'source.zip', 'upstream.lock.json', 'README.md', 'THIRD_PARTY_NOTICES.md',
    'GEO_DATA_NOTICES.md', 'LICENSE', 'LICENSE.framely-sdk', 'LICENSE.mihomo',
    'LICENSE.zashboard', 'LICENSE.react', 'LICENSE.react-dom',
    'dashboard', 'geo', 'upstream',
}


def pack(root=ROOT):
    root = Path(root)
    manifest = json.loads((root / 'manifest.json').read_text())
    repository = os.environ.get('GITHUB_REPOSITORY', 'toorux/framely-plugin-zashboard')
    manifest['downloadUrl'] = release_url(manifest, repository)
    manifest.pop('downloadSha256', None)
    payload = root / 'payload'
    if {p.name for p in payload.iterdir()} != PAYLOAD_ROOTS:
        raise ValueError('Unexpected/missing payload entries; rebuild the payload')
    files = {}
    for path in sorted(payload.rglob('*')):
        metadata = path.lstat()
        if stat.S_ISDIR(metadata.st_mode):
            continue
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError('Payload must not contain symlinks or special files')
        name = path.relative_to(payload).as_posix()
        files[name] = path.read_bytes()
    manifest['files'] = {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}
    output = root / 'dist' / f"{manifest['id']}-{manifest['version']}.framely"
    output.parent.mkdir(exist_ok=True)
    raw = json.dumps(manifest, ensure_ascii=False, separators=(',', ':')).encode()
    with zipfile.ZipFile(output, 'w') as archive:
        for name, data in {'manifest.json': raw, **files}.items():
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    with zipfile.ZipFile(output) as archive:
        if archive.testzip() is not None:
            raise ValueError('Archive CRC verification failed')
        packed = json.loads(archive.read('manifest.json'))
        if set(archive.namelist()) != {'manifest.json', *packed['files']}:
            raise ValueError('Archive does not match manifest file list')
        for name, digest in packed['files'].items():
            if hashlib.sha256(archive.read(name)).hexdigest() != digest:
                raise ValueError('Payload hash mismatch: ' + name)
    host = os.environ.get('FRAMELY_BIN') or shutil.which('framely')
    candidate = root.parent.parent / 'framely/target/release/framely'
    if not host and candidate.is_file():
        host = str(candidate)
    if host:
        result = subprocess.run([host, 'verify', str(output)], check=True, capture_output=True, text=True)
        verified = json.loads(result.stdout)['manifest']
        assert verified['id'] == manifest['id'] and verified['version'] == manifest['version']
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    checksum = f'{digest}  {output.name}\n'
    Path(str(output) + '.sha256').write_text(checksum)
    (output.parent / 'SHA256SUMS').write_text(checksum)
    print('Verified', output)
    return output


if __name__ == '__main__':
    pack()
