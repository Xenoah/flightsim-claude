"""Original generic two-seat sport aircraft; Blender 4.3+ reproducible asset.
Run: blender --background --python tools/blender/build_swift_sport.py -- OUTPUT_DIR
Blender axes: +X nose, +Z up. glTF export: +X nose, +Y up. Units: metres.
This is an original stylized simulator asset, not a licensed real aircraft model.
"""
import bpy, math, sys
from pathlib import Path
from mathutils import Vector

out = Path(sys.argv[sys.argv.index('--') + 1]) if '--' in sys.argv else Path('assets/aircraft')
out.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)


def mat(name, color, metallic=0.0, roughness=0.4):
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*color, 1.0)
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get('Principled BSDF')
    bsdf.inputs['Base Color'].default_value = (*color, 1.0)
    bsdf.inputs['Metallic'].default_value = metallic
    bsdf.inputs['Roughness'].default_value = roughness
    return m

ivory = mat('Ivory paint', (0.87, 0.88, 0.83), .16, .27)
navy = mat('Deep blue paint', (.016, .074, .15), .3, .27)
orange = mat('Safety orange livery', (1.0, .24, .026), .15, .3)
glass = mat('Blue tinted canopy', (.033, .15, .22), .68, .14)
rubber = mat('Tyres', (.011, .014, .02), .0, .8)
metal = mat('Brushed aluminium', (.37, .42, .46), .82, .23)
red = mat('Port navigation light', (.75, .016, .012), .1, .25)
green = mat('Starboard navigation light', (.015, .5, .045), .1, .25)


def finish(obj, name, material, smooth=False):
    obj.name = name
    obj.data.materials.append(material)
    if smooth:
        for p in obj.data.polygons:
            p.use_smooth = True
    return obj


def uv(name, pos, scale, material):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=32, ring_count=16, location=pos)
    obj=bpy.context.object
    obj.scale=scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(obj,name,material,True)


def tube(name,a,b,radius,material,vertices=16):
    a,b=Vector(a),Vector(b)
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=(b-a).length, location=(a+b)/2)
    obj=bpy.context.object
    obj.rotation_euler=(b-a).to_track_quat('Z','Y').to_euler()
    return finish(obj,name,material,True)


def mesh(name, verts, faces, material):
    data=bpy.data.meshes.new(name)
    data.from_pydata(verts,[],faces)
    data.update()
    obj=bpy.data.objects.new(name,data)
    bpy.context.collection.objects.link(obj)
    return finish(obj,name,material)

# Streamlined watertight fuselage with six elliptical longitudinal stations.
sections=[(-3.5,.09,.11,.28),(-2.5,.24,.26,.22),(-1.25,.48,.43,.08),(.3,.58,.54,.0),(1.65,.45,.41,-.015),(2.7,.23,.27,-.015),(3.35,.05,.065,-.015)]
verts=[]
for x,w,h,z in sections:
    for i in range(32):
        a=2*math.pi*i/32
        verts.append((x,w*math.cos(a),z+h*math.sin(a)))
faces=[]
for j in range(len(sections)-1):
    for i in range(32):
        k=j*32+i;n=j*32+(i+1)%32
        faces.append((k,n,n+32,k+32))
faces.extend([tuple(reversed(range(32))),tuple((len(sections)-1)*32+i for i in range(32))])
fuselage=mesh('Swift fuselage',verts,faces,ivory)
for p in fuselage.data.polygons:p.use_smooth=True

# Canopy over two seats, fully closed glTF-compatible opaque material.
uv('Canopy',(.12,0,.46),(1.12,.525,.56),glass)
tube('Canopy central bow',(.12,-.51,.56),(.12,.51,.56),.027,navy)

# Low wings: rounded airfoil stations, taper and slight dihedral.
def wing(side):
    verts=[]
    for y,chord,xcenter,z in [(0.40,1.65,.12,-.23),(2.75,1.26,-.05,-.06),(4.70,.86,-.30,.12)]:
        for i in range(20):
            a=2*math.pi*i/20
            x=xcenter+chord*.5*math.cos(a)
            thick=.075*chord*math.sin(a)*(0.72+.28*math.cos(a))
            verts.append((x,side*y,z+thick))
    faces=[]
    for j in range(2):
        for i in range(20):
            faces.append((j*20+i,j*20+(i+1)%20,(j+1)*20+(i+1)%20,(j+1)*20+i))
    faces.extend([tuple(reversed(range(20))),tuple(40+i for i in range(20))])
    obj=mesh(('Port' if side>0 else 'Starboard')+' low wing',verts,faces,ivory)
    for p in obj.data.polygons:p.use_smooth=True
    # Livery is physical geometry on the upper surface, no external texture files.
    band_vertices=[]
    for distance in [3.45, 3.78]:
        t=(distance-2.75)/(4.70-2.75)
        chord=1.26+t*(.86-1.26)
        xcenter=-.05+t*(-.30+.05)
        z=-.06+t*(.12+.06)
        for i in range(20):
            a=2*math.pi*i/20
            band_vertices.append((xcenter+chord*.5*math.cos(a),side*distance,z+(.075*chord*math.sin(a)*(0.72+.28*math.cos(a)))+(.0015 if math.sin(a)>=0 else -.0015)))
    band=mesh('Conforming orange wing band',band_vertices,[(i,(i+1)%20,20+(i+1)%20,20+i) for i in range(20)],orange)
    for polygon in band.data.polygons: polygon.use_smooth=True
    tube('Wing control seam',(-.53,side*.72,-.145),(-.69,side*4.43,.11),.008,navy,8)
    uv('Wingtip light',(-.29,side*4.69,.13),(.075,.07,.045),red if side>0 else green)
