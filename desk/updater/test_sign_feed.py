import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
from sign_feed import sign

class SigningTests(unittest.TestCase):
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
