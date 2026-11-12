"""Reuse the verified companion assets from the current public Mac ZIP for both native build hosts."""
import argparse
import hashlib
import json
import shutil
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def prepare(output):
    output = Path(output).resolve()
    if output.exists():
        raise ValueError('Asset output must be a new directory')
    manifest = json.loads((ROOT / 'desk/website/public/release-mac.json').read_text())
    # Pin the known, complete asset bundle. New updater releases do not change this source implicitly.
    url = 'https://api.productivitymon.com/downloads/ProductivityMon-0.1.0-alpha.mac.2-apple-silicon.zip'
    expected = '952d250ee3474f3ca861030e11fc12a29a5f0d25ce2e4486069c4407e212f961'
    with tempfile.TemporaryDirectory(prefix='productivitymon-assets-') as temporary:
        archive = Path(temporary) / 'assets.zip'
        with urllib.request.urlopen(url, timeout=180) as response, archive.open('wb') as file:
            shutil.copyfileobj(response, file)
        with archive.open('rb') as file:
            if hashlib.file_digest(file, 'sha256').hexdigest() != expected:
                raise ValueError('Published asset checksum does not match')
        output.mkdir(parents=True)
        with zipfile.ZipFile(archive) as zipped:
            prefix = 'ProductivityMon.app/Contents/Resources/'
            for entry in zipped.infolist():
                if not entry.filename.startswith((prefix + 'models/', prefix + 'icons/')) or entry.is_dir():
                    continue
                relative = Path(entry.filename[len(prefix):])
                if relative.is_absolute() or '..' in relative.parts or '\\' in str(relative):
                    raise ValueError('Unsafe archive path')
                destination = output / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                with zipped.open(entry) as source, destination.open('xb') as target:
                    shutil.copyfileobj(source, target)
        for kind in ['models', 'icons']:
            if not (output / kind).is_dir():
                raise ValueError('Asset archive is incomplete')
    print('Prepared verified model and icon assets.')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    prepare(parser.parse_args().output)
