"""Synthetic adversarial plumbing only; no Windows build, review or acceptance."""
import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import struct
import unittest
from unittest.mock import patch


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HERE = Path(__file__).resolve().parent
native = load('ordinary_native_test', HERE.parent / 'project-ordinary-native-evidence.py')
payload_tests = load('ordinary_native_payload_fixtures', HERE / 'test_ordinary_release_payload.py')
runtime_tests = load('ordinary_native_runtime_fixtures', HERE / 'test_analytical_runtime_facts.py')


def synthetic_pe():
    # Independent PE32+ field-layout fixture. This is deliberately not runnable.
    raw = bytearray(1024)
    raw[:2] = b'MZ'
    struct.pack_into('<I', raw, 0x3c, 0x80)
    raw[0x80:0x84] = b'PE\0\0'
    struct.pack_into('<HHIIIHH', raw, 0x84, 0x8664, 1, 0, 0, 0, 240, 0x22)
    struct.pack_into('<H', raw, 0x98, 0x20b)
    struct.pack_into('<I', raw, 0x98 + 60, 0x200)
    struct.pack_into('<H', raw, 0x98 + 68, 3)
    struct.pack_into('<I', raw, 0x98 + 108, 16)
    struct.pack_into('<IIII', raw, 0x98 + 240 + 8, 0x200, 0x1000, 0x200, 0x200)
    return raw


