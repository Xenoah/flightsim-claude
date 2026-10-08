#!/usr/bin/env python3
"""Validate the delivered Kestrel Jet Trainer asset without Blender/Cargo dependencies.

Run from any directory: python tools/blender/validate_kestrel_jet_trainer.py
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
CREATION_BASELINE = {'assets/aircraft/.gitattributes': 'b2b276e35044f3217231b6d4874589137762a45efad7d42c92cf967fe396740f',
 'assets/aircraft/light_single.glb': '8fc91894ea3f54d4226c3a30545de1947fa8a9fb0e013bb4bfec0b5e298effd9',
 'assets/aircraft/light_single.json': '8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25',
 'assets/aircraft/meadow_trainer.blend': '5a11c5453d62a68add7706230832d2e289c6626331c066a7fd1f25b650dc37db',
 'assets/aircraft/meadow_trainer.glb': 'c873b59a5e638c16bd2fb36c789704a41b1146ea90c7c44950257cbf5f806412',
 'assets/aircraft/meadow_trainer.json': '8ce67e37e40ece15bdaba239a058632b11efd73755d8965ca014bbaeb39313c0',
 'assets/aircraft/swift_sport.blend': '29a25578733be83d25ccff12c117b29124b57b5e69e8356bd8189d128e514e77',
 'assets/aircraft/swift_sport.glb': '9f30f6f9babe87a54d0f1f5da104f719d7e99cb05aada2848f8ea88b0bf9e3b1',
 'assets/aircraft/swift_sport.json': '319257c8363cf5b0d480b914796f43bbec8b7935adc32d095c706c86b8c3e663',
 'docs/examples/aircraft-profiles-v2/numerical-jet.json': '8339b68183ea31c228082e56984a39775f7f23df5633dd8a333fe06a65be7c74'}

# Creation-time hashes above are historical evidence. These exact current
# replacements are reviewed separately; no arbitrary alternate bytes are allowed.
# See docs/release/full-two-aircraft-source-candidate.md.
CURRENT_REPLACEMENTS = {'assets/aircraft/light_single.glb': 'b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc'}
BASELINE = {**CREATION_BASELINE, **CURRENT_REPLACEMENTS}


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


def dot(a,b): return sum(x*y for x,y in zip(a,b))
def sub(a,b): return [x-y for x,y in zip(a,b)]
def cross(a,b): return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
def norm(v): return math.sqrt(dot(v,v))


def ray_hit(origin,direction,tri):
    a,b,c=tri
    edge1,edge2=sub(b,a),sub(c,a)
    p=cross(direction,edge2); determinant=dot(edge1,p)
    if abs(determinant)<1e-10: return None
    s=sub(origin,a); u=dot(s,p)/determinant
    q=cross(s,edge1); v=dot(direction,q)/determinant
    distance=dot(edge2,q)/determinant
    return distance if u>=0 and v>=0 and u+v<=1 and distance>1e-8 else None


def point_triangle_distance(p,tri):
    # Orthogonal projection inside a triangle, otherwise nearest finite edge.
    a,b,c=tri; normal=cross(sub(b,a),sub(c,a)); n2=dot(normal,normal)
    signed=dot(sub(p,a),normal)/n2
    projected=[p[i]-signed*normal[i] for i in range(3)]
    edge_signs=[dot(cross(sub(y,x),sub(projected,x)),normal) for x,y in [(a,b),(b,c),(c,a)]]
    if min(edge_signs)>=-1e-12 or max(edge_signs)<=1e-12:
        return abs(signed)*math.sqrt(n2)
    distances=[]
    for x,y in [(a,b),(b,c),(c,a)]:
        edge=sub(y,x); t=max(0,min(1,dot(sub(p,x),edge)/dot(edge,edge)))
        distances.append(norm(sub(p,[x[i]+t*edge[i] for i in range(3)])))
    return min(distances)


def main():
    for filename,expected in BASELINE.items():
        require(sha(ROOT/filename)==expected,f'Existing asset/fixture modified: {filename}')
    profile=json.loads((ASSETS/'kestrel_jet_trainer.json').read_text())
    fixture=json.loads((ROOT/'docs/examples/aircraft-profiles-v2/numerical-jet.json').read_text())
    import jsonschema
    jsonschema.Draft202012Validator(json.loads((ROOT/'schemas/aircraft-profile-v2.schema.json').read_text())).validate(profile)
    require(profile['id']=='kestrel-jet-trainer' and profile['version']==2,'Unexpected identity/version')
    require(profile['dynamics']['kind']=='dry_jet_table' and profile['dynamics']['revision']==1,'Unexpected physical law')
    airframe=dict(profile['dynamics']['airframe'])
    airframe['name']=fixture['dynamics']['airframe']['name']
    require(airframe==fixture['dynamics']['airframe'],'Initial airframe values changed')
    require(profile['camera_eye_m']==[.6,-.25,-.9], 'Unexpected cockpit eye')
    require(profile['engine_sound']=='turbine','Unexpected sound family')
    require(profile['model']=={'path':'aircraft/kestrel_jet_trainer.glb','forward':'+z',
                              'up':'+y','length_m':8.5},'Unexpected ModelFit adapter')
    thrust=profile['dynamics']['thrust']
    require(thrust['pressure_ratios']==[0.0,1.0,1.10] and
            thrust['temperature_ratios']==[.65,1.0,1.20] and thrust['mach']==[0.0,.35],
            'Synthetic table reference knots changed')
    expected_cells=[{'idle_n':100*p*(1-.4*m)/math.sqrt(t),'maximum_dry_n':2500*p*(1-.12*m)/math.sqrt(t)}
       for p in thrust['pressure_ratios'] for t in thrust['temperature_ratios'] for m in thrust['mach']]
    require(thrust['cells']==expected_cells,'Authored thrust formula differs at a table knot')
    require(thrust['cells'][8]=={'idle_n':100.0,'maximum_dry_n':2500.0},'Sea-level static anchor lost')
    require(profile['dynamics']['envelope']=={'pressure_ratio':[.35,1.08],
             'temperature_ratio':[.65,1.20],'mach':[0.0,.35]},'Authored operating box changed')
    for knot,mach in zip(profile['dynamics']['aero']['knots'],[0.0,.35]):
        require(knot['mach']==mach and knot['aero']==fixture['dynamics']['aero']['knots'][0]['aero'],
                'Initial inherited low-speed aero coefficients changed')
    model_path=ROOT/'assets'/profile['model']['path']
    require(model_path==ASSETS/'kestrel_jet_trainer.glb' and model_path.is_file(),
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
    require(len(gltf['materials'])<=8 and len(gltf['meshes'])<=65,'Mesh/material budget exceeded')
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
    canopy_triangles=[]
    opaque_triangles=[]
    navigation=[]
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
                if node['name']=='Fuselage | physical CG origin':
                    fuselage_triangles.append((a,b,c))
                if node['name']=='Canopy | clear bubble shell': canopy_triangles.append((a,b,c))
                if gltf['materials'][primitive['material']].get('alphaMode','OPAQUE')=='OPAQUE':
                    opaque_triangles.append((a,b,c))
                ab=[b[j]-a[j] for j in range(3)]
                ac=[c[j]-a[j] for j in range(3)]
                triangle_cross=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]]
                require(sum(x*x for x in triangle_cross)>1e-16,'Degenerate triangle')
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
            require(any(close(expected,gear['contact_m']) for gear in profile['dynamics']['airframe']['landing_gear']),
                    'Wheel contact does not match profile physics')
            wheel_evidence.append({'mesh':node['name'],'actual_body_contact_m':contact,'expected_body_contact_m':expected})
        if 'navigation_side' in node.get('extras',{}):
            low,high=bounds(points)
            pos=[(a+b)/2 for a,b in zip(low,high)]
            primitive=gltf['meshes'][node['mesh']]['primitives'][0]
            color=gltf['materials'][primitive['material']]['pbrMetallicRoughness']['baseColorFactor']
            port=node['extras']['navigation_side']=='port_red'
            require(pos[0]>0 if port else pos[0]<0,'Navigation side sign inverted')
            require(color[0]>color[1]*3 if port else color[1]>color[0]*3,'Navigation lens color inverted')
            navigation.append({'side':node['extras']['navigation_side'],'gltf_position_m':pos,'rgba':color})
    require(len(wheel_evidence)==3 and len(navigation)==2,'Expected three wheels and two nav lenses')
    eye=[-profile['camera_eye_m'][1],-profile['camera_eye_m'][2],profile['camera_eye_m'][0]]
    require(inside_mesh(eye,canopy_triangles),'Cockpit eye is outside canopy shell')
    require(inside_mesh([0.,0.,0.],fuselage_triangles),'Physical CG is outside fuselage shell')
    clearance=min(point_triangle_distance(eye,tri) for tri in opaque_triangles)
    require(clearance>.18,'Opaque geometry intersects eye clearance sphere')
    sightlines=[]
    for direction in [(0,0,1),(.5,0,1),(-.5,0,1),(0,.18,1),(0,-.18,1)]:
        hits=[distance for tri in opaque_triangles if (distance:=ray_hit(eye,direction,tri)) is not None]
        require(not hits,'Opaque geometry blocks the forward pilot sightlines')
        sightlines.append({'gltf_direction':direction,'opaque_hits':len(hits)})
    minimum,maximum=bounds(all_points); extents=[b-a for a,b in zip(minimum,maximum)]
    require(close(minimum,[-5.5,-1.,-4.85]) and close(maximum,[5.5,1.86,3.65]),'Overall bounds changed')
    scale=profile['model']['length_m']/extents[2]
    require(scale==1.0,'ModelFit must preserve exact unit scale')
    require(triangle_count<=9000 and vertex_count<=7000,'Geometry budget exceeded')
    # Coordinate-contract evidence under arbitrary attitudes and large ECEF rebases.
    # No runtime transform implementation is used: Rodrigues rotations provide
    # independent rigid transforms of the inspected tyre contact points.
    transform_errors=[]
    for axis,angle in [((1,2,3),.71),((-2,1,.3),-1.12),((.4,-.7,1),2.64)]:
        axis=[x/norm(axis) for x in axis]; c,s=math.cos(angle),math.sin(angle)
        def rotate(v):
            cv=cross(axis,v)
            return [v[i]*c+cv[i]*s+axis[i]*dot(axis,v)*(1-c) for i in range(3)]
        aircraft_ecef=[3900000.25,4010000.75,3010000.50]
        origins=[[3900000,4010000,3010000],[3903100,4009200,3011200]]
        for wheel in wheel_evidence:
            actual=rotate(wheel['actual_body_contact_m']); expected=rotate(wheel['expected_body_contact_m'])
            for origin in origins:
                # f64 subtraction precedes f32 render storage in both paths.
                def local(v):
                    return [struct.unpack('<f',struct.pack('<f',aircraft_ecef[i]+v[i]-origin[i]))[0] for i in range(3)]
                error=norm(sub(local(actual),local(expected)))
                require(error<.001,'Contact transform/rebase error exceeds one millimetre')
                transform_errors.append(error)
    previews={f'kestrel-jet-trainer-{suffix}.jpg':sha(ROOT/'docs/qa/images'/f'kestrel-jet-trainer-{suffix}.jpg')
                          for suffix in ['three-quarter','starboard','front','eye']}
    report={'asset':'Kestrel Jet Trainer','status':'asset/schema contract PASS; runtime/native handling require separate acceptance',
        'glb_bytes':len(raw),'mesh_count':len(gltf['meshes']),'primitive_count':primitive_count,
        'material_count':len(gltf['materials']),'exported_vertices':vertex_count,'triangles':triangle_count,
        'gltf_bounds_m':{'min':minimum,'max':maximum,'extents':extents},'model_fit_scale':scale,
        'wheel_contacts':wheel_evidence,'navigation_lenses':navigation,'camera_eye_body_m':profile['camera_eye_m'],
        'camera_eye_gltf_m':eye,'minimum_eye_to_opaque_triangle_m':clearance,
        'eye_inside_canopy_and_cg_inside_fuselage':True,'forward_sightlines':sightlines,
        'arbitrary_attitude_rebase_contact_checks':len(transform_errors),
        'maximum_arbitrary_attitude_rebase_contact_error_m':max(transform_errors),
        'sea_level_static_thrust_n':thrust['cells'][8],
        'controls_match_initial_numerical_fixture':profile['controls']==fixture['controls'],
        'preview_sha256':previews,'historical_creation_baseline_sha256':CREATION_BASELINE,'current_expected_sha256':BASELINE,
        'sha256':{p.name:sha(p) for p in [ASSETS/'kestrel_jet_trainer.glb',
           ASSETS/'kestrel_jet_trainer.blend',ASSETS/'kestrel_jet_trainer.json',
           ROOT/'tools/blender/build_kestrel_jet_trainer.py']}}
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    main()
