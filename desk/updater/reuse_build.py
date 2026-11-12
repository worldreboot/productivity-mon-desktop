"""Reuse a native build only after verifying unchanged app sources and successful native acceptance steps."""
import argparse
import io
import json
import os
import subprocess
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

REPO = 'worldreboot/productivity-mon-desktop'
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl): return None

def api(path, binary=False):
    request = urllib.request.Request('https://api.github.com/repos/' + REPO + path,
                                    headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json', 'User-Agent': 'ProductivityMon release builder'})
    try: response = urllib.request.build_opener(NoRedirect).open(request, timeout=60)
    except urllib.error.HTTPError as error:
        if error.code != 302: raise
        url = error.headers['Location']
        if not url.startswith('https://'): raise ValueError('Non-HTTPS artifact redirect')
        # Never forward the GitHub token to artifact storage.
        response = urllib.request.urlopen(url, timeout=120)
    with response: data = response.read()
    return data if binary else json.loads(data)

def reuse(run_id, channel, output):
    if not str(run_id).isdigit(): raise ValueError('Expected a numeric Actions run ID')
    run = api(f'/actions/runs/{run_id}')
    if run['head_repository']['full_name'] != REPO or run['workflow_id'] != 415516940 or run['event'] != 'workflow_dispatch':
        raise ValueError('Expected this repository’s desktop release workflow')
    sha = run['head_sha']
    subprocess.run(['git', 'fetch', '--depth=1', 'origin', sha], check=True)
    subprocess.run(['git', 'diff', '--exit-code', sha, 'HEAD', '--', 'desk/app', 'desk/macos', 'packages/content/data/dex.json'], check=True)
    jobs = api(f'/actions/runs/{run_id}/jobs')['jobs']
    job = next(j for j in jobs if channel in j['name'])
    required = ['Test desktop and updater security', 'Test install, rejection, update and relaunch', 'Build desktop executable']
    for name in required:
        if not any(step['name'] == name and step['conclusion'] == 'success' for step in job['steps']):
            raise ValueError('Native acceptance did not pass: ' + name)
    if channel == 'osx-alpha' and not any(s['name'] == 'Native Mac rendering smoke' and s['conclusion'] == 'success' for s in job['steps']):
        raise ValueError('Mac rendering check did not pass')
    artifacts = api(f'/actions/runs/{run_id}/artifacts')['artifacts']
    def download(name):
        artifact = next(a for a in artifacts if a['name'] == name and not a['expired'])
        return zipfile.ZipFile(io.BytesIO(api(f'/actions/artifacts/{artifact["id"]}/zip', binary=True)))
    with download('updater-smoke-' + channel) as archive:
        report = json.loads(archive.read('updater-smoke-' + channel + '.json'))
    if not all(report.get(field) is True for field in ['nativeInstall', 'unsignedUpdateRejected', 'downloadApplyRelaunch', 'accountAndOutboxPreserved']):
        raise ValueError('Invalid native acceptance report')
    Path('updater-smoke-' + channel + '.json').write_text(json.dumps(report, indent=2))
    name = 'desk-mons.exe' if channel == 'win-alpha' else 'desk-mons'
    with download('updater-development-' + channel) as archive:
        if archive.namelist() != [name]: raise ValueError('Unexpected native artifact contents')
        destination = Path(output)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(archive.read(name))
        if channel == 'osx-alpha': destination.chmod(0o755)
    print(json.dumps({'reusedNativeRun': run_id, 'nativeSource': sha, 'unchangedRuntimeSources': True, 'channel': channel}))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run_id')
    parser.add_argument('--channel', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    reuse(args.run_id, args.channel, args.output)
