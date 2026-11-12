"""Seed the release directory with a verified previous package so Velopack can generate small delta updates."""
import argparse
import base64
import hashlib
import json
import re
import shutil
import time
import urllib.error
import urllib.request
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

BASE = 'https://github.com/worldreboot/productivity-mon-desktop/releases/download/desktop-alpha/'

def prepare(output, channel):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    try:
        with urllib.request.urlopen(BASE + f'releases.{channel}.signed.json', timeout=30) as response:
            envelope = json.loads(response.read(4 * 1024 * 1024))
    except urllib.error.HTTPError as error:
        if error.code == 404:
            print('First updater release: no previous package to seed.')
            return
        raise
    payload = base64.b64decode(envelope['payload'], validate=True)
    key = bytes.fromhex(Path(__file__).with_name('public-key.hex').read_text().strip())
    Ed25519PublicKey.from_public_bytes(key).verify(base64.b64decode(envelope['signature'], validate=True), payload)
    metadata = json.loads(payload)
    if metadata['app_id'] != 'ProductivityMon' or metadata['channel'] != channel or metadata['expires'] <= time.time():
        raise ValueError('Invalid previous update feed')
    assets = [a for a in metadata['feed']['Assets'] if a['Type'] == 'Full']
    def version(asset):
        match = re.fullmatch(r'(\d+)\.(\d+)\.(\d+)-alpha\.(\d+)', asset['Version'])
        if not match: raise ValueError('Unexpected previous release version')
        return tuple(map(int, match.groups()))
    if not assets: return
    asset = max(assets, key=version)
    name = asset['FileName']
    if not re.fullmatch(r'[A-Za-z0-9_.-]+\.nupkg', name) or '..' in name: raise ValueError('Unsafe package name')
    path = output / name
    with urllib.request.urlopen(BASE + name, timeout=180) as response, path.open('xb') as target:
        shutil.copyfileobj(response, target)
    with path.open('rb') as file:
        if hashlib.file_digest(file, 'sha256').hexdigest().lower() != asset['SHA256'].lower() or path.stat().st_size != asset['Size']:
            raise ValueError('Previous package failed verification')
    (output / f'releases.{channel}.json').write_text(json.dumps({'Assets': [asset]}))
    print('Prepared verified previous package for delta generation.')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output')
    parser.add_argument('--channel', choices=['win-alpha', 'osx-alpha'], required=True)
    args = parser.parse_args()
    prepare(args.output, args.channel)
