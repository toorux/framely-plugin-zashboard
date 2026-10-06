"""Fetch exact hash-locked upstream bytes. Never install or execute them."""
import hashlib
import json
import pathlib
import urllib.request

root = pathlib.Path(__file__).resolve().parent.parent
cache = root / '.build'
cache.mkdir(exist_ok=True)
lock = json.loads((root / 'upstream.lock.json').read_text())
for asset in lock['assets']:
    target = cache / asset['file']
    if not target.exists():
        with urllib.request.urlopen(asset['url'], timeout=60) as response:
            content = response.read(100 * 1024 * 1024 + 1)
        if len(content) > 100 * 1024 * 1024:
            raise ValueError('Upstream asset too large')
        if hashlib.sha256(content).hexdigest() != asset['sha256']:
            raise ValueError(f"Upstream bytes changed: {asset['file']}")
        target.write_bytes(content)
    if hashlib.sha256(target.read_bytes()).hexdigest() != asset['sha256']:
        raise ValueError(f"Checksum mismatch: {asset['file']}")
    print('Verified', asset['file'])
