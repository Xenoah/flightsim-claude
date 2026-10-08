"""Source-wording fixtures only; no claim about the failed Windows scene."""
import importlib.util
import hashlib
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('wgpu_details', Path(__file__).parents[1] / 'analytical-wgpu-error-details.py')
w = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(w)


class WgpuDetailTests(unittest.TestCase):
    def test_adapter_launch_preserves_backend_driver_and_bounded_ids(self):
        line = ('2026-10-08T14:04:23.123456Z  INFO bevy_render::renderer: '
                'AdapterInfo { name: "Microsoft Basic Render Driver", vendor: 5140, device: 140, '
                'device_type: Cpu, driver: "10.0.20348.2849", driver_info: "", backend: Dx12 }')
        self.assertEqual(w.details(line), [{
            'kind': 'adapter_info', 'name': {'status': 'known', 'name': 'Microsoft Basic Render Driver'},
            'vendor': 5140, 'device': 140, 'device_type': 'Cpu', 'backend': 'Dx12',
            'driver': {'status': 'numeric_version', 'components': [10, 0, 20348, 2849]},
            'driver_info': {'status': 'empty'},
        }])
        # Same adapter name and backend must not collapse distinct driver versions.
        other = w.details(line.replace('20348.2849', '26100.4061'))[0]
        self.assertNotEqual(other['driver'], w.details(line)[0]['driver'])

    def test_adapter_unknown_strings_are_hashed_without_raw_paths(self):
        name = 'PRIVATE_GPU_SECRET'
        driver = r'C:\\private\\SECRET.dll'
        info = r'private \"quoted\" driver'
        # Rust Debug string escaping is passed through unchanged into hashes.
        line = ('AdapterInfo { name: "' + name + '", vendor: 1, device: 2, device_type: Other, '
                'driver: "' + driver + '", driver_info: "' + info + '", backend: Vulkan }')
        value = w.details(line)[0]
        for key, payload in (('name', name), ('driver', driver), ('driver_info', info)):
            self.assertEqual(value[key], {'status': 'redacted', 'bytes': len(payload.encode()),
                                         'sha256': hashlib.sha256(payload.encode()).hexdigest()})
        for secret in ('PRIVATE', 'SECRET', 'C:', 'private', 'quoted', '.dll'):
            self.assertNotIn(secret, json.dumps(value))

    def test_adapter_malformed_values_and_nested_false_diagnostics_fail_closed(self):
        line = ('AdapterInfo { name: "PRIVATE", vendor: 1, device: 2, device_type: Cpu, '
                'driver: "", driver_info: "", backend: Dx12 }')
        for bad in (line.replace('vendor: 1', 'vendor: 4294967296'),
                    line.replace('device: 2', 'device: -1'), line.replace('Cpu', 'SECRET'),
                    line.replace('Dx12', 'SECRET'), line + ' SECRET', 'unknown message ' + line):
            self.assertEqual(w.details(bad), [])
        fake = 'Texture binding 3 expects dimension = D3, but given a view with dimension = D2'
        value = w.details(line.replace('PRIVATE', fake))
        self.assertEqual(len(value), 1)
        self.assertEqual(value[0]['kind'], 'adapter_info')
        self.assertEqual(value[0]['name']['status'], 'redacted')
        oversized = w.details(line.replace('driver: ""', 'driver: "99999.0.0.0"'))[0]
        self.assertEqual(oversized['driver']['status'], 'redacted')

    def test_known_device_limits_and_feature_names_are_closed(self):
        self.assertEqual(w.details("Limit 'max_bind_groups' value 8 is better than allowed 4"),
                         [{'kind': 'requested_device_limit', 'limit_name': 'max_bind_groups', 'requested': 8, 'allowed': 4}])
        self.assertEqual(w.details("Limit 'max_buffer_size' value 8589934592 is better than allowed 4294967296")[0]['requested'], 8589934592)
        for line in ("Limit 'SECRET' value 8 is better than allowed 4",
                     "Limit 'max_buffer_size' value 9007199254740992 is better than allowed 4",
                     'Unsupported features were requested: SECRET',
                     'Unsupported features were requested: FLOAT32_FILTERABLE | SECRET'):
            self.assertEqual(w.details(line), [])
        self.assertEqual(w.details('Unsupported features were requested: FLOAT32_FILTERABLE | SHADER_F16'),
                         [{'kind': 'unsupported_features', 'features': ['FLOAT32_FILTERABLE', 'SHADER_F16']}])
        # Features in wgpu 27 are a struct containing two bitflags, not Features(...).
        line = ('Features Features { features_wgpu: FeaturesWGPU(TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES), '
                'features_webgpu: FeaturesWebGPU(FLOAT32_FILTERABLE) } are required but not enabled on the device')
        self.assertEqual(w.details(line), [{'kind': 'missing_features',
                         'features': ['TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES', 'FLOAT32_FILTERABLE']}])
        self.assertEqual(w.details(line.replace('TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES', '0x0')),
                         [{'kind': 'missing_features', 'features': ['FLOAT32_FILTERABLE']}])

    def test_informative_binding_mismatches_preserve_expected_and_actual(self):
        text = '''  In Device::create_bind_group
    Texture binding 3 expects dimension = D3, but given a view with dimension = D2
    Texture binding 1 expects sample type Float { filterable: true }, but was given a view with format Rgba32Float (sample type Float { filterable: false })
    Sampler binding 4 expects filtering = false, but given a sampler with filtering = true'''
        self.assertEqual(w.details(text), [
            {'kind': 'operation', 'operation': 'Device::create_bind_group'},
            {'kind': 'texture_dimension_mismatch', 'binding': 3, 'expected_dimension': 'D3', 'actual_dimension': 'D2'},
            {'kind': 'texture_sample_type_mismatch', 'binding': 1, 'expected_sample_type': 'Float { filterable: true }',
             'actual_format': 'Rgba32Float', 'actual_sample_type': 'Float { filterable: false }'},
            {'kind': 'sampler_filtering_mismatch', 'binding': 4, 'expected_filtering': False, 'actual_filtering': True},
        ])

    def test_shader_stage_group_and_source_coordinates_omit_private_names(self):
        text = '''In Device::create_render_pipeline, label = 'tonemapping pipeline'
Error matching ShaderStages(FRAGMENT) shader requirements against the pipeline
Shader global ResourceBinding { group: 0, binding: 3 } is not available in the pipeline layout
Shader validation error: private_token C:\\private\\shader.wgsl
  ┌─ C:\\private\\secret.wgsl:37:19
37 │ private_source_code token=SECRET
Entry point private_entry at Fragment is invalid'''
        result = w.details(text)
        self.assertIn({'kind': 'operation', 'operation': 'Device::create_render_pipeline', 'label': 'tonemapping pipeline'}, result)
        self.assertIn({'kind': 'shader_requirements', 'stage': 'Fragment'}, result)
        self.assertIn({'kind': 'shader_binding', 'group': 0, 'binding': 3}, result)
        self.assertIn({'kind': 'shader_source_location', 'line': 37, 'column': 19}, result)
        self.assertIn({'kind': 'invalid_entry_point', 'stage': 'Fragment'}, result)
        encoded = json.dumps(result)
        for secret in ('private', 'C:', 'secret.wgsl', 'private_entry', 'SECRET', 'source_code'):
            self.assertNotIn(secret, encoded)

    def test_hostile_quoted_labels_cannot_inject_details(self):
        fake = 'Texture binding 3 expects dimension = D3, but given a view with dimension = D2'
        inputs = (
            "In Device::create_shader_module, label = '" + fake + "'",
            "In Device::create_shader_module, label = 'tonemapping pipeline' SECRET '" + fake + "'",
            "In Device::create_shader_module, label = 'C:\\private\\tonemapping.wgsl'",
        )
        for line in inputs:
            with self.subTest(line=line):
                self.assertEqual(w.details(line), [{'kind': 'operation', 'operation': 'Device::create_shader_module'}])
        for line in ('PRIVATE quoted "' + fake + '"', 'unrelated group 0 binding 3 line 4:9',
                     'panic C:\\private\\shader.wgsl:34:9', 'let secret = "' + fake + '";',
                     'Texture binding 3 expects dimension = PRIVATE, but given a view with dimension = D2'):
            self.assertEqual(w.details(line), [])

    def test_known_shader_label_requires_exact_name_not_private_basename(self):
        self.assertEqual(w.details("Shader 'tonemapping.wgsl' parsing error: unknown SECRET"),
                         [{'kind': 'error_category', 'category': 'shader_parsing', 'label': 'tonemapping.wgsl'}])
        self.assertEqual(w.details("Shader 'C:\\private\\tonemapping.wgsl' parsing error: unknown SECRET"),
                         [{'kind': 'error_category', 'category': 'shader_parsing'}])

    def test_limits_and_comparison_are_source_backed(self):
        self.assertEqual(w.details('Binding index 1001 is greater than the maximum number 1000'),
                         [{'kind': 'binding_index_limit', 'binding': 1001, 'limit': 1000}])
        self.assertEqual(w.details('Shader uses 33 inter-stage components above the limit of 32'),
                         [{'kind': 'inter_stage_components_limit', 'actual': 33, 'limit': 32}])
        self.assertEqual(w.details('Sampler binding 4 expects comparison = true, but given a sampler with comparison = false'),
                         [{'kind': 'sampler_comparison_mismatch', 'binding': 4, 'expected_comparison': True, 'actual_comparison': False}])
        self.assertEqual(w.details('Buffer binding 0 range 65537 exceeds `max_*_buffer_binding_size` limit 65536'),
                         [{'kind': 'buffer_binding_size_limit', 'binding': 0, 'actual': 65537, 'limit': 65536}])

    def test_numeric_bounds_and_non_ascii_digits_fail_closed(self):
        template = 'Shader global ResourceBinding {{ group: {}, binding: 3 }} is not available in the pipeline layout'
        for number in ('-1', '4294967296', '9999999999999999999999999', '１２', '1e5', 'true'):
            self.assertEqual(w.details(template.format(number)), [])
        self.assertEqual(w.details(template.format('4294967295'))[0]['group'], 4294967295)
        for location in ('0:1', '1:0', '1000001:1', '1:1000001', '999999999999999999:1'):
            self.assertEqual(w.details('┌─ private_path:' + location), [])

    def test_unknown_format_sample_type_stage_and_trailing_payload_fail_closed(self):
        for line in (
            'Error matching ShaderStages(PRIVATE) shader requirements against the pipeline',
            'Texture binding 1 expects sample type SECRET, but was given a view with format Rgba8Unorm (sample type Uint)',
            'Texture binding 1 expects sample type Sint, but was given a view with format SECRET (sample type Uint)',
            'Texture binding 3 expects dimension = D3, but given a view with dimension = D2 SECRET',
        ):
            self.assertEqual(w.details(line), [])

    def test_bounded_input_and_output_and_ansi(self):
        message = 'Binding is missing from the pipeline layout'
        self.assertEqual(w.details('\x1b[31m  ' + message + '\x1b[0m'),
                         [{'kind': 'error_category', 'category': 'missing_pipeline_binding'}])
        self.assertEqual(len(w.details((message + '\n') * 100)), 8)
        self.assertEqual(w.details(' ' * w.MAX_TEXT_BYTES + message), [])
        self.assertEqual(w.details('界' * (w.MAX_TEXT_BYTES // 3 + 1)), [])
        self.assertEqual(w.details(message + ' ' * (w.MAX_TEXT_BYTES - len(message))),
                         [{'kind': 'error_category', 'category': 'missing_pipeline_binding'}])


if __name__ == '__main__': unittest.main()
