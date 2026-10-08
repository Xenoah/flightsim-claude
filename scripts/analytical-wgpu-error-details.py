#!/usr/bin/env python3
"""Closed, bounded observations of wgpu diagnostics, never a root-cause claim.

Templates were checked against the locked dependency sources:
* wgpu-core 27.0.3: src/error.rs, src/binding_model.rs, src/validation.rs,
  src/pipeline.rs, src/device/mod.rs, src/instance.rs
* wgpu 27.0.1: src/backend/wgpu_core.rs (operation names/error formatting)
* naga 27.0.3: src/error.rs, src/valid/mod.rs, src/front/wgsl/tests.rs
* wgpu-types 27.0.1: src/{lib,features}.rs (Debug enums and adapter fields)
* wgpu-hal 27.0.4: src/dx12/adapter.rs (four-component driver version)
* bevy_render 0.18.1: src/renderer/mod.rs (AdapterInfo launch log)
* bevy_core_pipeline 0.18.1: src/tonemapping/{mod,node}.rs (known labels)

Unknown diagnostic text, source code, paths, entry-point names, and labels are
never returned. String values are literal allowlist tokens or SHA256 digests;
unknown adapter strings are represented by digests of their Rust Debug payload.
Only bounded numbers and booleans are converted from input. Exact templates
lose detail for unfamiliar wording. Recognition is evidence of text, not proof
of its origin or of why the process failed. The caller binds it to private logs.
"""
from __future__ import annotations

import hashlib
import re

MAX_TEXT_BYTES = 64 * 1024
MAX_DETAILS = 8
MAX_U32 = 2**32 - 1
MAX_SOURCE_COORDINATE = 1_000_000
ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')
NUMBER = r'[0-9]{1,10}'

OPERATIONS = (
    'Device::create_render_pipeline', 'Device::create_compute_pipeline',
    'Device::create_shader_module', 'Device::create_bind_group',
    'Device::create_bind_group_layout', 'Device::create_pipeline_layout',
    'Device::create_texture', 'Device::create_sampler',
)
KNOWN_LABELS = (
    'tonemapping pipeline', 'tonemapping_hdr_texture_bind_group_layout',
    'Tonemapping LUT sampler', 'tonemapping', 'tonemapping.wgsl',
)
DIMENSIONS = ('D1', 'D2', 'D2Array', 'Cube', 'CubeArray', 'D3')
SAMPLE_TYPES = ('Float { filterable: true }', 'Float { filterable: false }',
                'Depth', 'Sint', 'Uint')
# This is intentionally a reviewed subset, not a parser for arbitrary Debug text.
FORMATS = ('R8Unorm', 'R8Snorm', 'R8Uint', 'R8Sint', 'R16Uint', 'R16Sint',
           'R16Float', 'Rg8Unorm', 'Rg8Snorm', 'Rg8Uint', 'Rg8Sint',
           'R32Uint', 'R32Sint', 'R32Float', 'Rg16Uint', 'Rg16Sint',
           'Rg16Float', 'Rgba8Unorm', 'Rgba8UnormSrgb', 'Rgba8Snorm',
           'Rgba8Uint', 'Rgba8Sint', 'Bgra8Unorm', 'Bgra8UnormSrgb',
           'Rgb10a2Unorm', 'Rg11b10Ufloat', 'Rg32Uint', 'Rg32Sint',
           'Rg32Float', 'Rgba16Uint', 'Rgba16Sint', 'Rgba16Float',
           'Rgba32Uint', 'Rgba32Sint', 'Rgba32Float', 'Stencil8',
           'Depth16Unorm', 'Depth24Plus', 'Depth24PlusStencil8',
           'Depth32Float', 'Depth32FloatStencil8')
BINDING_TYPES = ('Buffer', 'Texture', 'Sampler', 'AccelerationStructure', 'ExternalTexture')
STAGES = {'ShaderStages(VERTEX)': 'Vertex', 'ShaderStages(FRAGMENT)': 'Fragment',
          'ShaderStages(COMPUTE)': 'Compute', 'ShaderStages(TASK)': 'Task',
          'ShaderStages(MESH)': 'Mesh'}
