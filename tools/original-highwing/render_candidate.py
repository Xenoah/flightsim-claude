"""Re-import the emitted candidate and render asset-only studio evidence.

Run with the installed Blender, after coordinating CPU use:
  blender --background --threads 2 --python render_candidate.py
Append -- --no-render for import/bounds validation without any image render.
MIT OR Apache-2.0. This does not run FlightSim or validate its renderer.
"""
import hashlib
import json
from pathlib import Path
import sys

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
path = HERE/'assets/aircraft/light_single.glb'
expected = 'b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc'
assert hashlib.sha256(path.read_bytes()).hexdigest() == expected
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=str(path))
objects = list(bpy.context.scene.objects)
assert len(objects) == 46 and all(obj.type == 'MESH' for obj in objects)
points = [obj.matrix_world @ v.co for obj in objects for v in obj.data.vertices]
minimum = [min(p[i] for p in points) for i in range(3)]
maximum = [max(p[i] for p in points) for i in range(3)]
# Blender import maps glTF (x,y,z) -> Blender (x,-z,y).
assert max(abs(a-b) for a,b in zip(minimum,[-3.250000238418579,-5.5,-1])) < 2e-6
assert max(abs(a-b) for a,b in zip(maximum,[5.050000190734863,5.5,2.2200000286102295])) < 2e-6
report = {'status':'PASS: Blender independently imported emitted GLB',
          'blender_version':bpy.app.version_string,
          'blender_build_hash':bpy.app.build_hash.decode(),
          'glb_sha256':expected,'imported_mesh_objects':len(objects),
          'imported_materials':len(bpy.data.materials),
          'blender_bounds_m':{'min':minimum,'max':maximum},
          'render_type':'asset-only studio render, not a simulator screenshot',
          'images':[]}
if '--no-render' not in sys.argv:
    scene = bpy.context.scene
    scene.unit_settings.system = 'METRIC'
    scene.unit_settings.scale_length = 1
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    scene.cycles.samples = 32
    scene.cycles.seed = 0
    scene.cycles.use_animated_seed = False
    scene.cycles.use_denoising = False
    scene.render.resolution_x = 1200
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    # QA display only. This choice does not change FlightSim tone recipes.
    scene.view_settings.view_transform = 'Standard'
    scene.view_settings.look = 'None'
    scene.world = bpy.data.worlds.new('Candidate QA studio')
    scene.world.use_nodes = True
    scene.world.node_tree.nodes['Background'].inputs[0].default_value = (.12,.15,.19,1)
    scene.world.node_tree.nodes['Background'].inputs[1].default_value = .45
    bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-1.004))
    ground = bpy.context.object
    ground.name = 'QA ground, never exported'
    material = bpy.data.materials.new('QA studio slate')
    material.use_nodes = True
    material.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (.105,.135,.16,1)
    material.node_tree.nodes['Principled BSDF'].inputs['Roughness'].default_value = .8
    ground.data.materials.append(material)
    for name,position,energy,size in [('Key',(-7,-5,10),1900,8),('Fill',(-2,6,6),1200,7),('Rim',(8,0,8),1700,6)]:
        bpy.ops.object.light_add(type='AREA',location=position)
        lamp = bpy.context.object
        lamp.name = name
        lamp.data.energy = energy
        lamp.data.shape = 'DISK'
        lamp.data.size = size
        lamp.rotation_euler = (Vector((.6,0,.4))-lamp.location).to_track_quat('-Z','Y').to_euler()
    bpy.ops.object.camera_add()
    camera = bpy.context.object
    camera.data.type = 'ORTHO'
    camera.data.ortho_scale = 13.5
    scene.camera = camera
    scene.render.image_settings.file_format = 'JPEG'
    scene.render.image_settings.quality = 93
    for name,(forward,right,up) in [('three-quarter',(11,-15,8)),('starboard',(-10,15,6)),('front',(16,0,3))]:
        camera.location = (-forward,right,up)
        camera.rotation_euler = (Vector((.6,0,.4))-camera.location).to_track_quat('-Z','Y').to_euler()
        image = HERE/'previews'/('candidate-'+name+'.jpg')
        scene.render.filepath = str(image)
        bpy.ops.render.render(write_still=True)
        report['images'].append({'path':str(image.relative_to(HERE)), 'sha256':hashlib.sha256(image.read_bytes()).hexdigest()})
    report['render_settings'] = {'engine':'Cycles','device':'CPU','samples':32,'denoising':False,'size':[1200,800],
                                 'view_transform':'Standard','look':'None'}
(HERE/'evidence/blender-import-and-render.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
