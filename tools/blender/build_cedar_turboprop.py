"""Build the original Cedar Utility Turboprop (experimental) exterior and actual CPU studio previews.

Blender 4.3.2:
  blender --background --disable-autoexec --threads 2 --python-exit-code 1 \
    --python tools/blender/build_cedar_turboprop.py
Optional arguments after --: ASSET_DIRECTORY PREVIEW_DIRECTORY [--no-render]

The asset is hand-authored procedural geometry in this file. No network, imported
mesh, texture, logo, blueprint or reference-aircraft dimensions are used.
All dimensions and styling are original provisional visual choices, not certified
aircraft data or a validated flight profile. The single shaft is body +X through
CG; the four-blade 2.4 m design disk has a static 20 degree pitch at 0.75 radius.
Neutral control surfaces do not claim flight trim. Shared mesh/export utilities
are adapted from this repository's original Meadow Trainer generator.
Geometry/material construction is deterministic; .blend container bytes
and render noise need not be identical across Blender versions/platforms.
License: MIT OR Apache-2.0, as the rest of this repository.
"""
import argparse
import json
import math
import struct
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('assets', nargs='?', type=Path, default=ROOT / 'assets/aircraft/experimental')
parser.add_argument('previews', nargs='?', type=Path, default=ROOT / 'docs/qa/images')
parser.add_argument('--no-render', action='store_true')
args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else [])
args.assets.mkdir(parents=True, exist_ok=True)
args.previews.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.context.preferences.filepaths.save_version = 0
scene = bpy.context.scene
scene.unit_settings.system = 'METRIC'
scene.unit_settings.scale_length = 1.0
aircraft_collection = bpy.data.collections.new('Cedar aircraft | export only')
scene.collection.children.link(aircraft_collection)

# Author dimensions as (forward, starboard, up), then map into right-handed
# Blender (-starboard, -forward, up). glTF export is (-starboard, up, forward):
# +Y up, +Z nose, -X starboard. ModelFit maps that to body (+forward,+right,+down).
def point(p):
    f, r, u = p
    return Vector((-r, -f, u))


def material(name, color, metallic=0.0, roughness=0.4):
    result = bpy.data.materials.new(name)
    result.diffuse_color = (*color, 1.0)
    result.use_nodes = True
    bsdf = result.node_tree.nodes.get('Principled BSDF')
    bsdf.inputs['Base Color'].default_value = (*color, 1.0)
    bsdf.inputs['Metallic'].default_value = metallic
    bsdf.inputs['Roughness'].default_value = roughness
    return result


cream = material('Warm white enamel', (.78, .81, .77), .08, .33)
teal = material('Cedar green enamel', (.035, .15, .105), .15, .29)
amber = material('Copper enamel', (.77, .255, .095), .10, .35)
glass = material('Opaque blue grey glazing', (.045, .095, .13), .45, .18)
rubber = material('Graphite rubber', (.018, .023, .025), 0, .83)
alloy = material('Satin alloy', (.48, .53, .55), .72, .29)
red = material('Port red light lens', (.65, .012, .018), .05, .23)
green = material('Starboard green light lens', (.015, .48, .10), .05, .23)


def finish(obj, name, mat, smooth=False):
    obj.name = name
    for old in list(obj.users_collection):
        old.objects.unlink(obj)
    aircraft_collection.objects.link(obj)
    obj.data.materials.append(mat)
    for face in obj.data.polygons:
        face.use_smooth = smooth
    return obj


def mesh(name, vertices, faces, mat, smooth=False):
    data = bpy.data.meshes.new(name)
    data.from_pydata([point(v) for v in vertices], [], faces)
    data.update()
    bm = bmesh.new()
    bm.from_mesh(data)
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    bm.to_mesh(data)
    bm.free()
    obj = bpy.data.objects.new(name, data)
    aircraft_collection.objects.link(obj)
    obj.data.materials.append(mat)
    for face in obj.data.polygons:
        face.use_smooth = smooth
    return obj


def loft(name, rings, mat, smooth=True):
    count = len(rings[0])
    faces = []
    for ring in range(len(rings) - 1):
        for i in range(count):
            faces.append((ring * count + i, ring * count + (i + 1) % count,
                          (ring + 1) * count + (i + 1) % count, (ring + 1) * count + i))
    faces.extend([tuple(reversed(range(count))),
                  tuple((len(rings) - 1) * count + i for i in range(count))])
    return mesh(name, [v for ring in rings for v in ring], faces, mat, smooth)