BACKENDS = ('Noop', 'Vulkan', 'Metal', 'Dx12', 'Gl', 'BrowserWebGpu')
DEVICE_TYPES = ('Other', 'IntegratedGpu', 'DiscreteGpu', 'VirtualGpu', 'Cpu')
KNOWN_ADAPTER_NAMES = ('Microsoft Basic Render Driver',)
FEATURES = ('TEXTURE_FORMAT_16BIT_NORM', 'TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES',
            'TEXTURE_BINDING_ARRAY', 'BUFFER_BINDING_ARRAY', 'STORAGE_RESOURCE_BINDING_ARRAY',
            'SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING',
            'STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING', 'PARTIALLY_BOUND_BINDING_ARRAY',
            'UNIFORM_BUFFER_BINDING_ARRAYS', 'PUSH_CONSTANTS', 'SHADER_F16', 'SHADER_F64',
            'BGRA8UNORM_STORAGE', 'FLOAT32_FILTERABLE', 'DUAL_SOURCE_BLENDING',
            'TEXTURE_COMPRESSION_BC', 'TEXTURE_COMPRESSION_BC_SLICED_3D')
LIMITS = ('max_texture_dimension_1d', 'max_texture_dimension_2d', 'max_texture_dimension_3d',
          'max_texture_array_layers', 'max_bind_groups', 'max_bindings_per_bind_group',
          'max_dynamic_uniform_buffers_per_pipeline_layout', 'max_dynamic_storage_buffers_per_pipeline_layout',
          'max_sampled_textures_per_shader_stage', 'max_samplers_per_shader_stage',
          'max_storage_buffers_per_shader_stage', 'max_storage_textures_per_shader_stage',
          'max_uniform_buffers_per_shader_stage', 'max_binding_array_elements_per_shader_stage',
          'max_binding_array_sampler_elements_per_shader_stage', 'max_uniform_buffer_binding_size',
          'max_storage_buffer_binding_size', 'max_vertex_buffers', 'max_buffer_size',
          'max_vertex_attributes', 'max_vertex_buffer_array_stride', 'min_uniform_buffer_offset_alignment',
          'min_storage_buffer_offset_alignment', 'max_inter_stage_shader_components',
          'max_color_attachments', 'max_color_attachment_bytes_per_sample',
          'max_compute_workgroup_storage_size', 'max_compute_invocations_per_workgroup',
          'max_compute_workgroup_size_x', 'max_compute_workgroup_size_y', 'max_compute_workgroup_size_z',
          'max_compute_workgroups_per_dimension', 'max_push_constant_size', 'max_non_sampler_bindings')

# Optional fixed-fragment counts for the outer projector. These are observations,
# and must not be used to infer that adjacent arbitrary numbers are GPU indices.
FIXED_SIGNATURES = {
    'shader_requirements': ('shader_validation', 'shader requirements against the pipeline'),
    'missing_pipeline_binding': ('gpu_binding', 'Binding is missing from the pipeline layout'),
    'binding_visibility': ('gpu_binding', "Visibility flags don't include the shader stage"),
    'shader_validation_error': ('shader_validation', 'Shader validation error:'),
    'integer_texture_filtering': ('gpu_binding', "Integer textures can't be sampled with a filtering sampler"),
    'float_texture_filtering': ('gpu_binding', "Non-filterable float textures can't be sampled with a filtering sampler"),
}
EXACT_CATEGORIES = {
    'wgpu error: Validation Error': 'validation',
    'Validation Error': 'validation',
    'Binding is missing from the pipeline layout': 'missing_pipeline_binding',
    "Visibility flags don't include the shader stage": 'binding_visibility',
    "Comparison flag doesn't match the shader": 'sampler_comparison',
    'Derived bind group layout type is not consistent between stages': 'inconsistent_binding_stages',
    "Integer textures can't be sampled with a filtering sampler": 'integer_texture_filtering',
    "Non-filterable float textures can't be sampled with a filtering sampler": 'float_texture_filtering',
    'Error matching shader requirements against the pipeline': 'shader_requirements',
    'Failed to generate the backend-specific code': 'shader_backend_generation',
    'Array binding provided zero elements': 'empty_binding_array',
}