class OrdinaryNativeTests(unittest.TestCase):
    def setUp(self):
        self.fixture = payload_tests.OrdinaryPayloadTests(methodName='runTest')
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        f = self.fixture
        self.repo, self.root = f.repo, f.root
        self.private, self.text, self.bundle = f.private, f.text, f.bundle
        self.runtime_fixture = runtime_tests.RuntimeFactsTests(methodName='runTest')
        with patch.object(runtime_tests.tempfile, 'tempdir', str(Path(runtime_tests.tempfile.gettempdir()).resolve())):
            self.runtime_fixture.setUp()
        self.addCleanup(self.runtime_fixture.doCleanups)
        self.runtime_private = self.runtime_fixture.private
        self.directory = self.private / 'capture/ordinary'
        self.compiler = self.directory / 'rustc.txt'
        self.compiler.write_text(self.runtime_fixture.outputs['rustc-version'])
        self.trace = self.directory / 'build.stderr'
        self.trace.write_text('SYNTHETIC empty build trace, not native proof\n')
        self.graph = self.directory / 'graph.txt'
        self.graph.write_text('flightsim-app v1.2.3 features=[default]\n')
        self.messages = self.directory / 'messages.jsonl'
        self.messages.write_text(json.dumps({'reason': 'build-script-executed',
            'package_id': 'synthetic-app', 'linked_libs': ['user32']}) + '\n')
        f.executable.write_bytes(synthetic_pe())
        self.package = {'id': 'synthetic-app', 'name': 'flightsim-app', 'version': '1.2.3',
                        'source': None, 'manifest_path': str(self.repo / 'Cargo.toml')}
        self.metadata = {'packages': [self.package], 'workspace_members': ['synthetic-app'],
                         'resolve': {'nodes': [{'id': 'synthetic-app', 'features': ['default'], 'deps': []}]}}
        f.metadata.write_text(json.dumps(self.metadata))
        inv = f.fixture.inventory
        inv['metadata_sha256'] = native.capture.file_record(f.metadata)['sha256']
        row = inv['packages'][0]
        row.update(source='workspace', source_checksum=None, source_revision=None, license_expression='MIT')
        row['notices'][0]['upstream_path'] = 'LICENSE'
        inv['embedded_assets'] = [
            {'id': name, 'package': 'bevy_core_pipeline' if 'fira' not in name else 'bevy_text',
             'version': '0.18.1', 'feature': 'tonemapping_luts' if 'fira' not in name else 'default_font',
             'source_path': 'synthetic/' + name, 'sha256': '1' * 64, 'observed_sha256': '1' * 64,
             'notices': [], 'review_state': 'original_source_recorded'} for name in sorted(native.EMBEDDED_IDS)]
        f.fixture.write('scripts/tonemapping-two-lut-source.json', '{"synthetic":true}')
        f.fixture.refresh_evidence()
        f.fixture.authorize_fixture()
        f.expected = f.fixture.git('rev-parse', 'HEAD').stdout.decode().strip()
        self.expected = f.expected
        f.source = {'source_sha': self.expected, 'source_tree': f.fixture.git('rev-parse', 'HEAD^{tree}').stdout.decode().strip()}
        f.source['files'] = []
        for name in f.fixture.git('ls-files').stdout.decode().splitlines():
            binding = native.capture.file_record(self.repo / name)
            f.source['files'].append({'path': name, 'git_mode': '100644',
                'checkout_sha256': binding['sha256'], 'checkout_bytes': binding['bytes'], 'canonical_git_blob': '0' * 40})
        shutil.copytree(f.original.parent, f.notices, dirs_exist_ok=True)
        f.refresh_verified()
        f.rebuild_bundle()
        self.collect_runtime()

    def collect_runtime(self):
        if self.runtime_private.exists():
            shutil.rmtree(self.runtime_private)
        value = self.runtime_fixture.collect(linker_trace=self.trace, audited_executable=self.fixture.executable,
            env={'PROGRAMFILES(X86)': str(self.runtime_fixture.program)})
        path = self.runtime_private / native.runtime_facts.PRIVATE_NAME
        manifest = json.loads(path.read_bytes())
        manifest['source_sha'] = self.expected
        path.write_bytes(native.runtime_facts.canonical(manifest))
        value = native.runtime_facts.project(self.runtime_private)
        (self.runtime_private / native.runtime_facts.PROJECTION_NAME).write_bytes(native.runtime_facts.canonical(value))
        self.runtime = value

    @contextlib.contextmanager
    def boundaries(self):
        # Only the genuine native capture and canonical-source admission boundary
        # is mocked. Gate/copy plan, notices, PE, runtime replay and new code run.
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(native.payload.capture, 'validate_export', return_value=self.fixture.verified))
            stack.enter_context(patch.object(native.payload.capture, 'source_evidence', return_value=self.fixture.source))
            stack.enter_context(patch.object(native.capture, 'source_evidence', return_value=self.fixture.source))
            yield

    def project(self):
        with self.boundaries():
            return native.project(self.repo, self.expected, self.private, self.text, self.bundle,
                                  runtime_facts_private=self.runtime_private)

    def test_full_ordinary_facts_bind_real_gate_runtime_and_pe_without_approval(self):
        result = self.project()
        self.assertEqual(result['kind'], native.KIND)
        self.assertEqual(result['source_sha'], self.expected)
        self.assertEqual(result['runtime_facts'], self.runtime)
        self.assertEqual(result['pe']['format'], 'PE32+')
        self.assertEqual(result['packages'][0]['source_kind'], 'workspace')
        self.assertEqual(result['packages'][0]['exact_features'], ['default'])
        self.assertEqual({row['id'] for row in result['embedded_assets']}, native.EMBEDDED_IDS)
        self.assertEqual(result['bindings']['build_trace'], native.capture.file_record(self.trace))
        self.assertEqual(result['bindings']['executable'], self.runtime['final_link']['executable']['record'])
        self.assertEqual(result['inventory_comparison'], result['payload_projection']['inventory_comparison'])
        self.assertEqual(set(result['content_view']['sections']), set(native.CONTENT_SECTIONS))
        self.assertEqual(native.content_view(result), result['content_view'])
        self.assertEqual(result['source_headers']['status'], 'not_established')
        self.assertEqual(result['runtime_coverage']['status'], 'not_established')
        self.assertEqual(result['runtime_facts']['final_link']['reason'], 'missing_fingerprint')
        for key in ('release_authorized', 'dependency_review_approved', 'review_applicability_approved',
                    'runtime_accepted', 'native_runtime_coverage_complete'):
            self.assertIs(result[key], False)
        public = native.payload.canonical(result)
        for private in (str(self.root), str(self.runtime_fixture.root), 'PRIVATE NOTICE', 'PRIVATE ERROR'):
            self.assertNotIn(private.encode(), public)
        self.assertNotIn('ui_capability_states', result)

    def test_metadata_encoding_difference_preserves_both_raw_identities_without_approval(self):
        old = self.project()
        f = self.fixture
        original = f.original.read_bytes()
        f.metadata.write_bytes(f.metadata.read_bytes() + b'\n')
        value = json.loads(f.captured.read_bytes())
        value['metadata_sha256'] = native.capture.file_record(f.metadata)['sha256']
        f.captured.write_text(json.dumps(value))
        f.refresh_verified()
        result = self.project()
        self.assertEqual(result['inventory_comparison']['different_fields'], ['metadata_sha256'])
        self.assertEqual(result['inventory_comparison']['metadata_difference_cause'], 'not_established')
        self.assertNotEqual(result['inventory_comparison']['original'], result['inventory_comparison']['captured'])
        self.assertEqual(result['content_view'], old['content_view'])
        self.assertEqual(f.original.read_bytes(), original)
        self.assertEqual((self.bundle / 'third-party/dependency-inventory.json').read_bytes(), original)
        self.assertFalse(result['review_applicability_approved'])

    def test_runtime_root_is_mandatory_and_disjoint(self):
        with self.assertRaises(TypeError):
            native.project(self.repo, self.expected, self.private, self.text, self.bundle)
        for root in (self.repo, self.private, self.text, self.bundle, self.private / 'nested'):
            with self.subTest(root=root), self.assertRaises(ValueError):
                native.project(self.repo, self.expected, self.private, self.text, self.bundle, runtime_facts_private=root)

    def test_native_validator_and_payload_adapter_cannot_be_skipped(self):
        with self.boundaries(), patch.object(native.payload.capture, 'validate_export', side_effect=ValueError('native rejected')):
            with self.assertRaisesRegex(ValueError, 'native rejected'):
                native.project(self.repo, self.expected, self.private, self.text, self.bundle,
                               runtime_facts_private=self.runtime_private)
        with patch.object(native.payload, 'project_payload', side_effect=ValueError('payload rejected')):
            with self.assertRaisesRegex(ValueError, 'payload rejected'):
                self.project()

    def test_runtime_compiler_different_or_unknown_fails_without_dropping_other_unknowns(self):
        for compiler in (None, self.runtime_fixture.outputs['rustc-version'].replace('release: 1.93.0', 'release: 1.94.0')):
            with self.subTest(compiler=compiler):
                self.runtime_fixture.outputs['rustc-version'] = compiler
                self.collect_runtime()
                with self.assertRaisesRegex(ValueError, 'runtime compiler differs'):
                    self.project()

    def test_wrong_runtime_source_or_original_build_paths_fail(self):
        value = copy.deepcopy(self.runtime)
        value['source_sha'] = 'b' * 40
        with patch.object(native.runtime_facts, 'project', return_value=value):
            with self.assertRaisesRegex(ValueError, 'source/target mismatch'):
                self.project()
        foreign = self.root / 'foreign-trace.stderr'
        foreign.write_bytes(self.trace.read_bytes())
        old_trace = self.trace
        self.trace = foreign
        self.collect_runtime()
        self.trace = old_trace
        with self.assertRaisesRegex(ValueError, 'exact ordinary build trace/executable'):
            self.project()

    def test_runtime_wrong_executable_is_not_allowed_even_when_bytes_equal(self):
        original = self.fixture.executable
        other = self.root / 'foreign-target' / native.check.TARGET / 'release/flightsim-app.exe'
        other.parent.mkdir(parents=True)
        other.write_bytes(original.read_bytes())
        self.fixture.executable = other
        self.collect_runtime()
        self.fixture.executable = original
        with self.assertRaisesRegex(ValueError, 'exact ordinary build trace/executable'):
            self.project()

    def test_changed_installed_runtime_library_and_private_projection_fail(self):
        self.runtime_fixture.rlib.write_bytes(b'changed sysroot content')
        with self.assertRaisesRegex(ValueError, 'installed file changed'):
            self.project()
        self.collect_runtime()
        projection = self.runtime_private / native.runtime_facts.PROJECTION_NAME
        projection.write_bytes(projection.read_bytes() + b' ')
        with self.assertRaisesRegex(ValueError, 'runtime fact projection changed'):
            self.project()

    def test_compiler_graph_messages_runtime_hardlinks_and_symlinks_fail(self):
        for original in (self.compiler, self.graph, self.messages, self.runtime_private / native.runtime_facts.PRIVATE_NAME):
            saved = original.read_bytes()
            external = self.root / 'link-target'
            external.write_bytes(saved)
            original.unlink()
            original.symlink_to(external)
            with self.subTest(path=original), self.assertRaises(ValueError):
                self.project()
            original.unlink()
            original.write_bytes(saved)
        alias = self.root / 'hardlink'
        os.link(self.graph, alias)
        with self.assertRaisesRegex(ValueError, 'independent'):
            self.project()

    def test_duplicate_metadata_id_and_unknown_graph_member_are_not_normalized(self):
        value = copy.deepcopy(self.metadata)
        value['packages'].append(copy.deepcopy(value['packages'][0]))
        with self.assertRaisesRegex(ValueError, 'duplicate Cargo'):
            native.validate_metadata_members(value)
        self.graph.write_text(self.graph.read_text() + 'foreign v9.9.9 features=[]\n')
        with self.assertRaisesRegex(ValueError, 'outside conservative'):
            self.project()

    def test_graph_features_and_build_script_scope_fail_closed(self):
        self.graph.write_text('flightsim-app v1.2.3 features=[default,extra]\n')
        with self.assertRaisesRegex(ValueError, 'omits exact graph features'):
            self.project()
        self.graph.write_text('flightsim-app v1.2.3 features=[default]\n')
        self.messages.write_text('{"reason":"build-script-executed","package_id":"foreign","linked_libs":[]}\n')
        with self.assertRaisesRegex(ValueError, 'outside native closure'):
            self.project()

    def test_duplicate_json_message_and_unbounded_native_text_fail(self):
        self.messages.write_text('{"reason":"other","reason":"build-script-executed"}\n')
        with self.assertRaisesRegex(ValueError, 'duplicate native message'):
            self.project()
        with self.assertRaisesRegex(ValueError, 'byte budget'):
            native.read_text(self.graph, 1)

    def test_source_content_is_complete_for_explicit_prefixes_and_byte_bound(self):
        result = self.project()
        by_path = {row['path']: row for row in result['source_content_files']}
        self.assertTrue(native.SOURCE_REQUIRED <= set(by_path))
        self.assertTrue({row['path'] for row in self.fixture.source['files']
                         if row['path'].startswith(native.SOURCE_PREFIXES)} <= set(by_path))
        self.assertFalse(native.SOURCE_PACKET_EXCLUSIONS & set(by_path))
        self.assertEqual(by_path['Cargo.lock']['sha256'], native.capture.file_record(self.repo / 'Cargo.lock')['sha256'])
        with self.assertRaisesRegex(ValueError, 'canonical source file set'):
            native.source_content_files(self.repo, {**self.fixture.source, 'files': []}, result['payload_projection'], result['packages'])

    def test_pe_malformed_or_bundled_diagnostic_extra_fails(self):
        self.fixture.executable.write_bytes(b'NOT A PE')
        self.fixture.refresh_verified()
        self.fixture.rebuild_bundle()
        self.collect_runtime()
        with self.assertRaisesRegex(ValueError, 'PE file size'):
            self.project()
        self.fixture.executable.write_bytes(synthetic_pe())
        self.fixture.refresh_verified()
        self.fixture.rebuild_bundle()
        self.collect_runtime()
        (self.bundle / 'bundle-manifest.json').write_text('{}')
        with self.assertRaises(ValueError):
            self.project()

    def test_changed_native_text_after_parsing_fails_final_stability_check(self):
        original_pe = native.pe_imports
        for path in (self.graph, self.messages, self.compiler):
            previous = path.read_bytes()
            def mutate(executable):
                result = original_pe(executable)
                path.write_bytes(previous + b'\n')
                return result
            with self.subTest(path=path), patch.object(native, 'pe_imports', side_effect=mutate):
                with self.assertRaisesRegex(ValueError, 'native fact input changed'):
                    self.project()
            path.write_bytes(previous)

    def test_runtime_input_hash_mismatch_and_unobserved_inputs_fail(self):
        for key in ('trace', 'executable'):
            value = copy.deepcopy(self.runtime)
            value['final_link'][key]['record']['sha256'] = 'f' * 64
            with self.subTest(key=key), patch.object(native.runtime_facts, 'project', return_value=value):
                with self.assertRaisesRegex(ValueError, 'runtime build input binding differs'):
                    self.project()
            value = copy.deepcopy(self.runtime)
            value['final_link'][key] = None
            with self.subTest(key=key), patch.object(native.runtime_facts, 'project', return_value=value):
                with self.assertRaisesRegex(ValueError, 'runtime build input binding differs'):
                    self.project()

    def test_revalidated_payload_or_runtime_cannot_change_after_projection(self):
        original = native.payload.project_payload
        calls = 0
        def mutate_payload(*args):
            nonlocal calls
            result = original(*args)
            calls += 1
            if calls == 2:
                result['source_tree'] = 'f' * 40
            return result
        with patch.object(native.payload, 'project_payload', side_effect=mutate_payload):
            with self.assertRaisesRegex(ValueError, 'payload facts changed'):
                self.project()
        original_runtime = native.validate_runtime_facts
        calls = 0
        def mutate_runtime(*args, **kwargs):
            nonlocal calls
            result = original_runtime(*args, **kwargs)
            calls += 1
            if calls == 2:
                result['rust']['identity']['llvm'] = '99.0.0'
            return result
        with patch.object(native, 'validate_runtime_facts', side_effect=mutate_runtime):
            with self.assertRaisesRegex(ValueError, 'runtime facts changed'):
                self.project()

    def test_projection_byte_budget_and_unknown_top_level_fail(self):
        value = self.project()
        with patch.object(native, 'MAX_BYTES', 100):
            with self.assertRaisesRegex(ValueError, 'budget'):
                native.content_view(value)
        value['new_native_obligation'] = 'must not be discarded'
        with self.assertRaisesRegex(ValueError, 'unknown ordinary native'):
            native.content_view(value)

    def test_cli_failure_is_bounded_and_no_private_path_leak(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            status = native.main(['--repo', str(self.repo), '--expected-sha', self.expected,
                                  '--build-private', str(self.private), '--build-text', str(self.text),
                                  '--bundle', str(self.bundle), '--runtime-facts-private', str(self.runtime_private)])
        self.assertEqual(status, 2)  # The synthetic fixture is not real native evidence.
        self.assertNotIn(str(self.root), err.getvalue())


class RuntimeContentTests(unittest.TestCase):
    def setUp(self):
        self.fixture = runtime_tests.RuntimeFactsTests(methodName='runTest')
        with patch.object(runtime_tests.tempfile, 'tempdir', str(Path(runtime_tests.tempfile.gettempdir()).resolve())):
            self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        trace, executable = self.fixture.link_fixture()
        self.value = self.fixture.collect(linker_trace=trace, audited_executable=executable,
            env={'PROGRAMFILES(X86)': str(self.fixture.program)})

    def test_complete_runtime_identity_and_unknowns_survive_content_projection(self):
        before = copy.deepcopy(self.value)
        view = native.runtime_content(self.value)
        self.assertEqual(self.value, before)
        rust = view['runtime_rust']
        self.assertEqual(rust['identity'], self.value['rust']['identity'])
        self.assertEqual(rust['target_rlibs'][0]['record'], self.value['rust']['target_rlibs'][0]['record'])
        self.assertIsNone(rust['actual_build_crt_static'])
        microsoft = view['runtime_microsoft']
        self.assertEqual(microsoft['sdk_candidates'][0]['libraries'][0]['record'],
                         self.value['microsoft']['sdk_candidates'][0]['libraries'][0]['record'])
        self.assertEqual(microsoft['selected_sdk'], 'not_established')
        link = view['runtime_final_link']
        self.assertEqual(link['linker']['record'], self.value['final_link']['linker']['record'])
        self.assertEqual(link['installed_candidate']['product'], 'Microsoft.VisualStudio.Product.Enterprise')
        self.assertEqual(link['installed_candidate']['linker']['record'], link['linker']['record'])
        self.assertEqual(link['requested_libraries'], ['kernel32.lib', 'msvcrt.lib'])
        self.assertEqual(link['rlib_input_names'], [self.fixture.rlib.name])
        self.assertEqual(link['output_association'], self.value['final_link']['output_association'])
        self.assertEqual(link['actual_static_membership'], 'not_established')
        self.assertIs(link['linker_process_execution_observed'], False)
        for name in ('trace', 'executable', 'fingerprint', 'link_output'):
            self.assertEqual(set(link['build_input_observations'][name]), {'status', 'file_version', 'product_version'})
        self.assertEqual(set(view['runtime_query_outcomes'][0]), {'id', 'outcome', 'exit_code'})
        self.assertNotIn('evidence_id', json.dumps(view))
        self.assertNotIn('private_manifest', json.dumps(view))
        self.assertNotIn(str(self.fixture.root), json.dumps(view))

    def test_every_substantive_runtime_file_identity_change_survives(self):
        baseline = native.runtime_content(self.value)
        notice_index = next(index for index, row in enumerate(self.value['rust']['notices'])
                            if row['id'] == 'copyright_library')
        vc_index = next(index for index, row in enumerate(self.value['microsoft']['visual_studio_candidates'][0]['toolsets'][0]['runtime_libraries'])
                        if row['name'] == 'vcruntime.lib')
        paths = [
            ('rust', 'notices', notice_index, 'record'),
            ('rust', 'target_rlibs', 0, 'record'),
            ('microsoft', 'vswhere', 'record'),
            ('microsoft', 'visual_studio_candidates', 0, 'default_toolset_hint', 'record'),
            ('microsoft', 'visual_studio_candidates', 0, 'terms_candidates', 0, 'record'),
            ('microsoft', 'visual_studio_candidates', 0, 'toolsets', 0, 'runtime_libraries', vc_index, 'record'),
            ('microsoft', 'sdk_candidates', 0, 'libraries', 0, 'record'),
            ('microsoft', 'sdk_candidates', 0, 'terms_candidates', 0, 'record'),
        ]
        for path in paths:
            value = copy.deepcopy(self.value)
            row = value
            for key in path:
                row = row[key]
            self.assertIsNotNone(row, path)
            row['sha256'] = 'f' * 64
            with self.subTest(path=path):
                self.assertNotEqual(native.runtime_content(value), baseline)
        value = copy.deepcopy(self.value)
        value['final_link']['linker']['record']['sha256'] = 'e' * 64
        value['microsoft']['visual_studio_candidates'][0]['toolsets'][0]['linkers'][0]['record']['sha256'] = 'e' * 64
        self.assertNotEqual(native.runtime_content(value), baseline)

    def test_enumerated_binding_exclusions_do_not_conceal_content_or_status(self):
        baseline = native.runtime_content(self.value)
        value = copy.deepcopy(self.value)
        value['private_manifest']['sha256'] = 'd' * 64
        for query in value['query_bindings']:
            query['stdout']['sha256'] = 'c' * 64
            query['stderr']['sha256'] = 'b' * 64
        for key in ('trace', 'executable', 'fingerprint', 'link_output'):
            value['final_link'][key]['record']['sha256'] = 'a' * 64
        self.assertEqual(native.runtime_content(value), baseline)
        value['final_link']['fingerprint']['file_version'] = '1.2.3.4'
        self.assertNotEqual(native.runtime_content(value), baseline)
        value = copy.deepcopy(self.value)
        value['query_bindings'][0]['outcome'] = 'failed'
        value['query_bindings'][0]['exit_code'] = 1
        self.assertNotEqual(native.runtime_content(value), baseline)

    def test_unknown_schema_fields_and_positive_inferences_fail_instead_of_being_dropped(self):
        for path in ((), ('rust',), ('microsoft',), ('final_link',),
                     ('rust', 'notices', 0), ('microsoft', 'sdk_candidates', 0),
                     ('microsoft', 'visual_studio_candidates', 0, 'toolsets', 0, 'runtime_libraries', 0)):
            value = copy.deepcopy(self.value)
            row = value
            for key in path:
                row = row[key]
            row['new_obligation'] = 'must_not_be_discarded'
            with self.subTest(path=path), self.assertRaises(ValueError):
                native.runtime_content(value)
        for key in ('release_authorized', 'native_runtime_coverage_complete', 'dependency_review_approved'):
            value = copy.deepcopy(self.value)
            value[key] = True
            with self.subTest(key=key), self.assertRaises(ValueError):
                native.runtime_content(value)
        value = copy.deepcopy(self.value)
        value['microsoft']['selected_sdk'] = 'observed'
        with self.assertRaisesRegex(ValueError, 'unsupported Microsoft'):
            native.runtime_content(value)

    def test_new_linker_without_candidate_match_keeps_distinct_identity(self):
        baseline = native.runtime_content(self.value)
        value = copy.deepcopy(self.value)
        value['final_link']['installed_candidate_evidence_id'] = None
        self.assertNotEqual(native.runtime_content(value), baseline)
        self.assertIsNone(native.runtime_content(value)['runtime_final_link']['installed_candidate'])

    def test_optional_diagnostics_absence_is_not_normalized_to_null(self):
        value = copy.deepcopy(self.value)
        old = native.runtime_content(value)
        del value['final_link']['output_association']
        self.assertNotEqual(native.runtime_content(value), old)
        del value['final_link']['trace_observation']
        self.assertNotIn('trace_observation', native.runtime_content(value)['runtime_final_link'])

    def test_only_total_trace_line_volume_is_excluded_after_closed_validation(self):
        baseline = native.runtime_content(self.value)
        value = copy.deepcopy(self.value)
        value['final_link']['trace_observation']['total_lines'] += 1
        value['final_link']['trace']['record']['bytes'] += 1
        before = copy.deepcopy(value)
        self.assertEqual(native.runtime_content(value), baseline)
        self.assertEqual(value, before)
        self.assertNotIn('total_lines', baseline['runtime_final_link']['trace_observation'])
        for field in native.runtime_facts.TRACE_COUNTERS:
            if field != 'total_lines':
                self.assertEqual(baseline['runtime_final_link']['trace_observation'][field],
                                 self.value['final_link']['trace_observation'][field])
        for bad in (True, -1, native.runtime_facts.MAX_TRACE + 1):
            value = copy.deepcopy(self.value)
            value['final_link']['trace_observation']['total_lines'] = bad
            with self.subTest(value=bad), self.assertRaises(ValueError):
                native.runtime_content(value)
        value = copy.deepcopy(self.value)
        del value['final_link']['trace_observation']['total_lines']
        with self.assertRaises(ValueError):
            native.runtime_content(value)

    def test_every_other_trace_counter_is_distinct_or_rejected(self):
        baseline = native.runtime_content(self.value)
        for field in native.runtime_facts.TRACE_COUNTERS:
            if field == 'total_lines':
                continue
            value = copy.deepcopy(self.value)
            value['final_link']['trace_observation'][field] += 1
            with self.subTest(field=field):
                try:
                    actual = native.runtime_content(value)
                except ValueError:
                    continue  # An inconsistent closed-schema record fails, never compares equal.
                self.assertNotEqual(actual, baseline)

    def test_unrequested_trace_null_and_absent_remain_distinct(self):
        self.fixture.private = self.fixture.root / 'environment-only-private'
        value = self.fixture.collect()
        value['final_link'].pop('output_association', None)
        before = copy.deepcopy(value)
        projected = native.runtime_content(value)
        self.assertIsNone(projected['runtime_final_link']['trace_observation'])
        self.assertEqual(value, before)
        del value['final_link']['trace_observation']
        # Absent optional diagnostics are a distinct historical schema state.
        absent = native.runtime_content(value)
        self.assertIn('trace_observation', projected['runtime_final_link'])
        self.assertNotIn('trace_observation', absent['runtime_final_link'])
        self.assertNotEqual(absent, projected)


class BuildScriptContentTests(unittest.TestCase):
    def test_cross_package_order_only_is_ignored_without_mutation(self):
        rows = [{'package_id': 'z@1', 'linked_libs': ['static=first', 'second']},
                {'package_id': 'a@1', 'linked_libs': []}]
        before = copy.deepcopy(rows)
        self.assertEqual(native.link_requests_content(rows), native.link_requests_content(rows[::-1]))
        self.assertEqual(rows, before)
        self.assertEqual(native.CONTENT_VERSION, 2)

    def test_row_content_multiplicity_and_library_order_remain_exact(self):
        rows = [{'package_id': 'z@1', 'linked_libs': ['static=first', 'second']},
                {'package_id': 'a@1', 'linked_libs': []}]
        baseline = native.link_requests_content(rows)
        variants = [rows[:-1], rows + [copy.deepcopy(rows[0])],
                    rows + [{'package_id': 'new@1', 'linked_libs': []}]]
        reordered = copy.deepcopy(rows); reordered[0]['linked_libs'].reverse(); variants.append(reordered)
        changed = copy.deepcopy(rows); changed[0]['package_id'] = 'other@1'; variants.append(changed)
        for value in variants:
            with self.subTest(value=value):
                self.assertNotEqual(native.link_requests_content(value), baseline)
        rows[0]['unknown'] = 'not silently dropped'
        with self.assertRaisesRegex(ValueError, 'unknown build-script request field'):
            native.link_requests_content(rows)

    def test_multiple_events_for_the_same_package_keep_their_order(self):
        rows = [{'package_id': 'same@1', 'linked_libs': ['first']},
                {'package_id': 'other@1', 'linked_libs': []},
                {'package_id': 'same@1', 'linked_libs': ['second']}]
        baseline = native.link_requests_content(rows)
        self.assertEqual(native.link_requests_content([rows[1], rows[0], rows[2]]), baseline)
        self.assertNotEqual(native.link_requests_content(rows[::-1]), baseline)


if __name__ == '__main__':
    unittest.main()
