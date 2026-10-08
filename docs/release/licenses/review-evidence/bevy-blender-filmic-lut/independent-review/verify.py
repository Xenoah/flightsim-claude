"""Read-only audit of one pinned reproduction; never invokes Blender."""
from pathlib import Path
import argparse, ctypes, ctypes.util, struct, hashlib, json, math, zlib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--base', type=Path, required=True,
                    help='Complete reproduction working directory containing the evaluator, inputs, evidence and runs')
parser.add_argument('--reference', type=Path, required=True,
                    help='Actual Bevy v0.18.1 Blender_-11_12.ktx2 reference file')
parser.add_argument('--run', action='append',
                    help='Run directory name relative to --base; repeat for multiple runs (default: run-5 and run-6)')
args = parser.parse_args()
BASE = args.base
REF = args.reference
RUNS = args.run or ['run-5','run-6']
if any(Path(run).is_absolute() or len(Path(run).parts) != 1 or run in ('.','..') for run in RUNS):
    parser.error('--run must be a simple directory name relative to --base')
sha = lambda b: hashlib.sha256(b).hexdigest()
blob = lambda b: hashlib.sha1(('blob '+str(len(b))+'\0').encode()+b).hexdigest()
report = {'kind': 'independent_read_only_bounded_filmic_audit', 'release_authorized': False, 'whole_release_review': False, 'evaluator_rerun': False}
b = REF.read_bytes()
assert b[:12] == b'\xabKTX 20\xbb\r\n\x1a\n'
keys = ['vkFormat','typeSize','pixelWidth','pixelHeight','pixelDepth','layerCount','faceCount','levelCount','supercompressionScheme','dfdByteOffset','dfdByteLength','kvdByteOffset','kvdByteLength','sgdByteOffset','sgdByteLength']
h = dict(zip(keys, struct.unpack_from('<13I2Q', b, 12)))
assert [h[x] for x in ['vkFormat','pixelWidth','pixelHeight','pixelDepth','layerCount','faceCount','levelCount','supercompressionScheme']] == [97,64,64,64,0,1,1,2]
off, clen, ulen = struct.unpack_from('<3Q', b, 80)
assert off+clen == len(b) and ulen == 64**3*8
z = ctypes.CDLL(ctypes.util.find_library('zstd'))
z.ZSTD_decompress.argtypes = [ctypes.c_void_p,ctypes.c_size_t,ctypes.c_void_p,ctypes.c_size_t]
z.ZSTD_decompress.restype = ctypes.c_size_t
src = ctypes.create_string_buffer(b[off:off+clen])
dst = ctypes.create_string_buffer(ulen)
n = z.ZSTD_decompress(dst, ulen, src, clen)
assert n == ulen
dec = dst.raw[:n]
report['reference'] = {'path':'bevy_core_pipeline-0.18.1/src/tonemapping/luts/Blender_-11_12.ktx2','header':h,'level':[off,clen,ulen],'file_sha256':sha(b),'git_blob':blob(b),'decoded_sha256':sha(dec),'all_alpha_1':all(dec[i:i+2] == b'\0<' for i in range(6,len(dec),8))}
report['runs'] = {}
for run in RUNS:
    p = BASE/run
    f = (p/'output.rgba32f').read_bytes()
    o = (p/'output.rgba16f').read_bytes()
    rounded = b''.join(struct.pack('<e', v[0]) for v in struct.iter_unpack('<f', f))
    observation = json.loads((p/'observation.json').read_text())
    checks = {'exact_reference_decoded_bytes':dec == o,'nearest_even_stdlib_conversion_matches':rounded == o,'float32_all_finite':all(math.isfinite(x[0]) for x in struct.iter_unpack('<f',f)),'float32_alpha_exactly_one':all(v[3] == 1 for v in struct.iter_unpack('<4f',f)),'observation_all_file_hashes_valid':all(sha((p/k).read_bytes()) == v['sha256'] and (p/k).stat().st_size == v['bytes'] for k,v in observation['files'].items())}
    assert all(checks.values())
    report['runs'][run] = {**checks,'different_bytes':sum(a != b for a,b in zip(dec,o)),'decoded_bytes':len(o),'output16_sha256':sha(o),'output32_sha256':sha(f),'observation_sha256':sha((p/'observation.json').read_bytes())}