def alternatives(values):
    return '(?:' + '|'.join(re.escape(value) for value in values) + ')'


def enum(values):
    # Return the allowlist's value, never a free-form captured string.
    tokens = {value: value for value in values}
    return tokens.__getitem__


def integer(value):
    result = int(value)
    if result > MAX_U32:
        raise ValueError('out-of-range diagnostic integer')
    return result


def coordinate(value):
    result = integer(value)
    if not 1 <= result <= MAX_SOURCE_COORDINATE:
        raise ValueError('out-of-range source coordinate')
    return result


def safe_integer(value):
    result = int(value)
    if result > 2**53 - 1:
        raise ValueError('diagnostic integer exceeds exact JSON number range')
    return result


def boolean(value):
    return {'true': True, 'false': False}[value]


# No unescaping is necessary to compare these records: the digest binds the exact
# Debug string payload bytes. In particular, escaped paths remain private.
DEBUG_STRING = r'"((?:[^"\\\r\n]|\\(?:["\\nrt0]|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}))*)"'
LOG_PREFIX = r'(?:(?:[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:.]+Z +)?INFO +bevy_render::renderer: +)?'
ADAPTER = re.compile(LOG_PREFIX + r'AdapterInfo \{ name: ' + DEBUG_STRING +
                     r', vendor: (' + NUMBER + r'), device: (' + NUMBER +
                     r'), device_type: (' + alternatives(DEVICE_TYPES) + r'), driver: ' + DEBUG_STRING +
                     r', driver_info: ' + DEBUG_STRING + r', backend: (' + alternatives(BACKENDS) + r') \}')
FEATURE_NAMES = alternatives(FEATURES) + r'(?: \| ' + alternatives(FEATURES) + r'){0,7}'
UNSUPPORTED_FEATURES = re.compile(r'Unsupported features were requested: (' + FEATURE_NAMES + r')')
MISSING_FEATURES = re.compile(r'Features Features \{ features_wgpu: FeaturesWGPU\((0x0|' + FEATURE_NAMES +
                              r')\), features_webgpu: FeaturesWebGPU\((0x0|' + FEATURE_NAMES +
                              r')\) \} are required but not enabled on the device')