def tube(name, a, b, radius, mat, segments=16):
    a, b = point(a), point(b)
    bpy.ops.mesh.primitive_cylinder_add(vertices=segments, radius=radius,
                                      depth=(b-a).length, location=(a+b)/2)
    obj = bpy.context.object
    obj.rotation_euler = (b-a).to_track_quat('Z', 'Y').to_euler()
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    return finish(obj, name, mat, True)


def ellipsoid(name, center, size, mat, segments=16, rings=8):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=rings, location=point(center))
    obj = bpy.context.object
    f, r, u = size
    obj.scale = (r, f, u)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(obj, name, mat, True)


# Rounded utility cabin and long tapered engine cowl. Origin is the proposed CG.
# f/r/u = forward/starboard/up, metres. No certified/real-aircraft dimensions.
sections = [(-5.70,.035,.49,.36),(-5.18,.13,.59,.22),
            (-4.2,.27,.72,.05),(-3.15,.43,.91,-.24),
            (-2.35,.62,1.31,-.53),(-1.75,.70,1.43,-.58),
            (.55,.70,1.43,-.58),(1.42,.57,.74,-.48),
            (2.05,.47,.43,-.42),(2.95,.40,.35,-.34),
            (3.47,.255,.25,-.25),(3.59,.16,.16,-.16)]
fuselage_rings=[]
for forward,width,top,bottom in sections:
    ring=[]
    for i in range(24):
        angle=2*math.pi*i/24
        s,c=math.sin(angle),math.cos(angle)
        exponent=.55 if -2.35<=forward<=.55 else .85
        ring.append((forward,width*math.copysign(abs(c)**exponent,c),
                     (top+bottom)/2+(top-bottom)/2*math.copysign(abs(s)**exponent,s)))
    fuselage_rings.append(ring)
fuselage=loft('Cedar fuselage | proposed CG at origin',fuselage_rings,cream)
fuselage['status']='Original experimental presentation; no qualified flight profile'
fuselage['proposed_cg_body_m']=[0.,0.,0.]

def fuselage_section(forward):
    for left, right in zip(fuselage_rings, fuselage_rings[1:]):
        if left[0][0] <= forward <= right[0][0]:
            t = (forward-left[0][0])/(right[0][0]-left[0][0])
            return [tuple(a+(b-a)*t for a,b in zip(p,q)) for p,q in zip(left,right)]
    raise ValueError('glazing is outside fuselage stations')


def surface_coordinate(forward, known, known_axis, result_axis):
    section = fuselage_section(forward)
    hits = []
    for p,q in zip(section, section[1:]+section[:1]):
        if min(p[known_axis],q[known_axis]) <= known <= max(p[known_axis],q[known_axis]):
            delta=q[known_axis]-p[known_axis]
            if abs(delta)>1e-8:
                t=(known-p[known_axis])/delta
                hits.append(p[result_axis]+t*(q[result_axis]-p[result_axis]))
    if not hits:
        raise ValueError('glazing is outside the fuselage surface')
    return max(hits)


def glazing(name, corners, surface):
    # Subdivide the pane and project each vertex onto the authored loft. A flat
    # quad clips the rounded cabin and creates visibly broken window silhouettes.
    steps=5
    vertices=[]
    for j in range(steps+1):
        v=j/steps
        for i in range(steps+1):
            u=i/steps
            a=[(1-u)*x+u*y for x,y in zip(corners[0],corners[1])]
            b=[(1-u)*x+u*y for x,y in zip(corners[3],corners[2])]
            vertices.append(surface(*[(1-v)*x+v*y for x,y in zip(a,b)]))
    faces=[]
    for j in range(steps):
        for i in range(steps):
            p=j*(steps+1)+i
            faces.append((p,p+1,p+steps+2,p+steps+1))
    return mesh(name,vertices,faces,glass,True)


# Opaque, fitted exterior glazing: pilot window, windshield and two cargo panes.
# It makes no cockpit interior or through-glass visibility promise.
for side,label in [(-1,'Port'),(1,'Starboard')]:
    def side_surface(f,u):
        return (f,side*(surface_coordinate(f,u,2,1)+.012),u)
    glazing(label+' pilot side window',[(.48,.64),(-.46,.64),(-.46,1.26),(.48,1.25)],side_surface)
    glazing(label+' cabin window',[(-.58,.70),(-1.27,.70),(-1.27,1.22),(-.58,1.22)],side_surface)
    glazing(label+' cargo window',[(-1.42,.70),(-2.03,.69),(-2.03,1.15),(-1.42,1.21)],side_surface)
    def top_surface(f,r):
        return (f,side*r,surface_coordinate(f,r,1,2)+.012)
    glazing(label+' windshield',[(1.37,.035),(1.37,.45),(.61,.57),(.61,.035)],top_surface)
    # Thin belt follows the actual body section; no decals/textures.
    stripe=[]
    for f,w,top,bottom in sections[1:-1]:
        center=(top+bottom)/2
        for u in [center-.10,center+.015]:
            stripe.append((f,side*(surface_coordinate(f,u,2,1)+.008),u))
    mesh(label+' cedar belt',stripe,[(i*2,i*2+1,i*2+3,i*2+2) for i in range(len(sections)-3)],teal)

