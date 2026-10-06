"""Bundle corresponding source using an explicit allowlist."""
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parent.parent
entries = ['.gitattributes', '.github', 'manifest.json', 'upstream.lock.json', 'package.json', 'package-lock.json', 'tsconfig.json', 'icon.png', 'LICENSE', 'README.md', 'THIRD_PARTY_NOTICES.md', 'GEO_DATA_NOTICES.md', '.gitignore', 'backend/Cargo.toml', 'backend/Cargo.lock', 'backend/src', 'ui', 'scripts', 'tests', 'assets', 'vendor']
with zipfile.ZipFile(root / 'payload/source.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for entry in entries:
        path = root / entry
        for item in ([path] if path.is_file() else path.rglob('*')):
            rel = item.relative_to(root)
            if item.is_file() and not item.is_symlink() and not set(rel.parts) & {'target', 'node_modules', '.git', '.build', '__pycache__'}:
                archive.write(item, rel)
