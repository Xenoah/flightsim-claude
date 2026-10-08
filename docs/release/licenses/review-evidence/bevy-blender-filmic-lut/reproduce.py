# CPU-only two-EXR colour-space recipe; never opens or changes the application.
import bpy,hashlib,json,os,sys
from pathlib import Path
import numpy as np
expected_inputs={
 '3.4/datafiles/colormanagement/config.ocio':'87b8abe0f57f8b3cb3132f43f84b240666c7f19f4505cedd5abb81df3cda4bf9',
 '3.4/datafiles/colormanagement/filmic/filmic_desat65cube.spi3d':'2c3dcccb5db817156737691924ade43d8daad16282cc6be3a9e242744002a0bb',
 '3.4/datafiles/colormanagement/filmic/filmic_to_0-70_1-03.spi1d':'ff0e4cd3745c3d6857c163ee33caa2c796d6c5bddd91859e4646518110bf5b13',
 '3.4/datafiles/colormanagement/luts/srgb.spi1d':'0928cc0b9f3c42b8101c67b701dcc4ba002c8a38060f0569522cac1eb7f0ce04',
 '3.4/datafiles/colormanagement/luts/srgb_inv.spi1d':'82be644be7667c32ac656006c1c430b73e4f9765892f9a0bf6ead5961e179806',
 'license/OpenColorIO.txt':'e39e97c2149a821e3e3b3c07839f8cadf6a98fb0a816b8ed69f16dd8817066fc',
}
executable=Path(bpy.app.binary_path).resolve();package=executable.parent
assert hashlib.sha256(executable.read_bytes()).hexdigest()=='40999207b8b7f8a43a8902535cd6e3a35724b9590cb08715b161c39c5a0119f6'
assert Path(bpy.utils.system_resource('DATAFILES')).resolve()==package/'3.4/datafiles'
assert Path(os.environ['OCIO']).resolve()==package/'3.4/datafiles/colormanagement/config.ocio'
assert '--factory-startup' in sys.argv and '--background' in sys.argv
assert '--python-exit-code' in sys.argv and sys.argv[sys.argv.index('--python-exit-code')+1]=='1'
def verify_inputs():
 for name,digest in expected_inputs.items():
  assert hashlib.sha256((package/name).read_bytes()).hexdigest()==digest
verify_inputs()
root=Path(sys.argv[sys.argv.index('--')+1]).resolve(); root.mkdir()
stimulus=Path(__file__).with_name('stimulus.rgba32f')
scene=bpy.context.scene
settings=scene.render.image_settings
settings.file_format='OPEN_EXR';settings.color_depth='32';settings.color_mode='RGBA'
settings.exr_codec='ZIP';settings.color_management='OVERRIDE'
settings.view_settings.view_transform='Standard';settings.view_settings.look='None'
settings.view_settings.exposure=0;settings.view_settings.gamma=1
settings.view_settings.use_curve_mapping=False
scene.render.dither_intensity=0
pixels=np.fromfile(stimulus,dtype='<f4').reshape((4096,64,4))
assert pixels.shape==(4096,64,4) and np.isfinite(pixels).all()
image=bpy.data.images.new('Documented log2 stimulus',width=64,height=4096,float_buffer=True,alpha=True)
image.colorspace_settings.name='Linear';image.alpha_mode='STRAIGHT'
# Rust/image and EXR use top-row-first; Blender's pixel accessor is bottom-row-first.
image.pixels.foreach_set(np.ascontiguousarray(pixels[::-1]).ravel())
settings.linear_colorspace_settings.name='Linear'
image.save_render(str(root/'stimulus.exr'),scene=scene)
bpy.data.images.remove(image)
image=bpy.data.images.load(str(root/'stimulus.exr'),check_existing=False)
image.colorspace_settings.name='Linear';image.alpha_mode='STRAIGHT'
settings.linear_colorspace_settings.name='Filmic sRGB'
image.save_render(str(root/'filmic-srgb.exr'),scene=scene)
bpy.data.images.remove(image)
image=bpy.data.images.load(str(root/'filmic-srgb.exr'),check_existing=False)
image.colorspace_settings.name='sRGB';image.alpha_mode='STRAIGHT'
settings.linear_colorspace_settings.name='Linear'
image.save_render(str(root/'linear-output.exr'),scene=scene)
bpy.data.images.remove(image)
image=bpy.data.images.load(str(root/'linear-output.exr'),check_existing=False)
image.colorspace_settings.name='Linear';image.alpha_mode='STRAIGHT'
values=np.empty(64*4096*4,dtype=np.float32);image.pixels.foreach_get(values)
values=np.ascontiguousarray(values.reshape((4096,64,4))[::-1])
assert np.isfinite(values).all() and np.all(values[:,:,3]==1)
values.astype('<f4').tofile(root/'output.rgba32f')
# half::f16::from_f32 uses nearest-even conversion, matched by IEEE NumPy float16.
values.astype('<f2').tofile(root/'output.rgba16f')
records={}
for p in sorted(root.iterdir()):
 if p.is_file():
  data=p.read_bytes();records[p.name]={'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
report={'kind':'bounded_cpu_filmic_recipe_observation_not_rights_clearance',
 'version':bpy.app.version_string,'build_hash':bpy.app.build_hash.decode(),'ocio':list(bpy.app.ocio),
 'explicit_ocio_config':'3.4/datafiles/colormanagement/config.ocio',
 'evaluator_sha256':'40999207b8b7f8a43a8902535cd6e3a35724b9590cb08715b161c39c5a0119f6',
 'pinned_inputs_sha256':expected_inputs,'pinned_inputs_unchanged':True,
 'recipe':['64-cube log2 stimulus using i/64, exposure -11..12, middle grey 0.18','EXR32 Linear to Filmic sRGB','reinterpret intermediate EXR as sRGB; export EXR32 Linear','top-row-first RGBA32F to nearest-even RGBA16F'],
 'renderer_invoked':False,'release_authorized':False,'files':records}
verify_inputs()
(root/'observation.json').write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
print('FILMIC_REPRODUCTION='+json.dumps(report,sort_keys=True))