def wing_ring(distance, chord, center, up, side):
    result=[]
    for i in range(24):
        a=2*math.pi*i/24
        result.append((center+chord*.5*math.cos(a), side*distance,
                       up+.060*chord*math.sin(a)*(1+.24*math.cos(a))))
    return result


# 12.6 m high wing with modest taper and neutral, static flap/aileron seams.
for side,label in [(-1,'Port'),(1,'Starboard')]:
    loft(label+' high wing',[wing_ring(0,1.90,.0,1.55,side),
        wing_ring(4.65,1.80,-.02,1.64,side),wing_ring(5.95,1.58,-.09,1.68,side),
        wing_ring(6.30,1.32,-.15,1.69,side)],cream)
    loft(label+' copper wingtip',[wing_ring(5.96,1.579,-.092,1.681,side),
        wing_ring(6.30,1.326,-.15,1.69,side)],amber)
    tube(label+' wing strut',(-.28,side*.63,-.37),(.10,side*3.94,1.51),.041,teal,12)
    tube(label+' neutral flap hinge',(-.68,side*.85,1.625),(-.69,side*3.33,1.675),.006,teal,8)
    tube(label+' neutral aileron hinge',(-.68,side*3.44,1.677),(-.69,side*5.92,1.704),.006,teal,8)
    ellipsoid(label+' navigation lens',(-.15,side*6.275,1.70),(.053,.021,.032),red if side<0 else green,12,6)

# Generous conventional tail, with no control deflection baked into the mesh.
for side,label in [(-1,'Port'),(1,'Starboard')]:
    loft(label+' neutral tailplane',[wing_ring(.10,1.44,-4.53,.59,side),
        wing_ring(1.62,1.08,-4.78,.64,side),wing_ring(2.02,.65,-5.01,.66,side)],cream)
outline=[(-3.70,.57),(-5.63,.43),(-5.38,2.53),(-4.94,2.65),(-4.61,1.73)]
n=len(outline)
mesh('Cedar green vertical fin',[(f,side*.058,u) for side in [-1,1] for f,u in outline],
     [tuple(reversed(range(n))),tuple(range(n,2*n))]+
     [(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)],teal)
for side,label in [(-1,'Port'),(1,'Starboard')]:
    mesh(label+' copper fin flash',[(-5.32,side*.060,2.08),(-4.86,side*.060,2.12),
         (-4.99,side*.060,2.50),(-5.28,side*.060,2.42)],[(0,1,2,3)],amber)

# Three fixed wheels: authored proposal only, not copied from another FDM.
# Contact plane is body down=1.60 m: 0.40 m uncompressed level disk clearance.
# Suspension sag, braking pitch and uneven-ground clearance need later qualification.
for label,f,r in [('Nose',2.,0.),('Port main',-1.,-1.55),('Starboard main',-1.,1.55)]:
    tube(label+' fixed gear leg',(f*.86,r*.33,-.48),(f,r,-1.30),.046,alloy,12)
    rings=[]
    for lateral,radius in [(-.125,.24),(-.09,.287),(0,.30),(.09,.287),(.125,.24)]:
        rings.append([(f+radius*math.sin(2*math.pi*i/24),r+lateral,
                       -1.30+radius*math.cos(2*math.pi*i/24)) for i in range(24)])
    wheel=loft(label+' tyre',rings,rubber)
    wheel['proposed_contact_body_m']=[f,r,1.60]
    tube(label+' axle and hub',(f,r-.13,-1.30),(f,r+.13,-1.30),.105,alloy,16)

