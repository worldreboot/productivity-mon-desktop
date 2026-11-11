"""Package an already-built Apple Silicon executable and locally prepared assets; never publish or sign it."""
import argparse
import plistlib
import re
import shutil
import struct
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--executable',type=Path,required=True)
parser.add_argument('--models',type=Path,required=True)
parser.add_argument('--icons',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True,help='New output directory; an existing app is never replaced.')
parser.add_argument('--server',default='https://api.productivitymon.com')
parser.add_argument('--version',default='0.1.0-alpha.mac.1')
args=parser.parse_args()
if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[A-Za-z0-9.]+)?',args.version):parser.error('--version must be a release version')
if not args.server.startswith(('https://','http://')):parser.error('--server must be an HTTP(S) URL')
with args.executable.open('rb') as executable:
    header=executable.read(8)
if len(header)!=8 or struct.unpack('<II',header)!=(0xfeedfacf,0x0100000c):
    parser.error('--executable must be an arm64 Mach-O binary built on macOS')
if not args.models.is_dir() or not args.icons.is_dir():parser.error('Prepared model and icon directories are required')
app=args.output.resolve()/'ProductivityMon.app'
if app.exists():parser.error(f'Refusing to overwrite existing app: {app}')
contents=app/'Contents'
(contents/'MacOS').mkdir(parents=True)
resources=contents/'Resources';resources.mkdir()
shutil.copy2(args.executable,contents/'MacOS/desk-mons')
(contents/'MacOS/desk-mons').chmod(0o755)
shutil.copytree(args.models,resources/'models')
shutil.copytree(args.icons,resources/'icons')
(resources/'server.txt').write_text(args.server+'\n',encoding='utf-8')
with (contents/'Info.plist').open('wb') as file:
    plistlib.dump({'CFBundleIdentifier':'com.productivitymon.desktop','CFBundleName':'ProductivityMon','CFBundleDisplayName':'ProductivityMon','CFBundleExecutable':'desk-mons','CFBundlePackageType':'APPL','CFBundleShortVersionString':args.version.split('-')[0],'CFBundleVersion':'1','LSMinimumSystemVersion':'12.0','LSUIElement':True,'NSHighResolutionCapable':True},file)
(resources/'release.json').write_text(__import__('json').dumps({'version':args.version,'channel':'development','architecture':'arm64','server':args.server},indent=2)+'\n',encoding='utf-8')
print(f'Created unsigned development bundle: {app}')
print('Native Mac smoke testing, signing and notarization are still required before distribution.')
