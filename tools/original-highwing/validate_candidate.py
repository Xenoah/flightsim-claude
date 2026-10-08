#!/usr/bin/env python3
"""Independently check the candidate's geometry, preserved data and ModelFit.

Does not import the adapter or execute Rust. MIT OR Apache-2.0.
"""
import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import struct

HERE = Path(__file__).resolve().parent
SOURCE_HASH = 'c873b59a5e638c16bd2fb36c789704a41b1146ea90c7c44950257cbf5f806412'
PROFILE_HASH = '8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25'


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def sub(a, b):
    return tuple(x-y for x, y in zip(a, b))


def cross(a, b):
    return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def bounds(points):
    return tuple(min(p[i] for p in points) for i in range(3)), tuple(max(p[i] for p in points) for i in range(3))


def close(a, b, tolerance=2e-6):
    return len(a) == len(b) and all(abs(x-y) <= tolerance for x, y in zip(a, b))


def inside(point, triangles):
    # Three non-axis-aligned rays avoid accepting a single edge-hit coincidence.
    parities = []
    for direction in [(1, .173, .311), (.227, 1, .463), (.137, .071, 1)]:
        hits = []
        for a, b, c in triangles:
            e1, e2 = sub(b, a), sub(c, a)
            p = cross(direction, e2)
            det = dot(e1, p)
            if abs(det) < 1e-12:
                continue
            s = sub(point, a)
            u = dot(s, p)/det
            q = cross(s, e1)
            v, distance = dot(direction, q)/det, dot(e2, q)/det
            if u >= 0 and v >= 0 and u+v <= 1 and distance > 1e-8:
                if all(abs(distance-old) > 1e-7 for old in hits):
                    hits.append(distance)
        parities.append(len(hits) % 2 == 1)
    return all(parities)


class Glb:
    def __init__(self, raw):
        self.raw = raw
        require(len(raw) >= 28, 'Truncated header')
        require(struct.unpack_from('<4sII', raw) == (b'glTF', 2, len(raw)), 'Invalid GLB header')
        offset, chunks = 12, []
        while offset < len(raw):
            require(offset+8 <= len(raw), 'Truncated chunk header')
            size, kind = struct.unpack_from('<II', raw, offset)
            require(size % 4 == 0 and offset+8+size <= len(raw), 'Invalid chunk bounds/alignment')
            chunks.append((kind, raw[offset+8:offset+8+size]))
            offset += 8+size
        require(offset == len(raw) and [k for k, _ in chunks] == [0x4e4f534a, 0x004e4942], 'Expected JSON/BIN only')
        self.g = json.loads(chunks[0][1])
        self.bin = chunks[1][1]
        g = self.g
        require(g['asset']['version'] == '2.0', 'Wrong glTF version')
        require(len(g['buffers']) == 1 and 'uri' not in g['buffers'][0], 'Expected one embedded buffer')
        require(0 <= len(self.bin)-g['buffers'][0]['byteLength'] <= 3, 'Buffer length mismatch')
        for field in ['images', 'textures', 'animations', 'skins', 'cameras', 'extensionsUsed', 'extensionsRequired']:
            require(not g.get(field), 'Unexpected resource/extension: '+field)
        def walk(value):
            if isinstance(value, float):
                require(math.isfinite(value), 'Nonfinite JSON value')
            elif isinstance(value, dict):
                require('uri' not in value, 'Unexpected external resource URI')
                for item in value.values():
                    walk(item)
            elif isinstance(value, list):
                for item in value:
                    walk(item)
        walk(g)
        require(g['scene'] == 0 and len(g['scenes']) == 1, 'Unexpected scene')
        require(sorted(g['scenes'][0]['nodes']) == list(range(len(g['nodes']))), 'Nodes must be direct scene children')
        for node in g['nodes']:
            require(set(node) <= {'name', 'mesh', 'extras'}, 'Unexpected nonidentity node')
        for i in range(len(g['accessors'])):
            self.values(i)

    def layout(self, i):
        a = self.g['accessors'][i]
        require(not a.get('sparse') and not a.get('normalized'), 'Unsupported accessor encoding')
        view = self.g['bufferViews'][a['bufferView']]
        require(view['buffer'] == 0, 'Nonembedded buffer view')
        fmt = '<'+{5126:'f', 5125:'I', 5123:'H', 5121:'B'}[a['componentType']]*{'SCALAR':1, 'VEC2':2, 'VEC3':3, 'VEC4':4}[a['type']]
        size = struct.calcsize(fmt)
        stride = view.get('byteStride', size)
        start = view.get('byteOffset', 0)+a.get('byteOffset', 0)
        end = start+(a['count']-1)*stride+size
        require(a['count'] > 0 and stride >= size and end <= view.get('byteOffset', 0)+view['byteLength'] and end <= self.g['buffers'][0]['byteLength'], 'Accessor out of bounds')
        return a, fmt, start, stride, size

    def values(self, i):
        a, fmt, start, stride, _ = self.layout(i)
        values = [struct.unpack_from(fmt, self.bin, start+j*stride) for j in range(a['count'])]
        require(all(math.isfinite(x) for p in values for x in p), 'Nonfinite binary value')
        return values


