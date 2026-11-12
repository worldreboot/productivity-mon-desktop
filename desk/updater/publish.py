"""Publish immutable versioned packages first, then atomically replace each signed channel feed."""
import argparse
import hashlib
import json
import os
import subprocess
from pathlib import Path

REPO = 'worldreboot/productivity-mon-desktop'
TAG = 'desktop-alpha'

def gh(*args):
    return subprocess.run(['gh', *args, '--repo', REPO], check=True, capture_output=True, text=True).stdout

def publish(directory, version):
    directory = Path(directory)
    try:
        release = json.loads(gh('release', 'view', TAG, '--json', 'assets'))
    except subprocess.CalledProcessError:
        gh('release', 'create', TAG, '--target', os.environ['GITHUB_SHA'], '--prerelease', '--title', 'ProductivityMon desktop alpha', '--notes', 'Windows and experimental Apple Silicon alpha. Install once to enable signed automatic updates.')
        release = {'assets': []}
    existing = {asset['name'] for asset in release['assets']}
    for path in directory.iterdir():
        if path.suffix in ('.nupkg', '.exe', '.zip') and version in path.name and path.name in existing:
            raise ValueError('This version already has published files. Use a new increasing version; immutable binaries are never replaced.')
    manifests = []
    for platform, pattern in [('windows', f'ProductivityMon-{version}-windows-Setup.exe'), ('macos', f'ProductivityMon-{version}-apple-silicon.zip')]:
        path = directory / pattern
        with path.open('rb') as file:
            digest = hashlib.file_digest(file, 'sha256').hexdigest()
        checksum = directory / (path.name + '.sha256')
        checksum.write_text(digest + '  ' + path.name + '\n')
        manifests.append({'platform': platform, 'architecture': 'x64' if platform == 'windows' else 'arm64', 'version': version,
                          'filename': path.name, 'url': f'https://github.com/{REPO}/releases/download/{TAG}/{path.name}',
                          'sha256': digest, 'bytes': path.stat().st_size, 'autoUpdate': True, 'signed': False, 'notarized': False,
                          'channel': 'alpha' if platform == 'windows' else 'experimental-alpha', 'modelCount': 952, 'shinyModelCount': 952,
                          'server': 'https://api.productivitymon.com', 'minimumMacOS': '12.0' if platform == 'macos' else None})
        manifests[-1]['sourceCommit']=os.environ['GITHUB_SHA']
        manifests[-1]['buildWorkflow']=f'https://github.com/{REPO}/actions/runs/' + os.environ['GITHUB_RUN_ID']
    # Never overwrite an existing version's binary. A retry only uploads missing immutable files.
    for pattern in ['*.nupkg', '*.exe', '*.zip', '*.sha256']:
        for path in directory.glob(pattern):
            if path.name not in existing:
                gh('release', 'upload', TAG, str(path))
    # Replacements happen only after both native builds/tests/package uploads completed successfully.
    for path in directory.glob('*.signed.json'):
        gh('release', 'upload', TAG, str(path), '--clobber')
    metadata = directory / f'ProductivityMon-{version}-downloads.json'
    metadata.write_text(json.dumps(manifests, indent=2) + '\n')
    if metadata.name not in existing:
        gh('release', 'upload', TAG, str(metadata))
    print(json.dumps({'published': version, 'platforms': ['windows', 'macos'], 'signedFeeds': 2}))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    publish(args.directory, args.version)
