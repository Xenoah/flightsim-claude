#!/usr/bin/env python3
"""Rebuild Kestrel's initial synthetic v2 profile after exporting its GLB.

All physics values are explicit authored assumptions for development acceptance.
The separate numerical fixture is read but never edited. Runtime table sampling
is piecewise trilinear, not continuous evaluation of the formula between knots.
This overwrites only assets/aircraft/kestrel_jet_trainer.json; do not run it over
subsequent manually calibrated values without deliberately reviewing the change.
License: MIT OR Apache-2.0.
"""
from pathlib import Path
import json,math,struct
root=Path(__file__).resolve().parents[2]
p=json.loads((root/'docs/examples/aircraft-profiles-v2/numerical-jet.json').read_text())
p['id']='kestrel-jet-trainer'
p['dynamics']['airframe']['name']='Kestrel Jet Trainer (fictional)'
p['model']={'path':'aircraft/kestrel_jet_trainer.glb','forward':'+z','up':'+y','length_m':8.5}
t=p['dynamics']['thrust']
t.update(pressure_ratios=[0.0,1.0,1.10],temperature_ratios=[.65,1.0,1.20],mach=[0.0,.35])
t['cells']=[{'idle_n':100.0*pressure*(1-.4*mach)/math.sqrt(temp),'maximum_dry_n':2500.0*pressure*(1-.12*mach)/math.sqrt(temp)}
 for pressure in t['pressure_ratios'] for temp in t['temperature_ratios'] for mach in t['mach']]
p['dynamics']['aero']['knots'][1]['mach']=.35
p['dynamics']['envelope']={'pressure_ratio':[.35,1.08],'temperature_ratio':[.65,1.20],'mach':[0.0,.35]}
raw=(root/'assets/aircraft/kestrel_jet_trainer.glb').read_bytes()
n=struct.unpack_from('<I',raw,12)[0];g=json.loads(raw[20:20+n])
positions=[g['accessors'][prim['attributes']['POSITION']] for mesh in g['meshes'] for prim in mesh['primitives']]
p['model']['length_m']=max(a['max'][2] for a in positions)-min(a['min'][2] for a in positions)
(root/'assets/aircraft/kestrel_jet_trainer.json').write_text(json.dumps(p,indent=2)+'\n')
print(p['model'])
