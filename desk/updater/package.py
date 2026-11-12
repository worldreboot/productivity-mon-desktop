"""Package prepared desktop assets using pinned Velopack. Signing the update feed is a separate final step."""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VELOPACK_VERSION = '1.2.161'

def package(platform, version, executable, assets, output, vpk='vpk'):
    if not re.fullmatch(r'\d+\.\d+\.\d+-alpha\.\d+', version):
        raise ValueError('Use a monotonically increasing version, e.g. 0.1.0-alpha.9')
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage = output / ('stage-' + platform)
    if stage.exists():
        raise ValueError('Use a fresh staging directory')
    stage.mkdir()
    executable, assets = Path(executable).resolve(), Path(assets).resolve()
    for kind in ['models', 'icons']:
        if not (assets / kind).is_dir():
            raise ValueError(f'Missing {kind} assets')
    metadata = {'version': version, 'channel': 'alpha' if platform == 'windows' else 'experimental-alpha',
                'server': 'https://api.productivitymon.com', 'architecture': 'x64' if platform == 'windows' else 'arm64', 'autoUpdate': True}
    if platform == 'windows':
        app = stage / 'app'
        app.mkdir()
        shutil.copy2(executable, app / 'desk-mons.exe')
        for kind in ['models', 'icons']:
            shutil.copytree(assets / kind, app / kind)
        icon = ROOT / 'desk/app/assets/productivitymon.ico'
    else:
        subprocess.run([sys.executable, str(ROOT / 'desk/macos/package.py'), '--executable', str(executable),
                        '--models', str(assets / 'models'), '--icons', str(assets / 'icons'), '--output', str(stage),
                        '--version', version, '--channel', 'experimental-alpha', '--build-number', str(int(version.rsplit('.', 1)[1]))], check=True)
        app = stage / 'ProductivityMon.app'
        icon = app / 'Contents/Resources/ProductivityMon.icns'
    resources = app if platform == 'windows' else app / 'Contents/Resources'
    (resources / 'server.txt').write_text(metadata['server'] + '\n')
    (resources / 'release.json').write_text(json.dumps(metadata, indent=2) + '\n')
    channel = 'win-alpha' if platform == 'windows' else 'osx-alpha'
    command = [vpk, 'pack', '--packId', 'ProductivityMon', '--packTitle', 'ProductivityMon', '--packAuthors', 'ProductivityMon',
               '--packVersion', version, '--packDir', str(app), '--mainExe', 'desk-mons.exe' if platform == 'windows' else 'desk-mons',
               '--runtime', 'win-x64' if platform == 'windows' else 'osx-arm64', '--channel', channel,
               '--outputDir', str(output), '--icon', str(icon), '--delta', 'None']
    if platform == 'windows':
        command += ['--shortcuts', 'StartMenuRoot', '--noPortable']
        # Optional OS code signing. Update-feed signatures are always mandatory, independently of these credentials.
        if os.environ.get('PRODUCTIVITYMON_WINDOWS_SIGN_PARAMS'):
            command += ['--signParams', os.environ['PRODUCTIVITYMON_WINDOWS_SIGN_PARAMS']]
    else:
        command += ['--noInst']
        if os.environ.get('PRODUCTIVITYMON_APPLE_SIGN_IDENTITY'):
            command += ['--signAppIdentity', os.environ['PRODUCTIVITYMON_APPLE_SIGN_IDENTITY']]
        if os.environ.get('PRODUCTIVITYMON_APPLE_NOTARY_PROFILE'):
            command += ['--notaryProfile', os.environ['PRODUCTIVITYMON_APPLE_NOTARY_PROFILE']]
    subprocess.run(command, check=True)
    source = next(output.glob('*Setup.exe')) if platform == 'windows' else next(output.glob('*Portable.zip'))
    name = f'ProductivityMon-{version}-windows-Setup.exe' if platform == 'windows' else f'ProductivityMon-{version}-apple-silicon.zip'
    source.rename(output / name)
    return channel

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=['windows', 'macos'], required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--executable', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True, help='Prepared directory containing models/ and icons/.')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--vpk', default='vpk')
    args = parser.parse_args()
    package(args.platform, args.version, args.executable, args.assets, args.output, args.vpk)