def adapter_string(value, *, adapter_name=False):
    if value == '':
        return {'status': 'empty'}
    if adapter_name:
        for name in KNOWN_ADAPTER_NAMES:
            if value == name:
                return {'status': 'known', 'name': name}
    elif re.fullmatch(r'[0-9]{1,5}(?:\.[0-9]{1,5}){3}', value):
        parts = [int(part) for part in value.split('.')]
        if all(part <= 65535 for part in parts):
            return {'status': 'numeric_version', 'components': parts}
    raw = value.encode('utf-8', errors='replace')
    return {'status': 'redacted', 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def adapter_details(line):
    match = ADAPTER.fullmatch(line)
    if match is None:
        return None
    name, vendor, device, device_type, driver, driver_info, backend = match.groups()
    try:
        return {'kind': 'adapter_info', 'name': adapter_string(name, adapter_name=True),
                'vendor': integer(vendor), 'device': integer(device),
                'device_type': enum(DEVICE_TYPES)(device_type), 'backend': enum(BACKENDS)(backend),
                'driver': adapter_string(driver), 'driver_info': adapter_string(driver_info)}
    except ValueError:
        return None


def feature_details(line):
    match = UNSUPPORTED_FEATURES.fullmatch(line)
    kind = 'unsupported_features'
    if match is None:
        match = MISSING_FEATURES.fullmatch(line)
        kind = 'missing_features'
    if match is None:
        return None
    names = [name for group in match.groups() if group != '0x0' for name in group.split(' | ')]
    if not 1 <= len(names) <= 8:
        return None
    return {'kind': kind, 'features': [enum(FEATURES)(name) for name in names]}


# Each rule fixes both the complete output schema and all possible string values.
# Numeric groups occur only in their actual source-backed sentence position.
RULES = []


def rule(pattern, kind, fields):
    RULES.append((re.compile(pattern), kind, fields))


rule(r'Error matching (' + alternatives(STAGES) + r') shader requirements against the pipeline',
     'shader_requirements', (('stage', STAGES.__getitem__),))
rule(r'Entry point [^\r\n]* at (Vertex|Fragment|Compute|Task|Mesh) is invalid',
     'invalid_entry_point', (('stage', enum(('Vertex', 'Fragment', 'Compute', 'Task', 'Mesh'))),))
rule(r'┌─ [^\r\n]*:(' + NUMBER + r'):(' + NUMBER + r')',
     'shader_source_location', (('line', coordinate), ('column', coordinate)))
rule(r'Shader global ResourceBinding \{ group: (' + NUMBER + r'), binding: (' + NUMBER + r') \} is not available in the pipeline layout',
     'shader_binding', (('group', integer), ('binding', integer)))
rule(r'Unable to filter the texture \(ResourceBinding \{ group: (' + NUMBER + r'), binding: (' + NUMBER + r') \}\) by the sampler \(ResourceBinding \{ group: (' + NUMBER + r'), binding: (' + NUMBER + r') \}\)',
     'texture_sampler_binding', (('texture_group', integer), ('texture_binding', integer),
                                ('sampler_group', integer), ('sampler_binding', integer)))
rule(r'Texture binding (' + NUMBER + r') expects dimension = (' + alternatives(DIMENSIONS) + r'), but given a view with dimension = (' + alternatives(DIMENSIONS) + r')',
     'texture_dimension_mismatch', (('binding', integer), ('expected_dimension', enum(DIMENSIONS)),
                                    ('actual_dimension', enum(DIMENSIONS))))
rule(r'Texture binding (' + NUMBER + r') expects sample type (' + alternatives(SAMPLE_TYPES) + r'), but was given a view with format (' + alternatives(FORMATS) + r') \(sample type (' + alternatives(SAMPLE_TYPES) + r')\)',
     'texture_sample_type_mismatch', (('binding', integer), ('expected_sample_type', enum(SAMPLE_TYPES)),
                                      ('actual_format', enum(FORMATS)), ('actual_sample_type', enum(SAMPLE_TYPES))))
rule(r'Storage texture binding (' + NUMBER + r') expects format = (' + alternatives(FORMATS) + r'), but given a view with format = (' + alternatives(FORMATS) + r')',
     'storage_texture_format_mismatch', (('binding', integer), ('expected_format', enum(FORMATS)),
                                         ('actual_format', enum(FORMATS))))
rule(r'Texture binding (' + NUMBER + r') expects multisampled = (true|false), but given a view with samples = (' + NUMBER + r')',
     'texture_multisample_mismatch', (('binding', integer), ('expected_multisampled', boolean),
                                      ('actual_samples', integer)))
for subject in ('filtering', 'comparison'):
    rule(r'Sampler binding (' + NUMBER + r') expects ' + subject + r' = (true|false), but given a sampler with ' + subject + r' = (true|false)',
         'sampler_' + subject + '_mismatch', (('binding', integer), ('expected_' + subject, boolean),
                                             ('actual_' + subject, boolean)))
rule(r'Type on the shader side \((' + alternatives(BINDING_TYPES) + r')\) does not match the pipeline binding \((' + alternatives(BINDING_TYPES) + r')\)',
     'shader_binding_type_mismatch', (('shader_type', enum(BINDING_TYPES)), ('layout_type', enum(BINDING_TYPES))))
rule(r'Binding index (' + NUMBER + r') is greater than the maximum number (' + NUMBER + r')',
     'binding_index_limit', (('binding', integer), ('limit', integer)))
rule(r'Bind group layout count (' + NUMBER + r') exceeds device bind group limit (' + NUMBER + r')',
     'bind_group_count_limit', (('actual', integer), ('limit', integer)))
rule(r'Shader uses (' + NUMBER + r') inter-stage components above the limit of (' + NUMBER + r')',
     'inter_stage_components_limit', (('actual', integer), ('limit', integer)))
rule(r'Buffer binding (' + NUMBER + r') range (' + NUMBER + r') exceeds `max_\*_buffer_binding_size` limit (' + NUMBER + r')',
     'buffer_binding_size_limit', (('binding', integer), ('actual', integer), ('limit', integer)))
rule(r'Number of bindings in bind group descriptor \((' + NUMBER + r')\) does not match the number of bindings defined in the bind group layout \((' + NUMBER + r')\)',
     'binding_count_mismatch', (('actual', integer), ('expected', integer)))
rule(r"Limit '(" + alternatives(LIMITS) + r")' value ([0-9]{1,16}) is better than allowed ([0-9]{1,16})",
     'requested_device_limit', (('limit_name', enum(LIMITS)), ('requested', safe_integer), ('allowed', safe_integer)))
rule(r"Buffer offset (" + NUMBER + r") does not respect device's requested `(min_uniform_buffer_offset_alignment|min_storage_buffer_offset_alignment)` limit (" + NUMBER + r')',
     'buffer_offset_alignment', (('actual', integer), ('limit_name', enum(LIMITS)), ('limit', integer)))


def line_details(line):
    adapter = adapter_details(line)
    if adapter is not None:
        return adapter
    features = feature_details(line)
    if features is not None:
        return features
    category = EXACT_CATEGORIES.get(line)
    if category:
        return {'kind': 'error_category', 'category': category}
    for operation in OPERATIONS:
        prefix = 'In ' + operation
        if line == prefix:
            return {'kind': 'operation', 'operation': operation}
        # Consume the whole label as an opaque suffix. Never search inside it.
        if line.startswith(prefix + ", label = '") and line.endswith("'"):
            item = {'kind': 'operation', 'operation': operation}
            for label in KNOWN_LABELS:
                if line == prefix + ", label = '" + label + "'":
                    item['label'] = label
                    break
            return item
    # ShaderError display prefixes contain arbitrary shader names/diagnostic text.
    # The fixed prefix is enough for a category; its payload is never inspected.
    if line.startswith('Shader validation error:'):
        return {'kind': 'error_category', 'category': 'shader_validation'}
    if re.fullmatch(r"Shader '[^'\r\n]*' parsing error:.*", line):
        item = {'kind': 'error_category', 'category': 'shader_parsing'}
        for label in KNOWN_LABELS:
            if line.startswith("Shader '" + label + "' parsing error:"):
                item['label'] = label
                break
        return item
    for pattern, kind, fields in RULES:
        match = pattern.fullmatch(line)
        if match:
            try:
                return {'kind': kind, **{name: convert(value) for (name, convert), value
                                         in zip(fields, match.groups())}}
            except (ValueError, KeyError):
                return None
    return None


def details(text: str) -> list[dict]:
    """Observe at most eight details in at most 64 KiB of UTF-8 text.

    Oversized input fails closed, without scanning a truncated diagnostic. The
    stream owner must supply bounded original chunks and retain their hashes.
    Multiline input is accepted, with the same eight-item cap for the whole call.
    """
    if not isinstance(text, str):
        raise TypeError('diagnostic input must be text')
    if len(text) > MAX_TEXT_BYTES or len(text.encode('utf-8', errors='replace')) > MAX_TEXT_BYTES:
        return []
    result = []
    for line in ANSI.sub('', text).splitlines():
        item = line_details(line.strip())
        if item is not None:
            result.append(item)
            if len(result) == MAX_DETAILS:
                break
    return result
