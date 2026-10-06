"""Store icons must resolve at the same path in source and the built package."""
import json
from pathlib import Path
import unittest
import zipfile

ROOT = Path(__file__).resolve().parent.parent


class PublishTests(unittest.TestCase):
    def test_store_icon_matches_source_payload_and_bundled_source(self):
        manifest = json.loads((ROOT / 'manifest.json').read_text())
        icon = manifest['icon']
        source = (ROOT / icon).read_bytes()
        self.assertEqual((ROOT / 'payload' / icon).read_bytes(), source)
        with zipfile.ZipFile(ROOT / 'payload/source.zip') as archive:
            self.assertEqual(archive.read(icon), source)
