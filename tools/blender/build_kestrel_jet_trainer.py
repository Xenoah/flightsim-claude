"""Build the original Kestrel Jet Trainer exterior and actual CPU studio previews.

Blender 4.3.2:
  blender --background --threads 2 --python tools/blender/build_kestrel_jet_trainer.py
Optional arguments after --: ASSET_DIRECTORY PREVIEW_DIRECTORY [--no-render]

The asset is hand-authored procedural geometry in this file. No network, imported
mesh, texture, logo, blueprint or reference-aircraft dimensions are used. Numeric
gear contacts, span and camera location initially match this project's synthetic
dry-jet numerical fixture. All visual design is newly authored and fictional.
Geometry/material construction is deterministic; .blend container bytes and
render noise need not be identical across Blender versions/platforms.
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
parser.add_argument('assets', nargs='?', type=Path, default=ROOT / 'assets/aircraft')
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
aircraft_collection = bpy.data.collections.new('Kestrel Jet Trainer | export only this collection')
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


cream = material('Porcelain enamel', (.84, .87, .88), .08, .31)
teal = material('Midnight navy enamel', (.018, .055, .105), .15, .28)
amber = material('Copper orange enamel', (.84, .21, .065), .18, .31)
glass = material('Transparent blue canopy', (.12, .29, .39), .12, .16)
glass.diffuse_color = (.12, .29, .39, .19)
glass.node_tree.nodes.get('Principled BSDF').inputs['Alpha'].default_value = .19
glass.surface_render_method = 'DITHERED'
glass.use_backface_culling = True
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


def ellipsoid(name, center, size, mat, segments=20, rings=10):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=rings, location=point(center))
    obj = bpy.context.object
    f, r, u = size
    obj.scale = (r, f, u)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(obj, name, mat, True)


# Body coordinates below use forward/right/up; point() maps them to Blender.
# The visual design is fictional. Mass/inertia/aero are not calculated from it.
sections=[(-4.55,.225,.29,-.25),(-4.15,.29,.37,-.30),
          (-3.40,.37,.45,-.36),(-2.30,.46,.49,-.42),
          (-1.25,.56,.49,-.44),(-.35,.64,.47,-.45),
          (.70,.63,.44,-.43),(1.70,.49,.38,-.37),
          (2.70,.29,.23,-.24),(3.35,.12,.10,-.12),(3.65,.014,.01,-.025)]
fuselage_rings=[]
for f,width,top,bottom in sections:
    fuselage_rings.append([(f,width*math.cos(2*math.pi*i/32),
        (top+bottom)/2+(top-bottom)/2*math.sin(2*math.pi*i/32)) for i in range(32)])
fuselage=loft('Fuselage | physical CG origin',fuselage_rings,cream)
fuselage['origin_semantics']='Physical CG; no bounding-box recentering'

# A transparent closed bubble above the opaque shoulder. The eye lies inside
# this shell with a forward sightline; seats/panel are deliberately below it.
canopy_sections=[(-1.36,.035,.56,.39),(-1.10,.41,1.02,.40),
                 (-.50,.54,1.31,.41),(.65,.53,1.32,.40),
                 (1.28,.40,1.00,.38),(1.77,.04,.43,.36)]
canopy_rings=[]
for f,w,top,bottom in canopy_sections:
    canopy_rings.append([(f,w*math.cos(2*math.pi*i/32),
         (top+bottom)/2+(top-bottom)/2*math.sin(2*math.pi*i/32)) for i in range(32)])
canopy=loft('Canopy | clear bubble shell',canopy_rings,glass)
canopy['eye_body_m']=[.6,-.25,-.9]
# Rails and single aft hoop leave the forward field unobstructed.
for side,label in [(-1,'Port'),(1,'Starboard')]:
    for a,b in zip(canopy_sections,canopy_sections[1:]):
        tube(label+' canopy rail', (a[0],side*a[1],(a[2]+a[3])/2),
             (b[0],side*b[1],(b[2]+b[3])/2),.016,teal,8)
for i in range(16):
    a,b=math.pi*i/16,math.pi*(i+1)/16
    tube('Aft canopy hoop',(-.50,.545*math.cos(a),.865+.45*math.sin(a)),
         (-.50,.545*math.cos(b),.865+.45*math.sin(b)),.014,teal,8)

# Box mesh helper for the modest original cabin furnishings.
def box(name,center,size,mat):
    f,r,u=center
    lf,lr,lu=[v/2 for v in size]
    vertices=[(f+df*lf,r+dr*lr,u+du*lu) for df,dr,du in
              [(-1,-1,-1),(1,-1,-1),(1,1,-1),(-1,1,-1),
               (-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]]
    return mesh(name,vertices,[(0,3,2,1),(4,5,6,7),(0,1,5,4),
                              (1,2,6,5),(2,3,7,6),(3,0,4,7)],mat)
box('Interior | instrument panel',(1.24,0,.53),(.08,.87,.27),rubber)
box('Interior | glare shield',(1.14,0,.68),(.24,.90,.035),teal)
for side,label in [(-1,'Port'),(1,'Starboard')]:
    box('Interior | '+label+' seat base',(.12,side*.26,.47),(.59,.37,.09),teal)
    box('Interior | '+label+' seat back',(-.20,side*.26,.67),(.11,.38,.40),teal)
    tube('Interior | '+label+' control stick',(.76,side*.26,.46),(.70,side*.26,.64),.016,alloy,8)
    box('Interior | '+label+' dark display',(1.194,side*.225,.55),(.008,.32,.18),glass)
box('Interior | centre console',(.66,0,.47),(.80,.105,.11),rubber)

# Low wings: exact 11 m span, trapezoidal planform area 16.17 m².
# Root/tip chords 1.99/.95 m; mild sweep and dihedral. This is geometry only.
def wing_ring(distance,chord,center,up,side):
    return [(center+chord*.5*math.cos(2*math.pi*i/32),side*distance,
             up+.060*chord*math.sin(2*math.pi*i/32)*(1+.24*math.cos(2*math.pi*i/32)))
            for i in range(32)]
for side,label in [(-1,'Port'),(1,'Starboard')]:
    stations=[(0,1.99,.05,-.12),(4.85,1.0729090909090908,-.682,.127),
              (5.50,.95,-.78,.16)]
    loft(label+' low wing',[wing_ring(*s,side) for s in stations],cream)
    loft(label+' copper tip',[wing_ring(4.85,1.077,-.682,.127,side),
                              wing_ring(5.50,.954,-.78,.16,side)],amber)
    # Aileron/flap seams follow the trailing upper surface and remain static.
    tube(label+' control seam',(-.57,side*1.25,-.035),(-1.045,side*4.75,.176),.008,teal,6)
    nav=ellipsoid(label+' navigation lens',(-.77,side*5.455,.18),(.08,.045,.040),
                  red if side<0 else green,16,8)
    nav['navigation_side']='port_red' if side<0 else 'starboard_green'
    # Original nose/side paint ribbons stay directly on the surface shoulder.
    for a,b in zip(sections[1:-1],sections[2:]):
        if b[0]>2.71: continue
        mesh(label+' navy side ribbon',[(a[0],side*(a[1]+.002),.00),
             (b[0],side*(b[1]+.002),.00),(b[0],side*(b[1]+.002),.09),
             (a[0],side*(a[1]+.002),.09)],[(0,1,2,3)],teal)
    # Raised compact cheek inlets behind the seating area, no external assets.
    intake_rings=[]
    for f,r,w,h,u in [(-1.90,.43,.10,.15,.18),(-1.30,.62,.19,.24,.18),
                       (-.40,.66,.185,.24,.19),(-.30,.67,.17,.225,.19)]:
        intake_rings.append([(f,side*(r+w*math.cos(2*math.pi*i/24)),
                              u+h*math.sin(2*math.pi*i/24)) for i in range(24)])
    loft(label+' intake housing',intake_rings,teal)
    # Ring facing forward: black disk is recessed behind a visible silver lip.
    ring=[]
    for radius_scale,f in [(1.0,-.287),(.80,-.28),(.80,-.33)]:
        ring.append([(f,side*(.67+.17*radius_scale*math.cos(2*math.pi*i/24)),
                      .19+.225*radius_scale*math.sin(2*math.pi*i/24)) for i in range(24)])
    loft(label+' intake lip',ring,alloy)
    mouth=[(-.276,side*(.67+.132*math.cos(2*math.pi*i/24)),
                     .19+.174*math.sin(2*math.pi*i/24)) for i in range(24)]
    mesh(label+' intake darkness',mouth,[tuple(range(24))],rubber)

# Swept cruciform tail, visually separate from the low main wing.
for side,label in [(-1,'Port'),(1,'Starboard')]:
    loft(label+' horizontal tail',[wing_ring(.0,1.12,-3.74,.67,side),
                  wing_ring(1.80,.62,-4.18,.77,side)],cream)
    loft(label+' tail tip',[wing_ring(1.48,.72,-4.102,.754,side),
                 wing_ring(1.80,.624,-4.18,.77,side)],amber)
outline=[(-2.72,.42),(-3.44,1.78),(-4.31,1.86),(-4.45,.40)]
vertices=[(f,r,u) for r in [-.055,.055] for f,u in outline]
mesh('Vertical tail | swept fin',vertices,[(0,1,2,3),(4,7,6,5),
     (0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],teal)
for side,label in [(-1,'Port'),(1,'Starboard')]:
    mesh(label+' fin copper flash',[(-3.37,side*.057,1.55),(-3.48,side*.057,1.765),
         (-4.30,side*.057,1.842),(-4.32,side*.057,1.63)],[(0,1,2,3)],amber)

# No propeller: single dry exhaust, recessed dark throat and alloy ring.
exhaust_rings=[]
for f,radius in [(-4.42,.225),(-4.67,.235),(-4.85,.235),(-4.85,.186),(-4.62,.186)]:
    exhaust_rings.append([(f,radius*math.cos(2*math.pi*i/32),
                           .01+radius*math.sin(2*math.pi*i/32)) for i in range(32)])
loft('Single rear dry exhaust',exhaust_rings,alloy)
mesh('Exhaust recessed darkness',[(-4.625,.185*math.cos(2*math.pi*i/32),
         .01+.185*math.sin(2*math.pi*i/32)) for i in range(32)],[tuple(range(32))],rubber)

# Fixed tricycle gear: physical contacts are tyre bottom-centres, not hub centres.
for f,r,label in [(1.6,0,'Nose'),(-.8,-1.3,'Port main'),(-.8,1.3,'Starboard main')]:
    root=(1.48,0,-.27) if r==0 else (-.46,math.copysign(.47,r),-.27)
    tube(label+' gear strut',root,(f,r,-.77),.032,alloy,12)
    tube(label+' torque link',(f-.11,r,-.46),(f,r,-.73),.016,teal,8)
    tire_rings=[]
    for lateral,radius in [(-.095,.18),(-.065,.222),(0,.23),(.065,.222),(.095,.18)]:
        tire_rings.append([(f+radius*math.sin(2*math.pi*i/32),r+lateral,
                            -.77+radius*math.cos(2*math.pi*i/32)) for i in range(32)])
    wheel=loft(label+' tyre',tire_rings,rubber)
    wheel['fdm_contact_body_m']=[f,r,1.0]
    tube(label+' axle and hub',(f,r-.10,-.77),(f,r+.10,-.77),.085,alloy,20)

# Merge repeated decorative pieces by semantic group, preserving tyre/cabin names.
for prefix in ['Port canopy rail','Starboard canopy rail','Aft canopy hoop',
               'Port navy side ribbon','Starboard navy side ribbon']:
    objects=[o for o in aircraft_collection.objects if o.name.startswith(prefix)]
    bpy.ops.object.select_all(action='DESELECT')
    for o in objects: o.select_set(True)
    bpy.context.view_layer.objects.active=objects[0]
    bpy.ops.object.join()
    bpy.context.object.name=prefix

# Bake aircraft transforms into mesh vertices, so the exported nodes use an
# identity transform, and the scene origin, not an AABB midpoint, remains CG.
bpy.ops.object.select_all(action='DESELECT')
for obj in aircraft_collection.objects:
    obj.select_set(True)
bpy.context.view_layer.objects.active=fuselage
bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
bpy.ops.export_scene.gltf(filepath=str(args.assets/'kestrel_jet_trainer.glb'),
    export_format='GLB',use_selection=True,export_yup=True,export_extras=True,
    export_cameras=False,export_lights=False,export_animations=False,
    export_materials='EXPORT')

# Blender can emit identical loop triangles in a different index order between
# processes (observed for sphere meshes in 4.3.2), changing buffer deduplication
# too. Canonicalize triangle order and repack the resulting accessors, preserving
# winding and every vertex/normal/material. No runtime parser is changed.
glb_path=args.assets/'kestrel_jet_trainer.glb'
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
scene.cycles.samples=40
scene.cycles.seed=0
scene.cycles.use_animated_seed=False
scene.cycles.use_denoising=False
scene.render.resolution_x=1440
scene.render.resolution_y=960
scene.render.resolution_percentage=100
scene.world=bpy.data.worlds.new('Neutral studio world')
scene.world.use_nodes=True
scene.world.node_tree.nodes['Background'].inputs[0].default_value=(.12,.15,.19,1)
scene.world.node_tree.nodes['Background'].inputs[1].default_value=.45
scene.view_settings.view_transform='AgX'
bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-1.004))
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
camera=studio_object(bpy.context.object,'Kestrel studio camera')
camera.data.type='ORTHO'
camera.data.ortho_scale=13.5
scene.camera=camera
scene.render.image_settings.file_format='JPEG'
scene.render.image_settings.quality=93

def view(forward,right,up,target=(-.6,0,.4)):
    camera.location=point((forward,right,up))
    camera.rotation_euler=(point(target)-camera.location).to_track_quat('-Z','Y').to_euler()

view(11,-15,8)
bpy.ops.object.select_all(action='DESELECT')
fuselage.select_set(True)
bpy.context.view_layer.objects.active=fuselage
# The .blend opens on the first view and preserves the export-only collection.
bpy.ops.wm.save_as_mainfile(filepath=str(args.assets/'kestrel_jet_trainer.blend'),compress=True)
if not args.no_render:
    for suffix,location in [('three-quarter',(11,-15,8)),('starboard',(-10,15,6)),('front',(16,0,3.0))]:
        view(*location)
        scene.render.filepath=str(args.previews/f'kestrel-jet-trainer-{suffix}.jpg')
        bpy.ops.render.render(write_still=True)
    camera.data.type='PERSP'
    camera.data.lens=24
    camera.data.clip_start=.02
    view(.6,-.25,.9,target=(10,-.25,.9))
    scene.render.filepath=str(args.previews/'kestrel-jet-trainer-eye.jpg')
    bpy.ops.render.render(write_still=True)
print('Kestrel Jet Trainer: exported original geometry and editable source'
      + ('; rendering skipped' if args.no_render else '; four studio renders complete'))
