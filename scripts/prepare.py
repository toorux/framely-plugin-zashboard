"""Materialize offline core, dashboard, databases and upstream source."""
import gzip
import pathlib
import shutil
import tarfile
import zipfile
import re

root = pathlib.Path(__file__).resolve().parent.parent
payload = root / 'payload'
payload.mkdir(exist_ok=True)
core_staging=payload / '.mihomo.tmp'
core_staging.write_bytes(gzip.decompress((root / '.build/mihomo.gz').read_bytes()))
core_staging.chmod(0o755)
core_staging.replace(payload / 'mihomo')
with zipfile.ZipFile(root / '.build/zashboard.zip') as archive:
    for info in archive.infolist():
        path = pathlib.PurePosixPath(info.filename)
        if path.is_absolute() or '..' in path.parts or path.parts[0] != 'dist' or (info.external_attr >> 16) & 0o170000 == 0o120000:
            raise ValueError('Unsafe dashboard member')
        if info.is_dir():
            continue
        target = payload / 'dashboard' / pathlib.Path(*path.parts[1:])
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(archive.read(info))
index = payload / 'dashboard/index.html'
html = index.read_text()
# Service workers can retain older plugin assets after an update. Use ordinary
# static resources and let the plugin package control versions.
html = re.sub(r'<script[^>]+id="vite-plugin-pwa:register-sw"[^>]*>.*?</script>', '', html)
html = html.replace('<head>', '<head><script src="./framely-bridge.js" defer></script>')
index.write_text(html)
(payload / 'geo').mkdir(exist_ok=True)
for name, target in [('geoip.dat','geoip.dat'),('geosite.dat','geosite.dat'),('country.mmdb','Country.mmdb')]:
    shutil.copyfile(root / '.build' / name, payload / 'geo' / target)
(payload / 'upstream').mkdir(exist_ok=True)
for name in ['mihomo', 'zashboard']:
    archive_path = root / '.build' / f'{name}-source.tar.gz'
    shutil.copyfile(archive_path, payload / 'upstream' / archive_path.name)
    with tarfile.open(archive_path) as archive:
        member = next(m for m in archive.getmembers() if m.name.count('/') == 1 and m.name.endswith('/LICENSE'))
        (payload / f'LICENSE.{name}').write_bytes(archive.extractfile(member).read())
shutil.copyfile(root / '.build/zashboard-source.tar.gz', payload / 'upstream/zashboard-source.tar.gz')
