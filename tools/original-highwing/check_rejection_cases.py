#!/usr/bin/env python3
"""Ensure candidate contract checks actually reject relevant faulty assets.

Fault fixtures are temporary and do not change any delivered GLB/profile.
MIT OR Apache-2.0.
"""
import json
from pathlib import Path
import struct
import tempfile

from validate_candidate import Glb, HERE, validate


def encode(g, binary):
    text = json.dumps(g,separators=(',',':')).encode()
    text += b' ' * (-len(text)%4)
    return struct.pack('<4sII',b'glTF',2,28+len(text)+len(binary))+struct.pack('<II',len(text),0x4e4f534a)+text+struct.pack('<II',len(binary),0x004e4942)+binary


original = (HERE/'source/meadow_trainer.glb').read_bytes()
candidate = (HERE/'assets/aircraft/light_single.glb').read_bytes()
source = Glb(original)
faults = {'renamed_unrotated_source':original}
broken = Glb(candidate)
binary = bytearray(broken.bin)
for mesh in broken.g['meshes']:
    for primitive in mesh['primitives']:
        i = primitive['attributes']['NORMAL']
        a,_,start,stride,size = broken.layout(i)
        for j in range(a['count']):
            offset = start+j*stride
            binary[offset:offset+size] = source.bin[offset:offset+size]
faults['positions_rotated_normals_not_rotated'] = encode(broken.g,binary)
broken = Glb(candidate)
binary = bytearray(broken.bin)
i = broken.g['meshes'][0]['primitives'][0]['indices']
a,fmt,start,stride,size = broken.layout(i)
binary[start:start+size],binary[start+stride:start+stride+size] = binary[start+stride:start+stride+size],binary[start:start+size]
faults['one_triangle_winding_reversed'] = encode(broken.g,binary)
broken = Glb(candidate)
broken.g['buffers'][0]['uri'] = 'unexpected.bin'
faults['external_buffer'] = encode(broken.g,broken.bin)
broken = Glb(candidate)
binary = bytearray(broken.bin)
i = broken.g['meshes'][0]['primitives'][0]['attributes']['POSITION']
_,_,start,_,_ = broken.layout(i)
struct.pack_into('<f',binary,start,float('nan'))
faults['nonfinite_position'] = encode(broken.g,binary)
broken = Glb(candidate)
broken.g['nodes'][0]['translation'] = [0.1,0,0]
faults['unintended_node_translation'] = encode(broken.g,broken.bin)
results = []
with tempfile.TemporaryDirectory(prefix='candidate-rejection-',dir=HERE/'evidence') as directory:
    path = Path(directory)/'fault.glb'
    for name,raw in faults.items():
        path.write_bytes(raw)
        try:
            validate(path)
        except ValueError as error:
            results.append({'case':name,'result':'correctly rejected','reason':str(error)})
        else:
            raise AssertionError('Validator accepted '+name)
print(json.dumps({'status':'PASS','cases':results},indent=2))
