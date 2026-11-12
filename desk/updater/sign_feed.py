"""Sign a Velopack feed after verifying every referenced package. Private keys never enter build output."""
import argparse
import base64
import hashlib
import json
import os
import re
import time
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

APP_ID = 'ProductivityMon'

def sign(directory, channel, private_bytes, *, expires=None, public_file=None):
    directory = Path(directory).resolve()
    if channel not in ('win-alpha', 'osx-alpha'):
        raise ValueError('Unsupported update channel')
    key = Ed25519PrivateKey.from_private_bytes(private_bytes)
    public_file = public_file or Path(__file__).with_name('public-key.hex')
    if key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex() != public_file.read_text().strip():
        raise ValueError('Signing key does not match the public key embedded in the app')
    feed = json.loads((directory / f'releases.{channel}.json').read_text(encoding='utf-8-sig'))
    names = set()
    for asset in feed['Assets']:
        name = asset['FileName']
        if not re.fullmatch(r'[A-Za-z0-9_.-]{1,199}\.nupkg', name) or '..' in name or name in names:
            raise ValueError('Unsafe or duplicate package filename')
        names.add(name)
        if asset['PackageId'] != APP_ID or asset['Type'] not in ('Full', 'Delta'):
            raise ValueError('Wrong package identity')
        package = directory / name
        with package.open('rb') as file:
            digest = hashlib.file_digest(file, 'sha256').hexdigest()
        if digest.lower() != asset['SHA256'].lower() or package.stat().st_size != asset['Size']:
            raise ValueError(f'Package does not match the feed: {name}')
    payload = json.dumps({'schema': 1, 'app_id': APP_ID, 'channel': channel,
                          'expires': expires or int(time.time()) + 180 * 86400, 'feed': feed}, separators=(',', ':')).encode()
    envelope = {'payload': base64.b64encode(payload).decode(), 'signature': base64.b64encode(key.sign(payload)).decode()}
    destination = directory / f'releases.{channel}.signed.json'
    temporary = destination.with_suffix('.next')
    temporary.write_text(json.dumps(envelope, separators=(',', ':')) + '\n', encoding='utf-8')
    temporary.replace(destination)
    return destination

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--channel', choices=['win-alpha', 'osx-alpha'], required=True)
    parser.add_argument('--key-file', type=Path, help='Restricted local raw Ed25519 key; CI uses PRODUCTIVITYMON_UPDATE_SIGNING_KEY hex secret.')
    args = parser.parse_args()
    private = args.key_file.read_bytes() if args.key_file else bytes.fromhex(os.environ['PRODUCTIVITYMON_UPDATE_SIGNING_KEY'])
    print('Signed update feed:', sign(args.directory, args.channel, private).name)
