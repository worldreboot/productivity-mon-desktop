"""Check Metal and the signed-out launch without contacting the production server."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

repo=Path(__file__).resolve().parents[2]
report=repo/'mac-smoke.json'
probe=subprocess.run(['xcrun','swift','-e','import Metal; print(MTLCreateSystemDefaultDevice()?.name ?? "")'],capture_output=True,text=True,timeout=120,check=True)
device=probe.stdout.strip()
result={'metal_device':device or None,'launch':'not_run','reason':None}
if not device:
    result['reason']='This hosted runner exposes no Metal device. Interactive Mac validation is still required.'
    report.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(result['reason'])
else:
    subprocess.run(['cargo','test','--locked','--all-features','--manifest-path',str(repo/'desk/app/Cargo.toml'),'--target','aarch64-apple-darwin','straight_alpha_overlay_preserves_transparency_and_color','--','--ignored'],check=True,timeout=180)
    result['alpha_readback']='passed'
    with tempfile.TemporaryDirectory(prefix='productivitymon-mac-smoke-') as directory:
        home=Path(directory)
        # An isolated, signed-out profile and closed loopback endpoint prevent account creation or activity reports.
        process=subprocess.run([str(repo/'desk/app/target/aarch64-apple-darwin/release/desk-mons'),'--server','http://127.0.0.1:1','--seconds','5','--pick','panel'],env={**os.environ,'DESK_MONS_HOME':str(home),'RUST_BACKTRACE':'1'},capture_output=True,text=True,timeout=45)
        print(process.stdout)
        print(process.stderr)
        result['launch']='passed' if process.returncode==0 else 'failed'
        result['exit_code']=process.returncode
        report.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
        process.check_returncode()
        print('Signed-out native launch and timed exit passed on '+device)
