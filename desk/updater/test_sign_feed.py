import hashlib
import json
import tempfile
import io
import zipfile
import unittest
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
from sign_feed import sign
from prepare_assets import extract_assets

class SigningTests(unittest.TestCase):
    def test_asset_extraction_accepts_portable_paths_and_rejects_traversal(self):
        prefix='ProductivityMon.app/Contents/Resources/'
        for name,valid in [('models/pm0001/model.mon',True),('icons/1.png',True),('models/../../escaped',False),('models/bad\\path',False)]:
            with tempfile.TemporaryDirectory() as temporary:
                data=io.BytesIO()
                with zipfile.ZipFile(data,'w') as archive: archive.writestr(prefix+name,b'fixture')
                data.seek(0)
                with zipfile.ZipFile(data) as archive:
                    if valid:
                        extract_assets(archive,Path(temporary))
                        self.assertEqual((Path(temporary)/name).read_bytes(),b'fixture')
                    else:
                        with self.assertRaises(ValueError): extract_assets(archive,Path(temporary))

    def test_signing_refuses_wrong_keys_and_changed_packages(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            key = Ed25519PrivateKey.generate()
            private = key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())
            public = directory / 'public.hex'
            public.write_text(key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex())
            package = directory / 'ProductivityMon-0.0.1-full.nupkg'
            package.write_bytes(b'good')
            feed = {'Assets': [{'FileName': package.name, 'PackageId': 'ProductivityMon', 'Version': '0.0.1', 'Type': 'Full',
                                'SHA256': hashlib.sha256(b'good').hexdigest(), 'Size': 4}]}
            (directory / 'releases.win-alpha.json').write_text(json.dumps(feed))
            self.assertTrue(sign(directory, 'win-alpha', private, public_file=public).is_file())
            with self.assertRaises(ValueError): sign(directory, 'win-alpha', bytes(32), public_file=public)
            package.write_bytes(b'evil')
            with self.assertRaises(ValueError): sign(directory, 'win-alpha', private, public_file=public)

if __name__ == '__main__': unittest.main()
