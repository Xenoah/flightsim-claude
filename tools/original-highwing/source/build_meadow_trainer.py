"""Build the original Meadow Trainer exterior and actual CPU studio previews.

Blender 4.3.2:
  blender --background --threads 2 --python tools/blender/build_meadow_trainer.py
Optional arguments after --: ASSET_DIRECTORY PREVIEW_DIRECTORY [--no-render]

The asset is hand-authored procedural geometry in this file. No network, imported
mesh, texture, logo, blueprint or reference-aircraft dimensions are used. Numeric
gear contacts, span and camera location come from this project's Light Single
profile. Geometry/material construction is deterministic; .blend container bytes
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
parser.add_argument('assets', nargs='?', type=Path, default=ROOT / 'assets/aircraft')
parser.add_argument('previews', nargs='?', type=Path, default=ROOT / 'docs/qa/images')
parser.add_argument('--no-render', action='store_true')
args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else [])
args.assets.mkdir(parents=True, exist_ok=True)
args.previews.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.unit_settings.system = 'METRIC'
scene.unit_settings.scale_length = 1.0
aircraft_collection = bpy.data.collections.new('Meadow Trainer | export only this collection')
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


cream = material('Warm white enamel', (.82, .84, .79), .08, .33)
teal = material('Deep teal enamel', (.015, .18, .20), .15, .29)
amber = material('Golden amber enamel', (.96, .48, .055), .10, .35)
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


def ellipsoid(name, center, size, mat, segments=20, rings=10):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=rings, location=point(center))
    obj = bpy.context.object
    f, r, u = size
    obj.scale = (r, f, u)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(obj, name, mat, True)


# Rounded, slightly squared cabin section: the origin remains the dynamics CG.
sections = [(-5.05, .035, .39, .27), (-4.72, .095, .44, .21),
            (-3.9, .19, .49, .10), (-2.75, .33, .62, -.08),
            (-1.70, .48, .90, -.30), (-.82, .59, 1.26, -.42),
            (.65, .60, 1.22, -.44), (1.48, .50, .57, -.38),
            (2.28, .405, .44, -.30), (2.79, .31, .30, -.23),
            (2.98, .19, .20, -.17)]
fuselage_rings = []
for forward, width, top, bottom in sections:
    ring = []
    for i in range(32):
        a = 2*math.pi*i/32
        s, c = math.sin(a), math.cos(a)
        roundness = .55 if -.83 <= forward <= .66 else .80
        right = width*math.copysign(abs(c)**roundness, c)
        up = (top+bottom)/2 + (top-bottom)/2*math.copysign(abs(s)**roundness, s)
        ring.append((forward, right, up))
    fuselage_rings.append(ring)
fuselage = loft('Fuselage | CG at origin', fuselage_rings, cream)


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
    steps=8
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

# Glazing is deliberately opaque and shallow: no alpha sorting or interior
# promises. These exterior panes do not replace the simulator's shared cockpit.
for side, label in [(-1, 'Port'), (1, 'Starboard')]:
    def side_surface(f,u):
        return (f,side*(surface_coordinate(f,u,2,1)+.012),u)
    glazing(label+' forward cabin window',[(.63,.59),(-.36,.59),(-.36,1.13),(.60,1.10)],side_surface)
    glazing(label+' rear cabin window',[(-.46,.59),(-1.30,.58),(-1.30,.98),(-.46,1.13)],side_surface)
    def top_surface(f,r):
        return (f,side*r,surface_coordinate(f,r,1,2)+.012)
    glazing(label+' windshield',[(1.445,.035),(1.445,.42),(.67,.50),(.67,.035)],top_surface)
    # A deliberate two-tone belt, on both sides, follows fuselage station widths.
    stripe_rings = []
    for f,w,top,bottom in sections[1:-1]:
        center=(top+bottom)/2
        stripe_rings.append([(f,side*(w+.004),center+.015),(f,side*(w+.004),center+.10)])
    mesh(label+' teal belt',[v for ring in stripe_rings for v in ring],
         [(i*2,i*2+1,i*2+3,i*2+2) for i in range(len(stripe_rings)-1)],teal)
    tube(label+' door sill',(.61,side*.609,.48),(-.85,side*.602,.48),.012,teal,8)
    tube(label+' door handle',(-.37,side*.622,.49),(-.58,side*.622,.49),.018,alloy,8)

# High wings: span 11 m, a modest original rounded airfoil and slight dihedral.
# The geometry is illustrative; numeric aerodynamic coefficients stay unchanged.
def wing_ring(distance, chord, center, up, side):
    result=[]
    for i in range(32):
        a=2*math.pi*i/32
        result.append((center+chord*.5*math.cos(a), side*distance,
                       up+.060*chord*math.sin(a)*(1+.24*math.cos(a))))
    return result

for side, label in [(-1,'Port'),(1,'Starboard')]:
    rings=[wing_ring(.0,1.56,.02,1.32,side),wing_ring(3.90,1.51,-.01,1.39,side),
           wing_ring(5.10,1.36,-.07,1.42,side),wing_ring(5.40,1.20,-.10,1.43,side),
           wing_ring(5.50,.98,-.13,1.43,side)]
    loft(label+' high wing',rings,cream)
    # Colored tip is a full airfoil sleeve, offset only 2 mm to avoid z-fighting.
    tip=loft(label+' amber wingtip', [wing_ring(5.10,1.366,-.07,1.42,side),
         wing_ring(5.40,1.206,-.10,1.43,side),wing_ring(5.50,.986,-.13,1.43,side)],amber)
    tube(label+' wing lift strut',(-.30,side*.52,-.20),(.02,side*3.58,1.30),.035,teal,12)
    tube(label+' rear lift strut',(-.67,side*.53,-.18),(-.58,side*3.58,1.335),.024,teal,12)
    # Surface divisions are geometry and dark material, no decals or textures.
    tube(label+' flap hinge',(-.54,side*.78,1.388),(-.57,side*2.83,1.429),.007,teal,8)
    tube(label+' aileron hinge',(-.58,side*2.93,1.431),(-.57,side*5.08,1.465),.007,teal,8)
    ellipsoid(label+' navigation lens',(-.11,side*5.475,1.435),(.068,.024,.033),red if side<0 else green,16,8)

# Conventional tail, intentionally unbranded and unrelated to a named airframe.
for side,label in [(-1,'Port'),(1,'Starboard')]:
    loft(label+' tailplane', [wing_ring(.10,1.26,-4.14,.44,side),
         wing_ring(1.30,.94,-4.38,.51,side),wing_ring(1.76,.56,-4.56,.55,side)],cream)
    tube(label+' elevator hinge',(-4.56,side*.25,.49),(-4.67,side*1.68,.582),.007,teal,8)
fin_outline=[(-3.45,.38),(-4.98,.36),(-4.79,2.12),(-4.48,2.22),(-4.18,1.42)]
fin_vertices=[(f,side*.048,u) for side in [-1,1] for f,u in fin_outline]
count=len(fin_outline)
fin_faces=[tuple(reversed(range(count))),tuple(range(count,2*count))]
fin_faces += [(i,(i+1)%count,(i+1)%count+count,i+count) for i in range(count)]
mesh('Swept vertical fin',fin_vertices,fin_faces,teal)
for side,label in [(-1,'Port'),(1,'Starboard')]:
    mesh(label+' fin amber flash',[(-4.765,side*.050,1.89),(-4.37,side*.050,1.90),
         (-4.49,side*.050,2.15),(-4.715,side*.050,2.075)],[(0,1,2,3)],amber)

# Three fixed wheels. The bottom centers exactly match Light Single contact_m
# [body forward, right, down]: (1.6,0,1), (-.8,-1.3,1), (-.8,1.3,1).
# Static mesh: it does not animate compression, steering or wheel spin.
for label,f,r in [('Nose',1.6,0),('Port main',-.8,-1.3),('Starboard main',-.8,1.3)]:
    tube(label+' gear leg',(f*.92,r*.34,-.30),(f,r,-.77),.035,alloy)
    tire_rings=[]
    for lateral,radius in [(-.095,.18),(-.065,.222),(0,.23),(.065,.222),(.095,.18)]:
        tire_rings.append([(f+radius*math.sin(2*math.pi*i/32),r+lateral,
                            -.77+radius*math.cos(2*math.pi*i/32)) for i in range(32)])
    wheel=loft(label+' tyre',tire_rings,rubber)
    wheel['fdm_contact_body_m']=[f,r,1.0]
    tube(label+' axle and hub',(f,r-.10,-.77),(f,r+.10,-.77),.085,alloy,20)

# A two-blade propeller matches the existing piston sound family. Static only.
ellipsoid('Spinner',(3.005,0,.015),(.245,.19,.19),teal)
for side,label in [(-1,'Lower'),(1,'Upper')]:
    rings=[]
    for radius,width,sweep in [(.10,.067,0),(.32,.112,.025),(.66,.091,.065),(.98,.046,.095),(1.04,.009,.101)]:
        ring=[]
        for i in range(12):
            a=2*math.pi*i/12
            # Slight angular offset keeps both blades clear of the ground plane.
            radial=side*radius
            right=radial*.55 + width*math.cos(a)
            up=.015+radial*.835
            ring.append((2.97+sweep+.023*math.sin(a),right,up))
        rings.append(ring)
    loft(label+' propeller blade',rings,rubber)

# Bake aircraft transforms into mesh vertices, so the exported nodes use an
# identity transform, and the scene origin, not an AABB midpoint, remains CG.
bpy.ops.object.select_all(action='DESELECT')
for obj in aircraft_collection.objects:
    obj.select_set(True)
bpy.context.view_layer.objects.active=fuselage
bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
bpy.ops.export_scene.gltf(filepath=str(args.assets/'meadow_trainer.glb'),
    export_format='GLB',use_selection=True,export_yup=True,export_extras=True,
    export_cameras=False,export_lights=False,export_animations=False,
    export_materials='EXPORT')

# Blender can emit identical loop triangles in a different index order between
# processes (observed for sphere meshes in 4.3.2), changing buffer deduplication
# too. Canonicalize triangle order and repack the resulting accessors, preserving
# winding and every vertex/normal/material. No runtime parser is changed.
glb_path=args.assets/'meadow_trainer.glb'
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
camera=studio_object(bpy.context.object,'Meadow studio camera')
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
bpy.ops.wm.save_as_mainfile(filepath=str(args.assets/'meadow_trainer.blend'),compress=True)
if not args.no_render:
    for suffix,location in [('three-quarter',(11,-15,8)),('starboard',(-10,15,6)),('front',(16,0,3.0))]:
        view(*location)
        scene.render.filepath=str(args.previews/f'meadow-trainer-{suffix}.jpg')
        bpy.ops.render.render(write_still=True)
print('Meadow Trainer: exported original geometry and editable source'
      + ('; rendering skipped' if args.no_render else '; three studio renders complete'))