report['run_observations_identical'] = all((BASE/RUNS[0]/'observation.json').read_bytes() == (BASE/run/'observation.json').read_bytes() for run in RUNS)
report['invocation_checks'] = {}
for run in RUNS:
    path = BASE/run/'invocation.json'
    if not path.exists():
        continue
    inv = json.loads(path.read_text())
    checks = {'process_exited_successfully': inv['process_exit_code'] == 0,'configuration_acknowledged':inv['explicit_ocio_configuration_acknowledged'] is True,'ambient_ocio_removed':inv['ambient_ocio_variables_removed_before_start'] is True,'current_script_hash_matches':inv['reproduction_script_sha256'] == sha((BASE/'reproduce.py').read_bytes()),'current_stimulus_hash_matches':inv['stimulus_sha256'] == sha((BASE/'stimulus.rgba32f').read_bytes()),'observation_binding_matches':inv['observation_sha256'] == sha((BASE/run/'observation.json').read_bytes()),'no_authorization':inv['release_authorized'] is False}
    assert all(checks.values())
    report['invocation_checks'][run] = {**checks,'invocation_sha256':sha(path.read_bytes())}

def read_zip_exr(path):
    """Independently decode these bounded RGBA32F, ZIP16 scanline EXRs."""
    b = path.read_bytes()
    magic, version = struct.unpack_from('<II', b)
    assert magic == 20000630 and version == 2
    offset = 8
    attrs = {}
    while b[offset]:
        end = b.index(0,offset); name = b[offset:end].decode(); offset = end+1
        end = b.index(0,offset); kind = b[offset:end].decode(); offset = end+1
        size = struct.unpack_from('<I',b,offset)[0]; offset += 4
        attrs[name] = (kind,b[offset:offset+size]); offset += size
    offset += 1
    assert struct.unpack('<4i',attrs['dataWindow'][1]) == (0,0,63,4095)
    assert attrs['compression'][1] == b'\x03' and attrs['lineOrder'][1] == b'\x00'
    v = attrs['channels'][1]; i = 0; channels = []
    while v[i]:
        end = v.index(0,i); name = v[i:end].decode(); i = end+1
        ptype, plinear, xs, ys = struct.unpack_from('<iB3xii',v,i); i += 16
        assert ptype == 2 and xs == 1 and ys == 1
        channels.append(name)
    assert channels == ['A','B','G','R']
    out = bytearray(64*4096*16)
    for c in range(256):
        chunk = struct.unpack_from('<Q',b,offset+c*8)[0]
        y, size = struct.unpack_from('<iI',b,chunk)
        assert y == c*16
        packed = b[chunk+8:chunk+8+size]
        expected = 16*64*16
        if size == expected:
            raw = packed
        else:
            pred = bytearray(zlib.decompress(packed)); assert len(pred) == expected
            for j in range(1,len(pred)):
                pred[j] = (pred[j-1]+pred[j]-128)&255
            raw = bytearray(expected)
            raw[::2] = pred[:expected//2]; raw[1::2] = pred[expected//2:]
        for row in range(16):
            for ch,name in enumerate(channels):
                out_ch = 'RGBA'.index(name)
                for x in range(64):
                    source = row*1024+ch*256+x*4
                    dest = ((y+row)*64+x)*16+out_ch*4
                    out[dest:dest+4] = raw[source:source+4]
    return bytes(out)

report['independent_exr_decode'] = {}
for run in RUNS:
    p = BASE/run
    stim = read_zip_exr(p/'stimulus.exr')
    final = read_zip_exr(p/'linear-output.exr')
    intermediate = read_zip_exr(p/'filmic-srgb.exr')
    checks = {'stimulus_equals_top_first_rgba32':stim == (BASE/'stimulus.rgba32f').read_bytes(),'linear_exr_equals_output_rgba32':final == (p/'output.rgba32f').read_bytes(),'all_intermediate_finite':all(math.isfinite(v[0]) for v in struct.iter_unpack('<f',intermediate))}
    assert all(checks.values())
    report['independent_exr_decode'][run] = {**checks,'dimensions':[64,4096],'channel_type':'FLOAT32','line_order':'increasing Y','compression':'ZIP16'}
# Recompute the exact 64 input levels and all axis coordinates without Blender.
libm = ctypes.CDLL(ctypes.util.find_library('m'))
libm.powf.argtypes = [ctypes.c_float,ctypes.c_float]
libm.powf.restype = ctypes.c_float
f32 = lambda v: struct.unpack('<f',struct.pack('<f',v))[0]
levels = []
for i in range(64):
    grid = f32(f32(i)*f32(1/64))
    exposure = f32(f32(grid*f32(23))+f32(-11))
    radiance = f32(libm.powf(2.,exposure)*f32(.18))
    levels.append(radiance)
s = (BASE/'stimulus.rgba32f').read_bytes()
assert len(s) == 64**3*16
grid_matches = all(v == (levels[i%64],levels[(i//64)%64],levels[i//4096],1.) for i,v in enumerate(struct.iter_unpack('<4f',s)))
assert grid_matches
report['stimulus'] = {'matches_all_documented_grid_coordinates':grid_matches,'sha256':sha(s),'first_level':levels[0],'last_level':levels[-1],'last_exposure':63/64*23-11,'order':'R fastest; G next; B slowest; strip x=R,y=64*B+G; alpha=1'}
meta = json.loads((BASE/'reference-metadata.json').read_text())
root = BASE/'blender-3.4.1-linux-x64'
files = {'config.ocio':'3.4/datafiles/colormanagement/config.ocio',**{k:'3.4/datafiles/colormanagement/'+k for k in meta['candidate']['input_blobs']},'license':'license/OpenColorIO.txt'}
report['input_assets'] = []
for key, rel in files.items():
    d = (root/rel).read_bytes()
    expected = meta['candidate']['config_blob'] if key == 'config.ocio' else meta['candidate']['license_blob'] if key == 'license' else meta['candidate']['input_blobs'][key]
    assert blob(d) == expected
    report['input_assets'].append({'path':rel,'bytes':len(d),'sha256':sha(d),'git_blob':blob(d),'matches_recorded_upstream_blob':blob(d) == expected})
report['reviewed_files'] = {}
for name in ['stimulus.rs','reproduce.py','run_pinned.py','verify_reproduction.py','test_comparison.py','bevy-recipe.txt','gist-evidence/stimulus-0.txt','gist-evidence/conversion-0.txt']:
    d = (BASE/name).read_bytes()
    report['reviewed_files'][name] = {'sha256':sha(d),'git_blob':blob(d),'bytes':len(d)}
archive = BASE/'blender-3.4.1-linux-x64.tar.xz'
provenance = json.loads((BASE/'blender-3.4.1-provenance.json').read_text())
archive_hash = sha(archive.read_bytes())
report['evaluator'] = {'archive_sha256':archive_hash,'matches_prior_official_checksum':archive_hash == provenance['archive_sha256'],'executable_sha256':sha((root/'blender').read_bytes()),'production_build':'55485cb379f7','source_tag_commit':'ef9ca44dee7fe3e25089dbfc49c69e9eff83ba5a','relevant_data_blob_identity_confirmed':True,'historical_bevy_evaluator_established':False}
assert report['evaluator']['matches_prior_official_checksum']
report['evaluator']['matches_recorded_executable'] = report['evaluator']['executable_sha256'] == provenance['executable_sha256']
print(json.dumps(report, indent=2, sort_keys=True))