# Single shaft along body +X at right=up=0, through the declared CG.
# Four static blades, clockwise looking forward (+X) for positive RH body-X spin.
# Pitch beta is exactly 20 degrees at 0.75*1.2=.9 m. The mesh is a neutral
# presentation reference; it is not connected to runtime shaft/pitch state.
ellipsoid('Cedar copper spinner',(3.63,0,0),(.27,.215,.215),amber)
# Intake and exhaust are illustrative cowl shapes, not separate physical engines.
ellipsoid('Recessed engine intake',(2.94,0,-.315),(.37,.225,.125),rubber)
tube('Port exhaust', (2.40,-.37,-.22),(2.18,-.57,-.30),.075,alloy,12)
for k in range(4):
    angle=math.pi/4+k*math.pi/2
    radial=(math.sin(angle),math.cos(angle))
    tangent=(math.cos(angle),-math.sin(angle))
    rings=[]
    for radius,half_chord,half_thickness,pitch in [(.13,.075,.021,29),(.36,.125,.019,25),
            (.64,.125,.016,22),(.90,.100,.013,20),(1.13,.064,.009,17),(1.198,.020,.004,16)]:
        beta=math.radians(pitch)
        ring=[]
        for i in range(12):
            a=2*math.pi*i/12
            chord=half_chord*math.cos(a)
            thick=half_thickness*math.sin(a)
            tangential=chord*math.cos(beta)-thick*math.sin(beta)
            axial=chord*math.sin(beta)+thick*math.cos(beta)
            ring.append((3.52+axial,radial[0]*radius+tangent[0]*tangential,
                         radial[1]*radius+tangent[1]*tangential))
        rings.append(ring)
    blade=loft(f'Propeller blade {k+1} | static 20 deg at 75 percent',rings,rubber)
    blade['static_pitch_at_075_radius_rad']=math.radians(20)
    blade['design_disk_diameter_m']=2.4
    blade['shaft_axis_body']=[1.,0.,0.]
    blade['shaft_center_body_m']=[3.52,0.,0.]

# Bake aircraft transforms into mesh vertices, so the exported nodes use an
# identity transform, and the scene origin, not an AABB midpoint, remains CG.
bpy.ops.object.select_all(action='DESELECT')
for obj in aircraft_collection.objects:
    obj.select_set(True)
bpy.context.view_layer.objects.active=fuselage
bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
# Identity pivot on the shaft line. Named grouping only; no animation or live state.
propeller=bpy.data.objects.new('Cedar propeller assembly',None)
aircraft_collection.objects.link(propeller)
propeller['shaft_axis_body']=[1.,0.,0.]
propeller['shaft_axis_gltf']=[0.,0.,1.]
propeller['pivot_body_m']=[0.,0.,0.]
propeller['presentation']='Static reference only; future phase binding needs qualification'
for obj in list(aircraft_collection.objects):
    if obj.name.startswith('Propeller blade ') or obj.name=='Cedar copper spinner':
        obj.parent=propeller
propeller.select_set(True)
bpy.ops.export_scene.gltf(filepath=str(args.assets/'cedar_turboprop_experimental.glb'),
    export_format='GLB',use_selection=True,export_yup=True,export_extras=True,
    export_cameras=False,export_lights=False,export_animations=False,
    export_materials='EXPORT',export_texcoords=False)

# Blender can emit identical loop triangles in a different index order between
# processes (observed for sphere meshes in 4.3.2), changing buffer deduplication
# too. Canonicalize triangle order and repack the resulting accessors, preserving
# winding and every vertex/normal/material. No runtime parser is changed.
glb_path=args.assets/'cedar_turboprop_experimental.glb'
glb_bytes=glb_path.read_bytes()
json_size=struct.unpack_from('<I',glb_bytes,12)[0]
document=json.loads(glb_bytes[20:20+json_size])
old_binary=glb_bytes[28+json_size:]
new_binary=bytearray()
new_accessors=[]
new_views=[]
accessor_cache={}


