"""Audit Cedar source and GLB import with Blender 4.3.2; MIT OR Apache-2.0.

blender --background --disable-autoexec --threads 2 --python-exit-code 1 \
  --python tools/blender/audit_cedar_blender.py
"""
import hashlib
import json
from pathlib import Path
import bpy

ROOT=Path(__file__).resolve().parents[2]
ASSETS=ROOT/'assets/aircraft/experimental'
blend=ASSETS/'cedar_turboprop_experimental.blend'
glb=ASSETS/'cedar_turboprop_experimental.glb'
bpy.ops.wm.open_mainfile(filepath=str(blend))
assert not bpy.data.libraries and not bpy.data.texts and not bpy.data.textures
assert not any(image.filepath for image in bpy.data.images)
assert not any(obj.animation_data for obj in bpy.data.objects)
assert not any(mat.animation_data for mat in bpy.data.materials)
assert not any(scene.animation_data for scene in bpy.data.scenes)
aircraft=bpy.data.collections['Cedar aircraft | export only']
assert len(aircraft.objects)==45
source_meshes=[obj for obj in aircraft.objects if obj.type=='MESH']
assert len(source_meshes)==44
assert len([obj for obj in aircraft.objects if obj.type=='EMPTY'])==1
studio=bpy.data.collections['Studio | not exported']
assert len([obj for obj in studio.objects if obj.type=='LIGHT'])==3
assert len([obj for obj in studio.objects if obj.type=='CAMERA'])==1
# Compare actual source/imported world vertices, independent of normal splits.
# Blender glTF export/import applies the Y-up basis round-trip, not object fit.
def positions(objects):
    return {obj.name:{tuple(float(value) for value in obj.matrix_world@v.co)
                     for v in obj.data.vertices} for obj in objects}
source_points=positions(source_meshes)
source_polygons=sum(len(obj.data.polygons) for obj in source_meshes)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=str(glb))
objects=list(bpy.context.scene.objects)
assert len(objects)==45
assert len([obj for obj in objects if obj.type=='EMPTY'])==1
objects=[obj for obj in objects if obj.type=='MESH']
assert len(objects)==44
assert not bpy.data.images and not bpy.data.texts and not bpy.data.libraries
assert not bpy.data.cameras and not bpy.data.lights
assert len(bpy.data.materials)==8
imported_points=positions(objects)
assert source_points.keys()==imported_points.keys()
# Use nearest source-vertex distance for every distinct imported point and
# vice versa; this tolerates float32 axis conversion, but not missing geometry.
maximum_error=0.
for name,points in imported_points.items():
    source=source_points[name]
    for p in points:
        error=min(sum((x-y)**2 for x,y in zip(p,q))**.5 for q in source)
        maximum_error=max(maximum_error,error)
        assert error<3e-5,(name,error)
    for p in source:
        error=min(sum((x-y)**2 for x,y in zip(p,q))**.5 for q in points)
        maximum_error=max(maximum_error,error)
        assert error<3e-5,(name,error)
report={'blender_version':bpy.app.version_string,'status':'source hygiene and GLB import PASS',
        'source_meshes':44,'source_polygons':source_polygons,'imported_meshes':len(objects),
        'imported_materials':len(bpy.data.materials),'source_import_vertex_error_bound_m':3e-5,
        'maximum_world_vertex_distance_m':maximum_error,
        'source_embedded_scripts_external_resources_linked_libraries':0,
        'imported_cameras_lights_images_scripts':0,
        'studio_camera_and_lights_excluded_from_glb':True,
        'sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [blend,glb]}}
(ROOT/'docs/qa/cedar-turboprop-blender-audit.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