wing(1);wing(-1)

# Tailplane and swept vertical fin.
for side in [-1,1]:
    mesh('Tailplane', [(-2.5,0,.34),(-3.48,0,.34),(-3.5,side*1.75,.43),(-2.91,side*1.75,.43),(-2.5,0,.40),(-3.48,0,.40),(-3.5,side*1.75,.48),(-2.91,side*1.75,.48)],[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],ivory)
mesh('Fin', [(-2.1,-.04,.32),(-3.47,-.04,.32),(-3.37,-.04,1.69),(-2.95,-.04,1.77),(-2.1,.04,.32),(-3.47,.04,.32),(-3.37,.04,1.69),(-2.95,.04,1.77)],[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],navy)
for side in [-1,1]:
    mesh('Fin orange flash',[(-3.35,side*.043,1.45),(-2.83,side*.043,1.43),(-2.96,side*.043,1.73),(-3.33,side*.043,1.67)],[(0,1,2,3)],orange)
    tube('Fuselage livery',(-2.65,side*.225,.28),(1.92,side*.40,.08),.027,orange,12)

# Three-wheel fixed gear; wheel bottoms at FDM contact coordinates z=-0.85.
for x,y in [(1.35,0),(-.70,-1.25),(-.70,1.25)]:
    tube('Gear strut',(x*.87,y*.36,-.25),(x,y,-.65),.035,metal)
    tube('Tyre',(x,y-.075,-.65),(x,y+.075,-.65),.20,rubber,24)
    tube('Wheel hub',(x,y-.079,-.65),(x,y+.079,-.65),.092,metal,20)

# Three-blade propeller, stationary in exported asset.
uv('Spinner',(3.28,0,-.015),(.34,.21,.21),orange)
for angle in [0,2*math.pi/3,4*math.pi/3]:
    obj=uv('Propeller blade',(3.19,math.sin(angle)*.53,math.cos(angle)*.53-.015),(.048,.105,.57),navy)
    obj.rotation_euler.x=-angle

# Export only the aircraft, before adding studio staging.
aircraft=[o for o in bpy.context.scene.objects if o.type=='MESH']
bpy.ops.object.select_all(action='DESELECT')
for obj in aircraft:obj.select_set(True)
bpy.context.view_layer.objects.active=fuselage
bpy.ops.export_scene.gltf(filepath=str(out/'swift_sport.glb'),export_format='GLB',use_selection=True,export_yup=True)

# Preserve editable source with ground, camera and lighting in a separate collection.
scene=bpy.context.scene
scene.render.engine='CYCLES'
scene.cycles.samples=32
scene.cycles.use_denoising=False
scene.render.resolution_x=1280;scene.render.resolution_y=800;scene.render.resolution_percentage=100
scene.world.color=(.13,.16,.22)
bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-.86))
finish(bpy.context.object,'Studio ground',mat('Studio ground',(.055,.074,.095),0,.8))
for name,pos,energy,size in [('Key',(3,-6,10),1700,8),('Fill',(-4,3,7),1200,7),('Rim',(-5,-4,4),1000,5)]:
    bpy.ops.object.light_add(type='AREA',location=pos)
    obj=bpy.context.object;obj.name=name;obj.data.energy=energy;obj.data.shape='DISK';obj.data.size=size
    obj.rotation_euler=(-obj.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add(location=(10,-13,7))
camera=bpy.context.object;camera.name='Aircraft presentation camera';camera.data.lens=46
camera.rotation_euler=(Vector((0,0,.2))-camera.location).to_track_quat('-Z','Y').to_euler();scene.camera=camera
scene.render.image_settings.file_format='JPEG'
scene.render.image_settings.quality=92
bpy.ops.wm.save_as_mainfile(filepath=str(out/'swift_sport.blend'), compress=True)
scene.render.filepath=str(out/'swift_sport_three_quarter.jpg')
bpy.ops.render.render(write_still=True)
camera.location=(8,12,5)
camera.rotation_euler=(Vector((0,0,.2))-camera.location).to_track_quat('-Z','Y').to_euler()
scene.render.filepath=str(out/'swift_sport_port.jpg')
bpy.ops.render.render(write_still=True)
print('Swift Sport export and actual Blender renders complete:',out)