def canonical_accessor(index, triangles=False):
    accessor=dict(document['accessors'][index])
    view=document['bufferViews'][accessor.pop('bufferView')]
    code={5121:'B',5123:'H',5125:'I',5126:'f'}[accessor['componentType']]
    components={'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4}[accessor['type']]
    element_size=struct.calcsize(code)*components
    start=view.get('byteOffset',0)+accessor.pop('byteOffset',0)
    assert not accessor.get('sparse')
    assert view.get('byteStride',element_size)==element_size
    payload=old_binary[start:start+element_size*accessor['count']]
    if triangles:
        assert accessor['type']=='SCALAR' and accessor['count']%3==0
        fmt='<'+code*accessor['count']
        indices=struct.unpack(fmt,payload)
        ordered=[]
        for i in range(0,len(indices),3):
            tri=indices[i:i+3]
            first=tri.index(min(tri))
            ordered.append(tri[first:]+tri[:first])
        ordered.sort()
        payload=struct.pack(fmt,*(v for tri in ordered for v in tri))
    target=view['target']
    key=(json.dumps(accessor,sort_keys=True),target,payload)
    if key not in accessor_cache:
        while len(new_binary)%4:
            new_binary.append(0)
        new_views.append({'buffer':0,'byteOffset':len(new_binary),
                          'byteLength':len(payload),'target':target})
        accessor['bufferView']=len(new_views)-1
        new_binary.extend(payload)
        accessor_cache[key]=len(new_accessors)
        new_accessors.append(accessor)
    return accessor_cache[key]


for exported_mesh in document['meshes']:
    for primitive in exported_mesh['primitives']:
        primitive['attributes']={name:canonical_accessor(index)
                                 for name,index in sorted(primitive['attributes'].items())}
        primitive['indices']=canonical_accessor(primitive['indices'],triangles=True)
document['accessors']=new_accessors
document['bufferViews']=new_views
document['buffers']=[{'byteLength':len(new_binary)}]
new_json=json.dumps(document,separators=(',',':')).encode('utf-8')
new_json+=b' '*((-len(new_json))%4)
new_binary+=b'\0'*((-len(new_binary))%4)
length=12+8+len(new_json)+8+len(new_binary)
glb_path.write_bytes(struct.pack('<4sII',b'glTF',2,length)
    +struct.pack('<II',len(new_json),0x4e4f534a)+new_json
    +struct.pack('<II',len(new_binary),0x004e4942)+new_binary)

# Studio is kept separate from the export collection in the editable .blend.
studio=bpy.data.collections.new('Studio | not exported')
scene.collection.children.link(studio)

def studio_object(obj,name):
    obj.name=name
    for old in list(obj.users_collection):
        old.objects.unlink(obj)
    studio.objects.link(obj)
    return obj

scene.render.engine='CYCLES'
scene.cycles.device='CPU'
scene.cycles.samples=32
scene.cycles.seed=0
scene.cycles.use_animated_seed=False
scene.cycles.use_denoising=False
scene.render.resolution_x=1200
scene.render.resolution_y=800
scene.render.resolution_percentage=100
scene.world=bpy.data.worlds.new('Neutral studio world')
scene.world.use_nodes=True
scene.world.node_tree.nodes['Background'].inputs[0].default_value=(.12,.15,.19,1)
scene.world.node_tree.nodes['Background'].inputs[1].default_value=.45
scene.view_settings.view_transform='AgX'
bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-1.604))
ground=studio_object(bpy.context.object,'Studio ground')
ground.data.materials.append(material('Studio slate',(.105,.135,.16),0,.8))
for name,position,energy,size in [('Key',(5,-7,10),1900,8),('Fill',(-6,-2,6),1200,7),('Rim',(0,8,8),1700,6)]:
    bpy.ops.object.light_add(type='AREA',location=position)
    lamp=studio_object(bpy.context.object,name)
    lamp.data.energy=energy
    lamp.data.shape='DISK'
    lamp.data.size=size
    lamp.rotation_euler=(Vector((0,0,.4))-lamp.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add()
camera=studio_object(bpy.context.object,'Cedar studio camera')
camera.data.type='ORTHO'
camera.data.ortho_scale=15.3
scene.camera=camera
scene.render.image_settings.file_format='JPEG'
scene.render.image_settings.quality=93

def view(forward,right,up,target=(-.7,0,.45)):
    camera.location=point((forward,right,up))
    camera.rotation_euler=(point(target)-camera.location).to_track_quat('-Z','Y').to_euler()

view(11,-15,8)
bpy.ops.object.select_all(action='DESELECT')
fuselage.select_set(True)
bpy.context.view_layer.objects.active=fuselage
# The .blend opens on the first view and preserves the export-only collection.
bpy.ops.wm.save_as_mainfile(filepath=str(args.assets/'cedar_turboprop_experimental.blend'),compress=True)
if not args.no_render:
    for suffix,location in [('three-quarter',(11,-15,8)),('starboard',(-10,15,6)),('front',(18,0,3.7))]:
        view(*location)
        scene.render.filepath=str(args.previews/f'cedar-turboprop-{suffix}.jpg')
        bpy.ops.render.render(write_still=True)
print('Cedar Utility Turboprop: exported original geometry and editable source'
      + ('; rendering skipped' if args.no_render else '; three studio renders complete'))
