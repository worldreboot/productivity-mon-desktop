"""Install v1, reject an unsigned update, then download/apply/relaunch v2 using the real native helper."""
import argparse
import functools
import http.server
import json
import os
import plistlib
import shutil
import subprocess
import threading
import time
import zipfile
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
from sign_feed import sign

def run(executable, work, vpk='vpk'):
    executable, work = Path(executable).resolve(), Path(work).resolve()
    work.mkdir(parents=True, exist_ok=False)
    channel = 'win-alpha' if os.name == 'nt' else 'osx-alpha'
    releases = work / 'releases'
    releases.mkdir()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(releases)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    key = Ed25519PrivateKey.generate()
    private = key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex()
    public_file = work / 'test-public.hex'
    public_file.write_text(public)
    profile = work / 'profile'
    profile.mkdir()
    (profile / 'token').write_bytes(b'fixture-account-kept')
    (profile / 'pending.jsonl').write_bytes(b'fixture-unsent-activity\n')
    config = {'root': str(work), 'base_url': f'http://127.0.0.1:{server.server_port}', 'public_key': public}
    installed = None
    for version in ['0.0.1', '0.0.2']:
        app = work / version
        if os.name == 'nt':
            app.mkdir()
            destination = app
        else:
            app = app.with_name(app.name + '.app')
            destination = app / 'Contents/MacOS'
            destination.mkdir(parents=True)
            with (app / 'Contents/Info.plist').open('wb') as file:
                plistlib.dump({'CFBundleIdentifier': 'com.productivitymon.updater-fixture', 'CFBundleName': 'ProductivityMon',
                              'CFBundleExecutable': 'update_fixture', 'CFBundlePackageType': 'APPL', 'CFBundleVersion': version,
                              'CFBundleShortVersionString': version, 'LSMinimumSystemVersion': '12.0'}, file)
        name = 'update_fixture.exe' if os.name == 'nt' else 'update_fixture'
        shutil.copy2(executable, destination / name)
        (destination / 'fixture.json').write_text(json.dumps(config))
        command = [vpk, 'pack', '-u', 'ProductivityMon', '-v', version, '-p', str(app), '-e', name,
                   '-r', 'win-x64' if os.name == 'nt' else 'osx-arm64', '-c', channel, '-o', str(releases), '--delta', 'None']
        if os.name != 'nt': command += ['--noInst']
        subprocess.run(command, check=True)
        if version == '0.0.1':
            if os.name == 'nt':
                setup = next(releases.glob('*Setup.exe'))
                subprocess.run([str(setup), '--silent', '--installto', str(work / 'installed')], check=True, timeout=90)
                installed = work / 'installed/current' / name
            else:
                portable = next(releases.glob('*Portable.zip'))
                with zipfile.ZipFile(portable) as zipped:
                    zipped.extractall(work / 'installed')
                installed = next((work / 'installed').rglob(name))
                for binary in installed.parent.iterdir():
                    if binary.is_file() and binary.name in (name, 'UpdateMac'): binary.chmod(0o755)
    signed = sign(releases, channel, private, public_file=public_file)
    valid = signed.read_bytes()
    envelope = json.loads(valid)
    envelope['signature'] = 'A' * 88
    signed.write_text(json.dumps(envelope))
    subprocess.run([str(installed)], check=True, timeout=30)
    rejected = json.loads((work / 'result.json').read_text())
    assert rejected['result'] == 'rejected' and rejected['version'] == '0.0.1', rejected
    signed.write_bytes(valid)
    subprocess.run([str(installed)], check=True, timeout=90)
    deadline = time.monotonic() + 90
    result = {}
    while time.monotonic() < deadline:
        try: result = json.loads((work / 'result.json').read_text())
        except (FileNotFoundError, json.JSONDecodeError): pass
        if result.get('result') == 'updated': break
        time.sleep(0.5)
    assert result.get('result') == 'updated' and result['profilePreserved'], result
    server.shutdown()
    report = {'platform': channel, 'nativeInstall': True, 'unsignedUpdateRejected': True, 'downloadApplyRelaunch': True, 'accountAndOutboxPreserved': True}
    (work.parent / ('updater-smoke-' + channel + '.json')).write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--executable', required=True)
    parser.add_argument('--work', required=True)
    parser.add_argument('--vpk', default='vpk')
    args = parser.parse_args()
    run(args.executable, args.work, args.vpk)