def validate(candidate):
    source = Glb((HERE/'source/meadow_trainer.glb').read_bytes())
    require(sha(source.raw) == SOURCE_HASH, 'Original Meadow hash mismatch')
    target = Glb(candidate.read_bytes())
    require(len(target.raw) < 512*1024, 'Asset budget exceeded')
    profile_bytes = (HERE/'assets/aircraft/light_single.json').read_bytes()
    require(sha(profile_bytes) == PROFILE_HASH, 'Light Single profile is not byte-identical')
    require(profile_bytes == (HERE/'source/light_single.json').read_bytes(), 'Profile copy differs')
    profile = json.loads(profile_bytes)
    require(profile['model'] == {'path':'aircraft/light_single.glb', 'forward':'-x', 'up':'+y', 'length_m':8.3}, 'Unexpected intended ModelFit')
    meadow = json.loads((HERE/'source/meadow_trainer.json').read_text())
    dynamics = copy.deepcopy(meadow['dynamics'])
    dynamics['name'] = profile['dynamics']['name']
    require(dynamics == profile['dynamics'] and meadow['controls'] == profile['controls'] and meadow['camera_eye_m'] == profile['camera_eye_m'], 'Numeric profile contract mismatch')
    # Only POSITION/NORMAL binary bytes and their existing accessor bounds may differ.
    source_meta, target_meta = copy.deepcopy(source.g), copy.deepcopy(target.g)
    changed_accessors = set()
    for m in source.g['meshes']:
        for p in m['primitives']:
            changed_accessors.update(p['attributes'][name] for name in ['POSITION', 'NORMAL'])
    touched = set()
    for index in changed_accessors:
        for key in ['min', 'max']:
            require((key in source_meta['accessors'][index]) == (key in target_meta['accessors'][index]), 'Bounds field presence changed')
            source_meta['accessors'][index].pop(key, None)
            target_meta['accessors'][index].pop(key, None)
        a, _, start, stride, size = target.layout(index)
        for j in range(a['count']):
            touched.update(range(start+j*stride, start+j*stride+size))
    require(source_meta == target_meta, 'Metadata changed beyond geometric accessor bounds')
    require(len(source.bin) == len(target.bin), 'Buffer size changed')
    require(all(a == b for i, (a, b) in enumerate(zip(source.bin, target.bin)) if i not in touched), 'Index, UV or other binary data changed')
    points, fuselage, wheel_report = [], [], []
    vertex_count = triangle_count = 0
    max_body_difference = max_normal_difference = max_winding_difference = 0.0
    for node in target.g['nodes']:
        node_points = []
        for p in target.g['meshes'][node['mesh']]['primitives']:
            require(p.get('mode', 4) == 4 and 'targets' not in p, 'Unexpected primitive type')
            position_index, normal_index = p['attributes']['POSITION'], p['attributes']['NORMAL']
            old_positions, positions = source.values(position_index), target.values(position_index)
            old_normals, normals = source.values(normal_index), target.values(normal_index)
            require(len(normals) == len(positions), 'Normal count mismatch')
            for name, i in p['attributes'].items():
                require(len(target.values(i)) == len(positions), 'Attribute count mismatch: '+name)
            for old, new in zip(old_positions, positions):
                # Independently derive both body bases from profile forward x up:
                # Meadow: body=(z,-x,-y); Light Single: body=(-x,-z,-y).
                a, b = (old[2], -old[0], -old[1]), (-new[0], -new[2], -new[1])
                difference = max(abs(x-y) for x,y in zip(a,b))
                max_body_difference = max(max_body_difference, difference)
                require(difference == 0, 'Body-space geometry does not exactly match original')
            for old, new in zip(old_normals, normals):
                a, b = (old[2], -old[0], -old[1]), (-new[0], -new[2], -new[1])
                difference = max(abs(x-y) for x,y in zip(a,b))
                max_normal_difference = max(max_normal_difference, difference)
                require(difference == 0, 'Body-space normal changed')
                require(abs(dot(new,new)-1) < 2e-4, 'Nonunit normal')
            lo, hi = bounds(positions)
            accessor = target.g['accessors'][position_index]
            require(close(lo, accessor['min'], 0) and close(hi, accessor['max'], 0), 'Incorrect position bounds')
            indices = [v[0] for v in target.values(p['indices'])]
            require(len(indices)%3 == 0 and all(0 <= i < len(positions) for i in indices), 'Invalid triangle indices')
            for k in range(0, len(indices), 3):
                ids = indices[k:k+3]
                a,b,c = [positions[i] for i in ids]
                oa,ob,oc = [old_positions[i] for i in ids]
                normal = cross(sub(b,a), sub(c,a))
                old_normal = cross(sub(ob,oa), sub(oc,oa))
                require(dot(normal,normal) > 1e-16, 'Degenerate triangle')
                body_normal = (-normal[0], -normal[2], -normal[1])
                original_body_normal = (old_normal[2], -old_normal[0], -old_normal[1])
                difference = max(abs(x-y) for x,y in zip(body_normal, original_body_normal))
                max_winding_difference = max(max_winding_difference, difference)
                require(difference < 1e-12, 'Winding/face orientation changed')
                if node['name'] == 'Fuselage | CG at origin':
                    fuselage.append((a,b,c))
            vertex_count += len(positions)
            triangle_count += len(indices)//3
            points.extend(positions)
            node_points.extend(positions)
        if node['name'].endswith(' tyre'):
            lo,hi = bounds(node_points)
            contact = (-(lo[0]+hi[0])/2, -(lo[2]+hi[2])/2, -lo[1])
            expected = node['extras']['fdm_contact_body_m']
            require(close(contact, expected), 'Wrong wheel contact: '+node['name'])
            require(any(close(expected,g['contact_m']) for g in profile['dynamics']['landing_gear']), 'Contact not in unchanged profile')
            wheel_report.append({'mesh':node['name'], 'body_contact_before_fit_m':contact, 'expected_body_contact_m':expected})
    lo,hi = bounds(points)
    extent = sub(hi,lo)
    require(close(lo,(-3.25,-1,-5.5)) and close(hi,(5.05,2.22,5.5)), 'Wrong global bounds/orientation')
    scale = 8.3/extent[0]
    f32 = lambda x: struct.unpack('<f',struct.pack('<f',x))[0]
    runtime_scale = f32(f32(8.3)/f32(f32(hi[0])-f32(lo[0])))
    require(abs(scale-1) < 2e-6 and abs(runtime_scale-1) < 2e-6, 'Nonunity ModelFit scale')
    require(len(wheel_report)==3, 'Missing tyres')
    for row in wheel_report:
        row['body_contact_after_f32_fit_m'] = [f32(f32(v)*runtime_scale) for v in row['body_contact_before_fit_m']]
        require(close(row['body_contact_after_f32_fit_m'],row['expected_body_contact_m']), 'Fitted contact mismatch')
    eye = profile['camera_eye_m']
    eye_gltf = (-eye[0], -eye[2], -eye[1])
    require(inside(eye_gltf, fuselage) and inside((0,0,0), fuselage), 'Eye or CG outside fuselage')
    require(vertex_count == 3759 and triangle_count == 5756, 'Geometry counts changed')
    return {'status':'PASS: asset contract only; simulator integration not run',
        'candidate_sha256':sha(target.raw), 'candidate_bytes':len(target.raw), 'source_sha256':SOURCE_HASH,
        'unchanged_profile_sha256':PROFILE_HASH, 'meshes':len(target.g['meshes']), 'materials':len(target.g['materials']),
        'vertices':vertex_count, 'triangles':triangle_count, 'bounds_m':{'min':lo,'max':hi,'extents':extent},
        'model_fit':{'forward':'-x','up':'+y','length_m':8.3,'double_precision_scale':scale,'f32_scale':runtime_scale,'translation':[0,0,0]},
        'max_body_space_position_difference_m':max_body_difference, 'max_body_space_normal_difference':max_normal_difference,
        'max_body_space_triangle_cross_difference':max_winding_difference, 'wheel_contacts':wheel_report,
        'eye_gltf_m':eye_gltf, 'cg_and_eye_contained_three_ray_checks':True,
        'preserved':'indices, triangle winding, UV bytes, materials, nodes, extras, buffer layout; profile byte-identical',
        'embedded_only_finite_resources':True, 'no_new_dynamics_calibration':True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate',type=Path,default=HERE/'assets/aircraft/light_single.glb')
    args = parser.parse_args()
    print(json.dumps(validate(args.candidate),indent=2))
