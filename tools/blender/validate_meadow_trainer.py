#!/usr/bin/env python3
"""Validate the delivered Meadow Trainer asset without Blender/Cargo dependencies.

Run from any directory: python tools/blender/validate_meadow_trainer.py
Prints a JSON evidence report. This checks asset/profile contracts, not simulator
startup, rendering, flight handling or redistribution policy approval.
"""
import hashlib
import json
import math
import struct
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
ASSETS=ROOT/'assets/aircraft'
BASELINE={
    'light_single.json':'8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25',
    'light_single.glb':'8fc91894ea3f54d4226c3a30545de1947fa8a9fb0e013bb4bfec0b5e298effd9',
    'swift_sport.json':'319257c8363cf5b0d480b914796f43bbec8b7935adc32d095c706c86b8c3e663',
    'swift_sport.glb':'9f30f6f9babe87a54d0f1f5da104f719d7e99cb05aada2848f8ea88b0bf9e3b1',
    'swift_sport.blend':'29a25578733be83d25ccff12c117b29124b57b5e69e8356bd8189d128e514e77',
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def close(a,b,tolerance=2e-5):
    return len(a)==len(b) and all(abs(x-y)<=tolerance for x,y in zip(a,b))


def bounds(points):
    return ([min(p[i] for p in points) for i in range(3)],
            [max(p[i] for p in points) for i in range(3)])


def inside_mesh(point, triangles):
    """Odd/even ray check on the closed exported fuselage, in glTF coordinates."""
    direction=(.137,.071,1.)
    def sub(a,b):
        return [x-y for x,y in zip(a,b)]
    def dot(a,b):
        return sum(x*y for x,y in zip(a,b))
    def cross(a,b):
        return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
    hits=[]
    for a,b,c in triangles:
        edge1,edge2=sub(b,a),sub(c,a)
        p=cross(direction,edge2)
        determinant=dot(edge1,p)
        if abs(determinant)<1e-10:
            continue
        s=sub(point,a)
        u=dot(s,p)/determinant
        q=cross(s,edge1)
        v=dot(direction,q)/determinant
        distance=dot(edge2,q)/determinant
        if u>=0 and v>=0 and u+v<=1 and distance>1e-8:
            if all(abs(distance-old)>1e-7 for old in hits):
                hits.append(distance)
    return len(hits)%2==1


def main():
    for filename,expected in BASELINE.items():
        require(sha(ASSETS/filename)==expected,f'Existing aircraft modified: {filename}')
    profile=json.loads((ASSETS/'meadow_trainer.json').read_text())
    legacy=json.loads((ASSETS/'light_single.json').read_text())
    require(set(profile)==set(legacy),'Profile must use the current exact schema')
    require(profile['id']=='meadow-trainer','Unexpected profile identifier')
    require(profile['version']==legacy['version']==1,'Unexpected profile version')
    dynamics=dict(profile['dynamics'])
    dynamics['name']=legacy['dynamics']['name']
    require(dynamics==legacy['dynamics'],'Reused numeric dynamics changed')
    require(profile['controls']==legacy['controls'],'Reused controls changed')
    require(profile['camera_eye_m']==legacy['camera_eye_m']==[.6,-.25,-.9], 'Camera eye changed')
    require(profile['engine_sound']=='piston','Unexpected engine sound')
    require(profile['model']=={'path':'aircraft/meadow_trainer.glb','forward':'+z',
                              'up':'+y','length_m':8.3},'Unexpected ModelFit adapter')
    model_path=ROOT/'assets'/profile['model']['path']
    require(model_path==ASSETS/'meadow_trainer.glb' and model_path.is_file(),
            'Model path must resolve relative to the asset root')
    raw=model_path.read_bytes()
    require(len(raw)<=512*1024,'GLB budget exceeded (512 KiB)')
    magic,version,length=struct.unpack_from('<4sII',raw)
    require((magic,version,length)==(b'glTF',2,len(raw)),'Invalid GLB header')
    offset=12
    chunks=[]
    while offset<len(raw):
        size,kind=struct.unpack_from('<II',raw,offset)
        require(size%4==0 and offset+8+size<=len(raw),'Invalid GLB chunk')
        chunks.append((kind,raw[offset+8:offset+8+size]))
        offset+=8+size
    require(offset==len(raw) and [c[0] for c in chunks]==[0x4e4f534a,0x004e4942],
            'Expected exactly embedded JSON and binary chunks')
    gltf=json.loads(chunks[0][1])
    binary=chunks[1][1]
    require(gltf['asset']['version']=='2.0','Unexpected glTF version')
    require(len(gltf['buffers'])==1 and 'uri' not in gltf['buffers'][0],
            'External buffer or multiple buffers')
    require(0<=len(binary)-gltf['buffers'][0]['byteLength']<=3,'Binary length mismatch')
    for field in ['images','textures','animations','skins','cameras']:
        require(not gltf.get(field),f'Unexpected external-resource/animation field: {field}')
    require(not gltf.get('extensionsRequired'),'Unexpected required glTF extension')
    require(len(gltf['materials'])<=8 and len(gltf['meshes'])<=48,'Mesh/material budget exceeded')
    require(len(gltf['scenes'])==1,'Unexpected scene count')
    require(sorted(gltf['scenes'][0]['nodes'])==list(range(len(gltf['nodes']))),
            'All aircraft nodes must be direct children of the CG scene root')

    def accessor(index):
        acc=gltf['accessors'][index]
        require('sparse' not in acc and not acc.get('normalized'),'Unsupported accessor encoding')
        view=gltf['bufferViews'][acc['bufferView']]
        require(view['buffer']==0,'Accessor does not use embedded buffer')
        kind={5126:'f',5125:'I',5123:'H',5121:'B'}[acc['componentType']]
        dimensions={'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4}[acc['type']]
        fmt='<'+kind*dimensions
        size=struct.calcsize(fmt)
        stride=view.get('byteStride',size)
        start=view.get('byteOffset',0)+acc.get('byteOffset',0)
        end=start+stride*(acc['count']-1)+size
        require(acc['count']>0 and stride>=size and end<=view.get('byteOffset',0)+view['byteLength']
                and end<=gltf['buffers'][0]['byteLength'],'Accessor exceeds its binary view')
        values=[struct.unpack_from(fmt,binary,start+i*stride) for i in range(acc['count'])]
        require(all(math.isfinite(x) for v in values for x in v),'Nonfinite attribute')
        return values

    all_points=[]
    fuselage_triangles=[]
    wheel_evidence=[]
    vertex_count=triangle_count=primitive_count=0
    for node in gltf['nodes']:
        require(set(node)<=set(['mesh','name','extras']),'Nonidentity transform/child in aircraft node')
        points=[]
        for primitive in gltf['meshes'][node['mesh']]['primitives']:
            primitive_count+=1
            require(primitive.get('mode',4)==4,'Nontriangle primitive')
            positions=accessor(primitive['attributes']['POSITION'])
            indices=[v[0] for v in accessor(primitive['indices'])]
            require(len(indices)%3==0 and all(i<len(positions) for i in indices),'Invalid indices')
            for attribute,idx in primitive['attributes'].items():
                values=accessor(idx)
                require(len(values)==len(positions),'Attribute vertex count mismatch')
                if attribute=='NORMAL':
                    require(all(abs(sum(x*x for x in n)-1)<2e-4 for n in values),'Nonunit normal')
            for i in range(0,len(indices),3):
                a,b,c=[positions[k] for k in indices[i:i+3]]
                if node['name']=='Fuselage | CG at origin':
                    fuselage_triangles.append((a,b,c))
                ab=[b[j]-a[j] for j in range(3)]
                ac=[c[j]-a[j] for j in range(3)]
                cross=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]]
                require(sum(x*x for x in cross)>1e-16,'Degenerate triangle')
            minimum,maximum=bounds(positions)
            acc=gltf['accessors'][primitive['attributes']['POSITION']]
            require(close(minimum,acc['min']) and close(maximum,acc['max']),'Accessor bounds mismatch')
            vertex_count+=len(positions)
            triangle_count+=len(indices)//3
            points.extend(positions)
        all_points.extend(points)
        if node['name'].endswith(' tyre'):
            low,high=bounds(points)
            # +Z forward/+Y up means body = (glTF.z, -glTF.x, -glTF.y).
            contact=[(low[2]+high[2])/2,-(low[0]+high[0])/2,-low[1]]
            expected=node['extras']['fdm_contact_body_m']
            require(close(contact,expected),f'Wheel contact mismatch: {node["name"]}')
            require(any(close(expected,gear['contact_m']) for gear in profile['dynamics']['landing_gear']),
                    'Wheel contact does not match profile physics')
            wheel_evidence.append({'mesh':node['name'],'actual_body_contact_m':contact,'expected_body_contact_m':expected})
    require(len(wheel_evidence)==3,'Expected three named tyres')
    require(inside_mesh([.25,.9,.6],fuselage_triangles),'Cockpit eye must lie within the cabin shell')
    require(inside_mesh([0.,0.,0.],fuselage_triangles),'Intended CG must lie within the fuselage shell')
    minimum,maximum=bounds(all_points)
    require(close(minimum,[-5.5,-1.,-5.05]) and close(maximum,[5.5,2.22,3.25]),'Overall bounds changed')
    extents=[b-a for a,b in zip(minimum,maximum)]
    scale=profile['model']['length_m']/extents[2]
    require(abs(scale-1)<2e-6,'ModelFit must preserve gear contacts with unit scale')
    require(triangle_count<=7000 and vertex_count<=6000,'Geometry budget exceeded')
    report={'asset':'Meadow Trainer','status':'asset contract PASS; FlightSim checks not run by this script',
        'glb_bytes':len(raw),'mesh_count':len(gltf['meshes']),'primitive_count':primitive_count,
        'material_count':len(gltf['materials']),'exported_vertices':vertex_count,'triangles':triangle_count,
        'gltf_bounds_m':{'min':minimum,'max':maximum,'extents':extents},'model_fit_scale':scale,
        'wheel_contacts':wheel_evidence,'camera_eye_body_m':profile['camera_eye_m'],
        'camera_eye_gltf_m':[.25,.9,.6],
        'camera_eye_and_cg_inside_fuselage':True,
        'preview_sha256':{f'meadow-trainer-{suffix}.jpg':sha(ROOT/'docs/qa/images'/f'meadow-trainer-{suffix}.jpg')
                          for suffix in ['three-quarter','starboard','front']},
        'unchanged_baseline_sha256':BASELINE,'sha256':{p.name:sha(p) for p in [
            ASSETS/'meadow_trainer.glb',ASSETS/'meadow_trainer.blend',ASSETS/'meadow_trainer.json',
            ROOT/'tools/blender/build_meadow_trainer.py']}}
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    main()
