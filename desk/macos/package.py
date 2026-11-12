"""Package an already-built Apple Silicon executable and locally prepared assets; never publish or sign it."""
import argparse
import plistlib
import re
import shutil
import struct
import zipfile
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--executable',type=Path,required=True)
parser.add_argument('--models',type=Path,required=True)
parser.add_argument('--icons',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True,help='New output directory; an existing app is never replaced.')
parser.add_argument('--server',default='https://api.productivitymon.com')
parser.add_argument('--version',default='0.1.0-alpha.mac.1')
parser.add_argument('--channel',choices=['development','experimental-alpha'],default='development')
parser.add_argument('--build-number',type=int,default=1)
parser.add_argument('--archive',action='store_true',help='Also create a ZIP preserving Mac executable permissions.')
args=parser.parse_args()
if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[A-Za-z0-9.]+)?',args.version):parser.error('--version must be a release version')
if args.build_number < 1:parser.error('--build-number must be positive')
if not args.server.startswith(('https://','http://')):parser.error('--server must be an HTTP(S) URL')
with args.executable.open('rb') as executable:
    header=executable.read(8)
if len(header)!=8 or struct.unpack('<II',header)!=(0xfeedfacf,0x0100000c):
    parser.error('--executable must be an arm64 Mach-O binary built on macOS')
if not args.models.is_dir() or not args.icons.is_dir():parser.error('Prepared model and icon directories are required')
app=args.output.resolve()/'ProductivityMon.app'
if app.exists():parser.error(f'Refusing to overwrite existing app: {app}')
archive=args.output.resolve()/f'ProductivityMon-{args.version}-apple-silicon.zip'
if args.archive and archive.exists():parser.error(f'Refusing to overwrite existing archive: {archive}')
icon=(Path(__file__).resolve().parent/'../app/assets/productivitymon.png').read_bytes()
if icon[:8]!=b'\x89PNG\r\n\x1a\n' or struct.unpack('>II',icon[16:24])!=(256,256):
    parser.error('Bundled app icon must be a 256px PNG')
contents=app/'Contents'
(contents/'MacOS').mkdir(parents=True)
resources=contents/'Resources';resources.mkdir()
# Modern ICNS accepts a PNG payload; ic08 is the existing 256px app mark.
(resources/'ProductivityMon.icns').write_bytes(b'icns'+struct.pack('>I',len(icon)+16)+b'ic08'+struct.pack('>I',len(icon)+8)+icon)
shutil.copy2(args.executable,contents/'MacOS/desk-mons')
(contents/'MacOS/desk-mons').chmod(0o755)
shutil.copytree(args.models,resources/'models')
shutil.copytree(args.icons,resources/'icons')
(resources/'server.txt').write_text(args.server+'\n',encoding='utf-8')
with (contents/'Info.plist').open('wb') as file:
    plistlib.dump({'CFBundleIdentifier':'com.productivitymon.desktop','CFBundleName':'ProductivityMon','CFBundleDisplayName':'ProductivityMon','CFBundleExecutable':'desk-mons','CFBundleIconFile':'ProductivityMon.icns','CFBundlePackageType':'APPL','CFBundleShortVersionString':args.version.split('-')[0],'CFBundleVersion':str(args.build_number),'LSMinimumSystemVersion':'12.0','LSUIElement':True,'NSHighResolutionCapable':True},file)
(resources/'release.json').write_text(__import__('json').dumps({'version':args.version,'channel':args.channel,'architecture':'arm64','server':args.server},indent=2)+'\n',encoding='utf-8')
print(f'Created unsigned development bundle: {app}')
if args.archive:
    with zipfile.ZipFile(archive,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as zipped:
        for source in sorted(app.rglob('*')):
            if not source.is_file():continue
            entry=zipfile.ZipInfo(source.relative_to(args.output.resolve()).as_posix())
            entry.create_system=3
            mode=0o755 if source==contents/'MacOS/desk-mons' else 0o644
            entry.external_attr=(0o100000|mode)<<16
            entry.compress_type=zipfile.ZIP_DEFLATED
            with source.open('rb') as original,zipped.open(entry,'w',force_zip64=True) as target:
                shutil.copyfileobj(original,target)
    print(f'Created Mac archive: {archive}')
print('Native Mac smoke testing, signing and notarization are still required before distribution.')
