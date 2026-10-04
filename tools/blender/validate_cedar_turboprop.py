#!/usr/bin/env python3
"""Audit the experimental Cedar GLB. No Blender, app, profile or FDM dependency.

The declared dimensions are original visual design, not a certified aircraft.
Run from any directory: python tools/blender/validate_cedar_turboprop.py
License: MIT OR Apache-2.0.
"""
import hashlib
import json
import math
import struct
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
ASSETS=ROOT/'assets/aircraft/experimental'
BASELINE={'assets/aircraft/.gitattributes': 'b2b276e35044f3217231b6d4874589137762a45efad7d42c92cf967fe396740f', 'assets/aircraft/kestrel_jet_trainer.blend': 'f098deb4a7e66e4c46a74383d4c0bf6708f9f0c6638ff3c6b7788765c3707c3c', 'assets/aircraft/kestrel_jet_trainer.glb': '5a9d8747084f7ad6ff93dbadc80d87acbef42c6882030c0b88ca3aded16bef21', 'assets/aircraft/kestrel_jet_trainer.json': 'fab72bf40f23a70f22c3ee11b33aa36ae8b736fcfed7f4df11c2d053db749e1f', 'assets/aircraft/light_single.glb': '8fc91894ea3f54d4226c3a30545de1947fa8a9fb0e013bb4bfec0b5e298effd9', 'assets/aircraft/light_single.json': '8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25', 'assets/aircraft/meadow_trainer.blend': '5a11c5453d62a68add7706230832d2e289c6626331c066a7fd1f25b650dc37db', 'assets/aircraft/meadow_trainer.glb': 'c873b59a5e638c16bd2fb36c789704a41b1146ea90c7c44950257cbf5f806412', 'assets/aircraft/meadow_trainer.json': '8ce67e37e40ece15bdaba239a058632b11efd73755d8965ca014bbaeb39313c0', 'assets/aircraft/swift_sport.blend': '29a25578733be83d25ccff12c117b29124b57b5e69e8356bd8189d128e514e77', 'assets/aircraft/swift_sport.glb': '9f30f6f9babe87a54d0f1f5da104f719d7e99cb05aada2848f8ea88b0bf9e3b1', 'assets/aircraft/swift_sport.json': '319257c8363cf5b0d480b914796f43bbec8b7935adc32d095c706c86b8c3e663', 'tools/blender/build_kestrel_jet_profile.py': '82f6a51a9dabfd071c5fbcdf5aecd4f9cebea56acaa9bd9701deac753edfcff9', 'tools/blender/build_kestrel_jet_trainer.py': '3fba71d2b695c58c68c3e559f6a92518cbfadfe8376c9f7738f3c1c7c247e6a2', 'tools/blender/build_meadow_trainer.py': '3b7ed8696b0e2d4a656645ceca9b5acddfd3242047cee698f98605eb68d49a1d', 'tools/blender/build_swift_sport.py': '007b52fabcd342eaf7a63813315e98541b5e4411e6b72a46cb014fa9a19fffdf', 'tools/blender/validate_kestrel_jet_trainer.py': '7100a4442324fa46010915e66d269f8e17be94946676fcb4921f044989b08523', 'tools/blender/validate_meadow_trainer.py': '85dc707800b808d66c1bd24a65f6ca4cf56cf6394a317487fa943271379c6bd1', 'docs/release/asset-rights-manifest.json': 'bd6c47141551072924154207fc125053f5c4fc63779225344a13951a9f55dcf8', 'LICENSE-MIT': '43d5e9a1dc86439b7882faa044d442dbaf4a0ef1056b1387f24866e0c9dbeb43', 'LICENSE-APACHE': '8b87d8cfb6a827ee6fa7224b150b994dc2842fc69fdf07273f291abb6c4b21a9'}

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
        require(sha(ROOT/filename)==expected,f'Existing aircraft/source/policy modified: {filename}')
    model_path=ASSETS/'cedar_turboprop_experimental.glb'
    raw=model_path.read_bytes()
    require(len(raw)<=140864,'GLB exceeds Meadow Trainer byte budget')
    magic,version,length=struct.unpack_from('<4sII',raw)
    require((magic,version,length)==(b'glTF',2,len(raw)),'Invalid GLB header')
    offset=12
    chunks=[]
    while offset<len(raw):
        require(offset+8<=len(raw),'Incomplete chunk header')
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
    for field in ['images','textures','animations','skins','cameras','extensions','extensionsUsed','extensionsRequired']:
        require(not gltf.get(field),f'Unexpected resource/animation/extension: {field}')
    def audit_tree(value):
        if isinstance(value,dict):
            for key,item in value.items():
                require(key.lower() not in ['uri','url','script','scripts','camera','light','lights'],
                        f'Unexpected resource/script key: {key}')
                audit_tree(item)
        elif isinstance(value,list):
            for item in value:audit_tree(item)
        elif isinstance(value,str):
            require(not any(t in value.lower() for t in ['://','data:','javascript:','file:']),
                    'Unexpected resource URI or executable string')
    audit_tree(gltf)
    require(len(gltf['materials'])<=8 and len(gltf['meshes'])<=46,'Mesh/material budget exceeded')
    for material in gltf['materials']:
        require(material.get('alphaMode','OPAQUE')=='OPAQUE','This exterior uses opaque materials')
        pbr=material['pbrMetallicRoughness']
        require(set(pbr)<=set(['baseColorFactor','metallicFactor','roughnessFactor']),
                'Unexpected textured material')
    require(len(gltf['scenes'])==1 and gltf.get('scene')==0,'Unexpected scene count/index')
    assembly=[(i,node) for i,node in enumerate(gltf['nodes']) if node['name']=='Cedar propeller assembly']
    require(len(assembly)==1,'Missing stable propeller assembly')
    root_index,root=assembly[0]
    require(set(root)==set(['name','children','extras']),'Assembly must be an identity empty')
    children=root['children']
    require(len(children)==5 and len(set(children))==5,'Assembly must contain spinner and four blades')
    require({gltf['nodes'][i]['name'] for i in children}==
            {'Cedar copper spinner'}|{f'Propeller blade {i} | static 20 deg at 75 percent' for i in range(1,5)},
            'Unexpected propeller assembly child')
    require(sorted(gltf['scenes'][0]['nodes']+children)==list(range(len(gltf['nodes']))),
            'Scene tree must reach every node exactly once')
    require(root_index in gltf['scenes'][0]['nodes'],'Assembly must be a scene root')
    require(len(gltf['nodes'])==len(gltf['meshes'])+1,'Expected one node per mesh plus rotor root')
    require(root['extras']['pivot_body_m']==[0.,0.,0.] and root['extras']['shaft_axis_gltf']==[0.,0.,1.],
            'Assembly must pivot about shaft axis through CG')

    def accessor(index):
        require(0<=index<len(gltf['accessors']),'Invalid accessor reference')
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
        require(acc['count']>0 and stride>=size and start>=0 and end<=view.get('byteOffset',0)+view['byteLength']
                and end<=gltf['buffers'][0]['byteLength'],'Accessor exceeds binary view')
        values=[struct.unpack_from(fmt,binary,start+i*stride) for i in range(acc['count'])]
        require(all(math.isfinite(x) for v in values for x in v),'Nonfinite attribute')
        return values

    all_points=[]
    fuselage_triangles=[]
    wheel_evidence=[]
    rotor_evidence=[]
    lens_evidence=[]
    vertex_count=triangle_count=primitive_count=0
    expected_contacts={'Nose tyre':[2.,0.,1.60],'Port main tyre':[-1.,-1.55,1.60],
                       'Starboard main tyre':[-1.,1.55,1.60]}
    for node in gltf['nodes']:
        if node is root:continue
        require(set(node)<=set(['mesh','name','extras']),'Nonidentity transform/child in node')
        require(0<=node['mesh']<len(gltf['meshes']),'Invalid mesh reference')
        points=[]
        for primitive in gltf['meshes'][node['mesh']]['primitives']:
            primitive_count+=1
            require(primitive.get('mode',4)==4,'Nontriangle primitive')
            require(set(primitive['attributes'])<=set(['POSITION','NORMAL']),'Unexpected vertex attribute')
            require(0<=primitive['material']<len(gltf['materials']),'Invalid material reference')
            positions=accessor(primitive['attributes']['POSITION'])
            indices=[v[0] for v in accessor(primitive['indices'])]
            require(len(indices)%3==0 and all(0<=i<len(positions) for i in indices),'Invalid indices')
            for attribute,idx in primitive['attributes'].items():
                values=accessor(idx)
                require(len(values)==len(positions),'Attribute vertex count mismatch')
                if attribute=='NORMAL':
                    require(all(abs(sum(x*x for x in n)-1)<2e-4 for n in values),'Nonunit normal')
            for i in range(0,len(indices),3):
                a,b,c=[positions[k] for k in indices[i:i+3]]
                if node['name']=='Cedar fuselage | proposed CG at origin':fuselage_triangles.append((a,b,c))
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
        low,high=bounds(points)
        if node['name'] in expected_contacts:
            contact=[(low[2]+high[2])/2,-(low[0]+high[0])/2,-low[1]]
            expected=expected_contacts[node['name']]
            require(close(contact,expected),'Visual wheel contact mismatch')
            require(close(node['extras']['proposed_contact_body_m'],expected),'Unexpected contact metadata')
            wheel_evidence.append({'mesh':node['name'],'actual_body_contact_m':contact,'proposal_body_m':expected})
        if node['name'].startswith('Propeller blade '):
            blade_index=int(node['name'].split()[2])-1
            angle=math.pi/4+blade_index*math.pi/2
            # Independent measurement of the chord at 0.75*D/2=.9 m.
            cross_section=[]
            for x,y,z in points:
                forward,right,up=z,-x,y
                radial=right*math.sin(angle)+up*math.cos(angle)
                tangent=right*math.cos(angle)-up*math.sin(angle)
                if abs(radial-.9)<2e-6:cross_section.append((tangent,forward))
            require(len(cross_section)>=12,'Missing 75-percent blade station')
            a=min(cross_section);b=max(cross_section)
            measured_pitch=math.degrees(math.atan2(b[1]-a[1],b[0]-a[0]))
            require(abs(measured_pitch-20)<.002,'Static blade reference pitch changed')
            require(abs((a[1]+b[1])/2-3.52)<2e-6,'Propeller shaft axial center changed')
            radii=[math.hypot(x,y) for x,y,z in points]
            require(1.197<max(radii)<=1.2,'Blade must fit inside 2.4 m design disk')
            require(node['extras']['shaft_axis_body']==[1.,0.,0.],'Shaft must be body +X')
            require(node['extras']['shaft_center_body_m']==[3.52,0.,0.],'Shaft must pass through CG axis')
            rotor_evidence.append({'blade':blade_index+1,'measured_pitch_at_075_radius_deg':measured_pitch,
                                   'measured_outer_radius_m':max(radii)})
        if node['name'].endswith('navigation lens'):
            x=(low[0]+high[0])/2
            mat=gltf['materials'][gltf['meshes'][node['mesh']]['primitives'][0]['material']]
            color=mat['pbrMetallicRoughness']['baseColorFactor']
            if node['name'].startswith('Port'):
                require(x>0 and color[0]>color[1]*10,'Port/red side convention changed')
            else:require(x<0 and color[1]>color[0]*10,'Starboard/green side convention changed')
            lens_evidence.append({'mesh':node['name'],'gltf_x_m':x,'rgba':color})
    require(len(wheel_evidence)==3 and len(rotor_evidence)==4 and len(lens_evidence)==2,'Missing named components')
    require(inside_mesh([0.,0.,0.],fuselage_triangles),'Proposed CG must be inside fuselage')
    require(inside_mesh([.28,1.,.65],fuselage_triangles),'Proposed cockpit eye must be inside fuselage')
    minimum,maximum=bounds(all_points)
    require(close(minimum,[-6.3,-1.60,-5.7]) and close(maximum,[6.3,2.65,3.9]),'Overall bounds changed')
    extents=[b-a for a,b in zip(minimum,maximum)]
    require(abs((-minimum[1]-1.2)-.40)<2e-6,'Level uncompressed disk clearance changed')
    scale=9.6/extents[2]
    require(abs(scale-1)<2e-6,'Proposed ModelFit length must preserve contacts at unit scale')
    require(triangle_count<=5756 and vertex_count<=3759 and primitive_count<=46,'Geometry budget exceeded')
    previews=[ROOT/'docs/qa/images'/f'cedar-turboprop-{suffix}.jpg' for suffix in ['three-quarter','starboard','front']]
    require(all(p.is_file() for p in previews),'Missing required neutral studio preview')
    report={'asset':'Cedar Utility Turboprop (experimental)',
        'status':'visual asset contract PASS; no aircraft profile or flight qualification',
        'source_base_commit':'b38f41e94b6231037c54c538a5b99c33b19ca7dc',
        'glb_bytes':len(raw),'mesh_count':len(gltf['meshes']),'primitive_count':primitive_count,
        'material_count':len(gltf['materials']),'exported_vertices':vertex_count,'triangles':triangle_count,
        'gltf_bounds_m':{'min':minimum,'max':maximum,'extents':extents},
        'proposed_adapter':{'forward':'+z','up':'+y','length_m':9.6,'measured_scale':scale},
        'proposed_cg_body_m':[0.,0.,0.],'wheel_contacts':wheel_evidence,
        'proposed_camera_eye_body_m':[.65,-.28,-1.],
        'proposed_camera_eye_gltf_m':[.28,1.,.65],
        'proposed_eye_and_cg_inside_fuselage':True,
        'asset_to_body_mapping':'(glTF.z, -glTF.x, -glTF.y)',
        'stable_propeller_assembly_node':'Cedar propeller assembly',
        'propeller_assembly_pivot_body_m':[0.,0.,0.],
        'propeller_assembly_axis_gltf':[0.,0.,1.],
        'design_propeller_disk_diameter_m':2.4,'level_uncompressed_all_phase_design_disk_clearance_m':.40,
        'measured_contact_plane_clearance_m':-minimum[1]-1.2,
        'suspension_sag_pitch_and_uneven_ground_clearance_qualified':False,
        'static_reference_pitch_only':True,'rotor_measurements':rotor_evidence,
        'navigation_lenses':lens_evidence,'external_resources':0,'camera_light_animation_skin_count':0,
        'preview_sha256':{p.name:sha(p) for p in previews},'unchanged_baseline_sha256':BASELINE,
        'sha256':{p.name:sha(p) for p in [model_path,ASSETS/'cedar_turboprop_experimental.blend',
                   ROOT/'tools/blender/build_cedar_turboprop.py']}}
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
