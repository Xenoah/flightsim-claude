"""Synthetic boundary/receipt tests; these do not establish a native capture."""
import copy
import importlib.util
import json
import os
from pathlib import Path, PureWindowsPath
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('runtime_facts', Path(__file__).parents[1] / 'collect-analytical-runtime-facts.py')
facts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(facts)


class RuntimeFactsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.private = self.root / 'private'
        self.sysroot = self.root / 'Users' / 'Private Person 日本語' / 'rust'
        self.program = self.root / 'Program Files (x86)'
        self.install = self.program / 'Microsoft Visual Studio/2022/Enterprise'
        self.rlib = self.write(self.sysroot / 'lib/rustlib' / facts.TARGET / 'lib/libstd-0123abcd.rlib', b'rlib identity')
        self.notice = self.write(self.sysroot / facts.NOTICE_PATHS['copyright_library'], b'<html>PRIVATE NOTICE TEXT</html>')
        self.write(self.sysroot / facts.NOTICE_PATHS['license_unicode'], b'PRIVATE UNICODE NOTICE')
        self.write(self.program / 'Microsoft Visual Studio/Installer/vswhere.exe', b'fake executable never run')
        self.linker = self.write(self.install / 'VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe', b'fake linker never run')
        self.write(self.install / 'VC/Tools/MSVC/14.44.35207/lib/x64/vcruntime.lib', b'VC library candidate')
        self.write(self.install / 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt', b'14.44.35207\n')
        self.write(self.install / 'license.txt', b'PRIVATE VS TERMS')
        sdk = self.program / 'Windows Kits/10'
        self.write(sdk / 'Lib/10.0.26100.0/ucrt/x64/ucrt.lib', b'SDK library candidate')
        self.write(sdk / 'Licenses/10.0.26100.0/license.rtf', b'PRIVATE SDK TERMS')
        self.write(sdk / 'Redist/10.0.26100.0/redist.txt', b'PRIVATE SDK REDIST')
        self.cfg = '\n'.join(['debug_assertions', 'panic="unwind"', 'target_arch="x86_64"',
                              'target_os="windows"', 'target_env="msvc"', 'target_feature="sse2"']) + '\n'
        self.outputs = {
            'rustc-version': 'rustc 1.93.0 (254b59607 2026-01-19)\nrelease: 1.93.0\ncommit-hash: ' + facts.RUST_COMMIT
                             + '\nhost: ' + facts.TARGET + '\nLLVM version: 21.1.8\n',
            'rustc-default-cfg': self.cfg,
            'rustc-recipe-cfg': self.cfg,
            'rustc-sysroot': str(self.sysroot) + '\n',
            'vswhere': json.dumps([{'productId': 'Microsoft.VisualStudio.Product.Enterprise',
                                    'installationVersion': '17.14.36510.44', 'installationPath': str(self.install),
                                    'displayName': 'PRIVATE DISPLAY NAME', 'extra': 'PRIVATE UNTRUSTED FIELD'}]),
        }

    @staticmethod
    def write(path, data):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def collect(self, recipe=True, env=None, **kwargs):
        def query(collector, identity, command):
            value = self.outputs.get(identity)
            stdout = collector.private / (identity + '.stdout')
            stderr = collector.private / (identity + '.stderr')
            stdout.write_bytes(value.encode('utf-8') if value is not None else b'')
            stderr.write_bytes(b'' if value is not None else b'PRIVATE ERROR PATH')
            collector.queries.append({'id': identity, 'command': command,
                                      'outcome': 'succeeded' if value is not None else 'failed',
                                      'exit_code': 0 if value is not None else 1,
                                      'stdout': facts.file_record(stdout), 'stderr': facts.file_record(stderr)})
            return value
        with patch.object(facts.sys, 'platform', 'win32'), patch.object(facts.Collector, 'query', query):
            return facts.collect_runtime_facts(self.private, source_sha='a' * 40,
                    env={'ProgramFiles(x86)': str(self.program)} if env is None else env,
                    recipe_cfg_args=['-D', 'warnings'] if recipe else None, **kwargs)

    def test_native_projection_bound_reproducible_and_private(self):
        value = self.collect()
        self.assertEqual(value, facts.project(self.private))
        self.assertEqual(value, facts.validate_private(self.private, value))
        public = facts.canonical(value)
        self.assertEqual(public, (self.private / facts.PROJECTION_NAME).read_bytes())
        self.assertTrue(public.isascii())
        self.assertNotIn(b'\r', public)
        for secret in (b'PRIVATE', b'Program Files', b'Users', str(self.root).encode(), b'"command":', b'"environment":'):
            self.assertNotIn(secret, public)
        self.assertTrue(value['rust']['identity']['matches_required_identity'])
        self.assertEqual(value['rust']['target_defaults']['panic_strategy'], 'unwind')
        self.assertFalse(value['rust']['target_defaults']['crt_static'])
        self.assertEqual(len(value['rust']['target_rlibs']), 1)
        self.assertEqual(value['rust']['target_rlibs'][0]['record'], facts.file_record(self.rlib))
        self.assertIsNone(value['rust']['actual_build_crt_static'])
        self.assertIsNone(value['rust']['actual_build_panic_strategy'])
        self.assertFalse(value['native_runtime_coverage_complete'])
        self.assertFalse(value['release_authorized'])
        self.assertFalse(value['dependency_review_approved'])
        microsoft = value['microsoft']
        self.assertEqual(microsoft['selected_linker'], facts.UNKNOWN)
        self.assertEqual(microsoft['selected_sdk'], facts.UNKNOWN)
        self.assertEqual(microsoft['licensed_product_entitlement'], facts.UNKNOWN)
        self.assertFalse(microsoft['installed_candidates_exhaustive'])
        self.assertEqual(microsoft['visual_studio_candidates'][0]['product'], 'Microsoft.VisualStudio.Product.Enterprise')
        self.assertEqual(microsoft['visual_studio_candidates'][0]['toolsets'][0]['linkers'][0]['record'], facts.file_record(self.linker))
        self.assertEqual(microsoft['sdk_candidates'][0]['version'], '10.0.26100.0')
        self.assertEqual({row['location'] for row in microsoft['sdk_candidates'][0]['terms_candidates']},
                         {'version_licenses', 'version_redist'})

    def test_absent_facts_stay_unknown(self):
        self.outputs = {}
        value = self.collect(recipe=False, env={})
        self.assertEqual(value['rust']['identity']['status'], facts.UNKNOWN)
        self.assertFalse(value['rust']['identity']['matches_required_identity'])
        self.assertEqual(value['rust']['sysroot_status'], facts.UNKNOWN)
        self.assertEqual(value['rust']['target_rlibs_status'], facts.UNKNOWN)
        self.assertEqual(value['rust']['target_defaults'], facts.unknown_cfg())
        self.assertEqual(value['rust']['recipe_cfg_probe'], facts.unknown_cfg())
        self.assertFalse(value['rust']['recipe_settings_supplied'])
        self.assertEqual(value['microsoft']['visual_studio_candidates'], [])
        self.assertEqual(value['microsoft']['sdk_candidates'], [])
        self.assertIsNone(value['microsoft']['vswhere'])

    def test_windows_uppercase_environment_discovers_and_replays_candidates(self):
        # CPython os.environ supplies uppercase keys on Windows, unlike the
        # mixed-case synthetic mapping used by the original discovery fixture.
        supplied = {'PROGRAMFILES(X86)': str(self.program), 'RUSTUP_AUTO_INSTALL': '1'}
        value = self.collect(env=supplied)
        self.assertEqual(value['microsoft']['vswhere']['status'], 'observed')
        self.assertEqual(len(value['microsoft']['visual_studio_candidates']), 1)
        self.assertEqual(len(value['microsoft']['sdk_candidates']), 1)
        manifest = json.loads((self.private / facts.PRIVATE_NAME).read_bytes())
        self.assertEqual(manifest['discovery_roots'], {'program_files_x86': str(self.program)})
        self.assertEqual(facts.project(self.private), value)
        self.assertEqual(facts.project(self.private, recheck_installed=False), value)
        self.assertNotIn(str(self.program), facts.canonical(value).decode())
        self.assertEqual(supplied['RUSTUP_AUTO_INSTALL'], '1')
        self.assertEqual(value['microsoft']['selected_sdk'], facts.UNKNOWN)
        self.assertEqual(value['microsoft']['static_contributions'], facts.UNKNOWN)

    def test_environment_case_aliases_are_canonical_and_ambiguity_fails_closed(self):
        for key in ('PROGRAMFILES(X86)', 'ProgramFiles(x86)', 'programfiles(x86)'):
            with self.subTest(key=key):
                original = {key: str(self.program), 'rustup_auto_install': '1'}
                collector = facts.Collector(self.root, original)
                self.assertEqual(collector.env, {'PROGRAMFILES(X86)': str(self.program),
                                                  'RUSTUP_AUTO_INSTALL': '0'})
                self.assertEqual(original, {key: str(self.program), 'rustup_auto_install': '1'})
        same = {'ProgramFiles(x86)': str(self.program), 'PROGRAMFILES(X86)': str(self.program)}
        self.assertEqual(facts.Collector(self.root, same).env['PROGRAMFILES(X86)'], str(self.program))
        for aliases in (same | {'PROGRAMFILES(X86)': str(self.root / 'foreign')},
                        {'RUSTUP_AUTO_INSTALL': '0', 'rustup_auto_install': '1'}):
            with self.subTest(aliases=aliases):
                with self.assertRaisesRegex(ValueError, 'conflicting environment key aliases'):
                    facts.Collector(self.root, aliases)

    def test_cfg_probe_separated_from_defaults_and_actual_build(self):
        self.outputs['rustc-recipe-cfg'] = self.cfg.replace('panic="unwind"', 'panic="abort"') + 'target_feature="crt-static"\n'
        value = self.collect()
        self.assertFalse(value['rust']['target_defaults']['crt_static'])
        self.assertTrue(value['rust']['recipe_cfg_probe']['crt_static'])
        self.assertEqual(value['rust']['recipe_cfg_probe']['panic_strategy'], 'abort')
        self.assertFalse(value['rust']['actual_compiler_invocation_observed'])
        self.assertIsNone(value['rust']['actual_build_panic_strategy'])

    def test_mismatched_compiler_is_not_requested_native_identity(self):
        self.outputs['rustc-version'] = self.outputs['rustc-version'].replace('release: 1.93.0', 'release: 1.94.0')
        value = self.collect()
        self.assertEqual(value['rust']['identity']['release'], '1.94.0')
        self.assertFalse(value['rust']['identity']['matches_required_identity'])

    def test_changed_installed_file_rejected_but_explicit_offline_receipts_validate(self):
        value = self.collect()
        self.rlib.write_bytes(b'mutated rlib')
        with self.assertRaisesRegex(ValueError, 'installed file changed'):
            facts.validate_private(self.private, value)
        self.assertEqual(facts.validate_private(self.private, value, recheck_installed=False), value)

    def test_changed_private_notice_rejected_even_offline(self):
        value = self.collect()
        notice = next(row for row in value['rust']['notices'] if row['id'] == 'copyright_library')
        (self.private / (notice['evidence_id'] + '.bin')).write_bytes(b'mutated private notice')
        with self.assertRaisesRegex(ValueError, 'snapshot changed'):
            facts.validate_private(self.private, value, recheck_installed=False)

    def test_changed_raw_query_rejected(self):
        value = self.collect()
        (self.private / 'rustc-version.stdout').write_bytes(b'mutated compiler output')
        with self.assertRaisesRegex(ValueError, 'query output changed'):
            facts.validate_private(self.private, value)

    def test_changed_private_manifest_rejected(self):
        value = self.collect()
        path = self.private / facts.PRIVATE_NAME
        path.write_bytes(path.read_bytes() + b' ')
        with self.assertRaisesRegex(ValueError, 'manifest changed'):
            facts.validate_private(self.private, value)

    def test_reprojection_reparses_raw_compiler_and_cfg(self):
        self.collect()
        path = self.private / facts.PRIVATE_NAME
        manifest = json.loads(path.read_bytes())
        manifest['rust']['target_defaults']['panic_strategy'] = 'abort'
        path.write_bytes(facts.canonical(manifest))
        with self.assertRaisesRegex(ValueError, 'cfg differs'):
            facts.project(self.private)

    def test_public_unknown_fields_and_privilege_inferences_rejected(self):
        value = self.collect()
        mutations = [lambda row: row.update(source_path='C:\\Users\\secret'),
                     lambda row: row.update(release_authorized=True),
                     lambda row: row.update(native_runtime_coverage_complete=True),
                     lambda row: row['rust'].update(actual_build_crt_static=True),
                     lambda row: row['microsoft'].update(selected_linker='observed'),
                     lambda row: row['microsoft'].update(licensed_product_entitlement='approved'),
                     lambda row: row['rust']['target_rlibs'][0].update(name='../libstd.rlib'),
                     lambda row: row['microsoft']['visual_studio_candidates'][0].update(product='C:\\Users\\secret')]
        for mutate in mutations:
            changed = copy.deepcopy(value); mutate(changed)
            with self.subTest(value=changed):
                with self.assertRaises(ValueError):
                    facts.validate_projection(changed)

    def test_recipe_flags_cannot_create_artifacts_or_override_paths(self):
        self.assertEqual(facts.validate_recipe_args(['-D', 'warnings']), ['-D', 'warnings'])
        for args in (['-o', 'payload'], ['--emit', 'link'], ['--sysroot', '/secret'], ['@args'],
                     ['-C', 'linker=payload'], ['-D', 'warnings', '-D', 'warnings'], ['-C', 'panic=abort\n--emit=link']):
            with self.subTest(args=args):
                with self.assertRaises(ValueError):
                    facts.validate_recipe_args(args)

    def test_hostile_rlib_basename_rejected(self):
        self.write(self.rlib.parent / 'secret user.rlib', b'hostile')
        with self.assertRaisesRegex(ValueError, 'unsafe identity'):
            self.collect()

    def test_hostile_vs_product_and_relative_root_rejected(self):
        self.outputs['vswhere'] = json.dumps([{'productId': 'C:\\Private\\secret',
            'installationVersion': '17.0.0.0', 'installationPath': str(self.install)}])
        with self.assertRaisesRegex(ValueError, 'unsafe identity'):
            self.collect()

    def test_relative_sysroot_rejected(self):
        self.outputs['rustc-sysroot'] = '../private-source\n'
        with self.assertRaisesRegex(ValueError, 'sysroot must be absolute'):
            self.collect()

    def test_notice_symlink_rejected(self):
        if os.name == 'nt':
            self.skipTest('synthetic symlink fixture does not request Windows privilege')
        self.notice.unlink()
        self.notice.symlink_to(self.rlib)
        with self.assertRaisesRegex(ValueError, 'linked or reparse'):
            self.collect()

    def test_ancestor_symlink_rejected(self):
        if os.name == 'nt':
            self.skipTest('synthetic symlink fixture does not request Windows privilege')
        linked = self.root / 'linked'
        linked.symlink_to(self.sysroot, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'linked or reparse'):
            facts.file_record(linked / 'share/doc/rust/COPYRIGHT-library.html')

    def test_unc_device_and_alternate_stream_rejected_before_stat(self):
        for path in ('\\\\server\\share\\secret', '\\\\?\\C:\\secret', '\\\\.\\pipe\\name', 'C:\\notice.txt:secret'):
            with self.subTest(path=path), patch.object(Path, 'lstat', side_effect=AssertionError('no external path access')):
                with self.assertRaises(ValueError):
                    facts.no_links(path)

    def test_bounds_for_files_directories_lists_and_aggregate(self):
        with self.assertRaisesRegex(ValueError, 'file budget'):
            facts.file_record(self.rlib, maximum=2)
        with patch.object(facts, 'MAX_ENTRIES', 0):
            with self.assertRaisesRegex(ValueError, 'directory entry budget'):
                facts.children(self.rlib.parent)
        with patch.object(facts, 'MAX_TOTAL', 2):
            with self.assertRaisesRegex(ValueError, 'aggregate file budget'):
                self.collect()

    def test_missing_target_library_directory_recorded(self):
        self.rlib.unlink(); self.rlib.parent.rmdir()
        value = self.collect()
        self.assertEqual(value['rust']['target_rlibs_status'], 'missing')
        self.assertEqual(value['rust']['target_rlibs'], [])

    def test_unreadable_file_recorded_without_path_export(self):
        collector = facts.Collector(self.root, {})
        with patch.object(facts, 'file_record', side_effect=PermissionError('PRIVATE PATH')):
            observed = collector.observe(self.rlib)
        self.assertEqual(observed['status'], 'unreadable')
        self.assertIsNone(observed['record'])
        self.assertEqual(collector.files, [])

    def test_query_never_implicitly_installs_toolchains(self):
        collector = facts.Collector(self.root, {'RUSTUP_AUTO_INSTALL': '1'})
        self.assertEqual(collector.env['RUSTUP_AUTO_INSTALL'], '0')
        # A tiny real subprocess tests byte journaling; no native facts claimed.
        result = collector.query('rustc-version', [sys.executable, '-c', 'print("fixture")'])
        self.assertEqual(result.strip(), 'fixture')
        self.assertEqual(collector.queries[0]['outcome'], 'succeeded')
        self.assertEqual(collector.queries[0]['stdout'], facts.file_record(self.root / 'rustc-version.stdout'))

    def test_collection_requires_native_windows(self):
        with patch.object(facts.sys, 'platform', 'linux'):
            with self.assertRaisesRegex(ValueError, 'native Windows'):
                facts.collect_runtime_facts(self.private, source_sha='a' * 40)
        self.assertFalse(self.private.exists())

    def test_resealed_origin_relabels_and_source_substitution_rejected(self):
        self.collect()
        path = self.private / facts.PRIVATE_NAME
        original = json.loads(path.read_bytes())
        mutations = [lambda raw: raw['rust']['target_rlibs'][0].update(name='libstd-deadbeef.rlib'),
                     lambda raw: raw['microsoft']['sdk_candidates'][0].update(version='99.0.0'),
                     lambda raw: raw['microsoft']['visual_studio_candidates'][0]['toolsets'][0].update(version='99.0.0'),
                     lambda raw: raw['rust'].update(target_rlibs=[]),
                     lambda raw: raw['files'][0].update(source=str(self.root / 'foreign' / self.notice.name))]
        self.write(self.root / 'foreign' / self.notice.name, self.notice.read_bytes())
        for mutate in mutations:
            raw = copy.deepcopy(original); mutate(raw); path.write_bytes(facts.canonical(raw))
            with self.subTest(mutation=mutate):
                with self.assertRaises(ValueError):
                    facts.project(self.private)
                with self.assertRaises(ValueError):
                    facts.project(self.private, recheck_installed=False)

    def test_resealed_version_resource_rejected_by_native_recheck(self):
        self.collect()
        path = self.private / facts.PRIVATE_NAME
        raw = json.loads(path.read_bytes())
        observed = raw['microsoft']['visual_studio_candidates'][0]['toolsets'][0]['linkers'][0]
        observed['product_version'] = '99.0.0.0'
        row = next(row for row in raw['files'] if row['id'] == observed['evidence_id'])
        row['product_version'] = '99.0.0.0'
        path.write_bytes(facts.canonical(raw))
        with self.assertRaisesRegex(ValueError, 'source path or version changed'):
            facts.project(self.private)

    def test_revalidation_does_not_write_snapshots(self):
        value = self.collect()
        before = {path.name: (path.read_bytes(), path.stat().st_mtime_ns) for path in self.private.iterdir()}
        with patch.object(Path, 'write_bytes', side_effect=AssertionError('validation cannot write')):
            facts.validate_private(self.private, value)
        after = {path.name: (path.read_bytes(), path.stat().st_mtime_ns) for path in self.private.iterdir()}
        self.assertEqual(before, after)

    def test_offline_windows_origin_semantics_on_other_host(self):
        self.collect()
        raw = json.loads((self.private / facts.PRIVATE_NAME).read_bytes())
        def windows(value):
            return value.replace(str(self.root), 'C:\\Users\\Private Person')
        for row in raw['files']:
            row['source'] = windows(row['source'])
        raw['discovery_roots']['program_files_x86'] = windows(raw['discovery_roots']['program_files_x86'])
        outputs = dict(self.outputs)
        outputs['rustc-sysroot'] = windows(outputs['rustc-sysroot'])
        installs = json.loads(outputs['vswhere'])
        installs[0]['installationPath'] = windows(installs[0]['installationPath'])
        outputs['vswhere'] = json.dumps(installs)
        records = {row['id']: row for row in raw['files']}
        facts.validate_origins(raw, records, outputs.get)
        raw['rust']['target_rlibs'][0]['name'] = 'libstd-deadbeef.rlib'
        with self.assertRaisesRegex(ValueError, 'source path'):
            facts.validate_origins(raw, records, outputs.get)

    def link_fixture(self, *, output='deps', duplicates=False, mismatch=False, linker=None):
        executable = self.write(self.root / 'target-analytic' / facts.TARGET / 'release/flightsim-app.exe', b'audited app fixture')
        suffix = '0123456789abcdef'
        self.write(executable.parent / '.fingerprint' / ('flightsim-app-' + suffix) / 'bin-flightsim-app.json', b'{"fixture":true}')
        deps = self.write(executable.parent / 'deps' / ('flightsim_app-' + suffix + '.exe'),
                          b'different binary' if mismatch else executable.read_bytes())
        path = deps if output == 'deps' else executable
        args = [str(linker or self.linker), '/NOLOGO', '/OUT:' + str(path), 'kernel32.lib', '/DEFAULTLIB:msvcrt',
                '/LIBPATH:' + str(executable.parent / 'deps'), '/LIBPATH:' + str(self.rlib.parent), str(self.rlib)]
        line = ' INFO rustc_codegen_ssa::back::link ' + ' '.join(json.dumps(item, ensure_ascii=False) for item in args) + '\n'
        trace = self.write(self.root / 'build.stderr', (line * (2 if duplicates else 1)).encode('utf-8'))
        return trace, executable

    def test_unique_final_deps_link_matches_hashes_without_sdk_or_execution_claim(self):
        trace, executable = self.link_fixture()
        value = self.collect(linker_trace=trace, audited_executable=executable)
        link = value['final_link']
        self.assertEqual(link['status'], 'observed')
        self.assertEqual(link['executable']['record'], link['link_output']['record'])
        self.assertEqual(link['linker']['record'], facts.file_record(self.linker))
        candidate = value['microsoft']['visual_studio_candidates'][0]['toolsets'][0]['linkers'][0]
        self.assertEqual(link['installed_candidate_evidence_id'], candidate['evidence_id'])
        self.assertEqual(link['requested_libraries'], ['kernel32.lib', 'msvcrt.lib'])
        self.assertEqual(link['rlib_input_names'], [self.rlib.name])
        self.assertEqual({row['origin'] for row in link['explicit_library_paths']}, {'cargo_release_deps', 'rust_target_libraries'})
        self.assertFalse(link['linker_process_execution_observed'])
        self.assertEqual(link['implicit_sdk_selection'], facts.UNKNOWN)
        self.assertEqual(link['actual_static_membership'], facts.UNKNOWN)
        self.assertNotIn(str(self.root), facts.canonical(value).decode())
        self.assertEqual(link['trace_observation'], dict.fromkeys(facts.TRACE_COUNTERS, 1)
                         | {'rejected_command_lines': 0})
        self.assertEqual(facts.project(self.private), value)

    def test_duplicate_final_link_is_unknown(self):
        trace, executable = self.link_fixture(duplicates=True)
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['reason'], 'ambiguous_command')
        self.assertFalse(value['final_link']['constructed_final_command_observed'])

    def test_unbound_deps_binary_is_unknown(self):
        trace, executable = self.link_fixture(mismatch=True)
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['reason'], 'output_hash_mismatch')
        self.assertIsNone(value['final_link']['linker'])

    def test_relative_linker_is_unknown(self):
        trace, executable = self.link_fixture(linker='link.exe')
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['reason'], 'unresolved_linker')

    def test_same_bytes_foreign_linker_is_not_installed_candidate(self):
        other = self.write(self.root / 'foreign/link.exe', self.linker.read_bytes())
        trace, executable = self.link_fixture(linker=other)
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['status'], 'observed')
        self.assertIsNone(value['final_link']['installed_candidate_evidence_id'])

    def test_foreign_output_and_unsupported_debug_format_do_not_match(self):
        trace, executable = self.link_fixture()
        trace.write_text(' INFO rustc_codegen_ssa::back::link "link.exe" "/OUT:C:\\\\foreign\\\\flightsim-app.exe"\n', encoding='utf-8')
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['reason'], 'no_matching_command')

    def test_trace_counts_expose_format_boundaries_without_private_text(self):
        trace, executable = self.link_fixture()
        prefix = ' INFO rustc_codegen_ssa::back::link'
        trace.write_text('\n'.join([
            'PRIVATE ERROR PATH',
            '2026-10-08T00:00:00Z' + prefix + ' "PRIVATE unsupported prefix"',
            prefix + ': preparing linker PRIVATE TEXT',
            prefix + ' ENV="PRIVATE" "link.exe"',
            prefix + ' "C:\\\\Private\\\\link.exe" "kernel32.lib"',
            prefix + ' "C:\\\\Private\\\\link.exe" "/OUT:C:\\\\foreign\\\\flightsim-app.exe"',
        ]) + '\n', encoding='utf-8')
        value = self.collect(linker_trace=trace, audited_executable=executable)
        link = value['final_link']
        self.assertEqual(link['reason'], 'no_matching_command')
        self.assertEqual(link['trace_observation'], {
            'total_lines': 6, 'link_module_lines': 5, 'recognized_info_lines': 4,
            'quoted_command_lines': 2, 'parsed_command_lines': 2, 'rejected_command_lines': 0,
            'output_switch_commands': 1, 'matching_output_commands': 0})
        self.assertFalse(link['constructed_final_command_observed'])
        for private in ('PRIVATE', 'Private', 'foreign', str(self.root)):
            self.assertNotIn(private, facts.canonical(value).decode())
        self.assertEqual(facts.project(self.private), value)

    def test_unparseable_debug_command_remains_unknown_with_bounded_counts(self):
        trace, executable = self.link_fixture()
        trace.write_text(' INFO rustc_codegen_ssa::back::link "link.exe" "\\q"\n', encoding='utf-8')
        value = self.collect(linker_trace=trace, audited_executable=executable)
        link = value['final_link']
        self.assertEqual(link['reason'], 'unparseable_command')
        self.assertEqual(link['trace_observation'], dict.fromkeys(facts.TRACE_COUNTERS, 0) | {
            'total_lines': 1, 'link_module_lines': 1, 'recognized_info_lines': 1,
            'quoted_command_lines': 1, 'rejected_command_lines': 1})
        self.assertIsNone(link['linker'])

    def test_no_trace_scan_does_not_project_zero_counts(self):
        value = self.collect()
        self.assertIsNone(value['final_link']['trace_observation'])

    def test_trace_diagnostic_schema_is_closed_and_replay_bound(self):
        trace, executable = self.link_fixture()
        value = self.collect(linker_trace=trace, audited_executable=executable)
        mutations = [
            lambda row: row.update(raw_command='private'),
            lambda row: row.update(total_lines=True),
            lambda row: row.update(total_lines=-1),
            lambda row: row.update(total_lines=facts.MAX_TRACE + 1),
            lambda row: row.update(parsed_command_lines=2),
            lambda row: row.update(matching_output_commands=0),
        ]
        for mutate in mutations:
            changed = copy.deepcopy(value)
            mutate(changed['final_link']['trace_observation'])
            with self.subTest(mutation=mutate), self.assertRaises(ValueError):
                facts.validate_projection(changed)
        path = self.private / facts.PRIVATE_NAME
        manifest = json.loads(path.read_bytes())
        # A plausible count increase passes the scalar schema, but cannot be
        # resealed into a different native observation of the same trace.
        manifest['final_link']['trace_observation']['total_lines'] += 1
        path.write_bytes(facts.canonical(manifest))
        with self.assertRaisesRegex(ValueError, 'final link observations changed'):
            facts.project(self.private)

    def test_legacy_capture_without_trace_counters_still_replays(self):
        trace, executable = self.link_fixture()
        self.collect(linker_trace=trace, audited_executable=executable)
        path = self.private / facts.PRIVATE_NAME
        manifest = json.loads(path.read_bytes())
        del manifest['final_link']['trace_observation']
        del manifest['final_link']['output_association']
        path.write_bytes(facts.canonical(manifest))
        value = facts.project(self.private)
        self.assertNotIn('trace_observation', value['final_link'])
        self.assertEqual(value['final_link']['status'], 'observed')
        self.assertEqual(facts.project(self.private, recheck_installed=False), value)

    def test_only_output_arguments_in_admitted_roots_are_candidates(self):
        trace, executable = self.link_fixture()
        foreign = executable.parent / 'foreign/flightsim-app.exe'
        wrong_deps = executable.parent / 'deps/flightsim_app-ffffffffffffffff.exe'
        alternatives = [
            ['/OUT:' + str(foreign)],
            ['/OUT:' + str(wrong_deps)],
            ['/OUT:flightsim-app.exe'],
            ['/COMMENT:/OUT:' + str(executable)],
            ['/LIBPATH:' + str(executable)],
            ['literal "/OUT:' + str(executable) + '"'],
        ]
        collector = facts.Collector(self.root, {})
        for args in alternatives:
            trace.write_text(' INFO rustc_codegen_ssa::back::link ' +
                             ' '.join(json.dumps(item) for item in [str(self.linker), *args]) + '\n',
                             encoding='utf-8')
            with self.subTest(args=args):
                link = facts.final_link_facts(collector, trace, executable)
                self.assertEqual(link['reason'], 'output_identity_unavailable' if args == ['/OUT:' + str(wrong_deps)]
                                 else 'no_matching_command')
                self.assertEqual(link['trace_observation']['matching_output_commands'], 0)
                self.assertIsNone(link['linker'])

    def test_multiple_output_arguments_cannot_select_first_matching_path(self):
        trace, executable = self.link_fixture()
        collector = facts.Collector(self.root, {})
        for extra in (str(executable), str(executable.parent / 'foreign/flightsim-app.exe')):
            args = [str(self.linker), '/OUT:' + str(executable), '/OUT:' + extra]
            trace.write_text(' INFO rustc_codegen_ssa::back::link ' +
                             ' '.join(json.dumps(item) for item in args) + '\n', encoding='utf-8')
            with self.subTest(extra=extra):
                link = facts.final_link_facts(collector, trace, executable)
                self.assertEqual(link['reason'], 'unparseable_command')
                self.assertEqual(link['trace_observation']['matching_output_commands'], 0)
                self.assertIsNone(link['linker'])

    def replace_link_output(self, trace, old, new):
        text = trace.read_text(encoding='utf-8')
        before, after = json.dumps('/OUT:' + str(old)), json.dumps('/OUT:' + str(new))
        self.assertIn(before, text)
        trace.write_text(text.replace(before, after), encoding='utf-8')

    def test_unexpected_same_build_output_name_requires_exact_content(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/observed-output.exe', executable.read_bytes())
        self.replace_link_output(trace, executable, other)
        value = self.collect(linker_trace=trace, audited_executable=executable)
        link = value['final_link']
        self.assertEqual(link['status'], 'observed')
        self.assertEqual(link['output_association'], {'method': facts.OUTPUT_ASSOCIATION,
                         'scan': {'candidate_commands': 1, 'missing_outputs': 0, 'mismatched_outputs': 0}})
        self.assertEqual(link['link_output']['record'], facts.file_record(executable))
        self.assertEqual(facts.project(self.private), value)
        with self.assertRaisesRegex(ValueError, 'requires original build files'):
            facts.project(self.private, recheck_installed=False)
        self.assertFalse(link['linker_process_execution_observed'])
        self.assertEqual(link['actual_static_membership'], facts.UNKNOWN)
        self.assertEqual(link['implicit_sdk_selection'], facts.UNKNOWN)
        self.assertFalse(value['release_authorized'])

    def test_safe_name_with_same_size_different_content_stays_unknown(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/observed-output.exe', b'x' * executable.stat().st_size)
        self.replace_link_output(trace, executable, other)
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['reason'], 'output_hash_mismatch')
        self.assertEqual(value['final_link']['output_association']['scan']['mismatched_outputs'], 1)
        self.assertFalse(value['final_link']['constructed_final_command_observed'])

    def test_matching_bytes_outside_roots_or_through_parent_alias_are_not_read(self):
        trace, executable = self.link_fixture(output='root')
        foreign = self.write(self.root / 'other-build/release/flightsim-app.exe', executable.read_bytes())
        same_tree_nested = self.write(executable.parent / 'nested/flightsim-app.exe', executable.read_bytes())
        alias = str(executable.parent / 'nested') + '/../flightsim-app.exe'
        for candidate in (foreign, same_tree_nested, alias):
            args = [str(self.linker), '/OUT:' + str(candidate)]
            trace.write_text(' INFO rustc_codegen_ssa::back::link ' + ' '.join(map(json.dumps, args)) + '\n')
            collector = facts.Collector(self.root, {})
            original = facts.file_record
            def record(path, *args):
                self.assertNotEqual(str(path), str(candidate))
                return original(path, *args)
            with self.subTest(candidate=candidate), patch.object(facts, 'file_record', side_effect=record):
                link = facts.final_link_facts(collector, trace, executable)
                self.assertEqual(link['reason'], 'no_matching_command')
                self.assertEqual(link['output_association']['scan']['candidate_commands'], 0)

    def test_two_different_names_with_identical_bytes_remain_ambiguous(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/observed-output.exe', executable.read_bytes())
        original = trace.read_text()
        trace.write_text(original + original.replace(json.dumps('/OUT:' + str(executable)), json.dumps('/OUT:' + str(other))))
        value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertEqual(value['final_link']['reason'], 'ambiguous_command')
        self.assertEqual(value['final_link']['trace_observation']['matching_output_commands'], 2)
        self.assertFalse(value['final_link']['constructed_final_command_observed'])

    def test_other_nonmatching_outputs_do_not_hide_unique_equal_output(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/different.exe', b'different')
        original = trace.read_text()
        trace.write_text(original + original.replace(json.dumps('/OUT:' + str(executable)), json.dumps('/OUT:' + str(other))))
        value = self.collect(linker_trace=trace, audited_executable=executable)
        link = value['final_link']
        self.assertEqual(link['status'], 'observed')
        self.assertEqual(link['output_association']['scan'],
                         {'candidate_commands': 2, 'missing_outputs': 0, 'mismatched_outputs': 1})
        # Replacing the other output with copied matching bytes changes the
        # association to ambiguous and must fail native fresh replay.
        other.write_bytes(executable.read_bytes())
        with self.assertRaisesRegex(ValueError, 'final link observations changed'):
            facts.project(self.private)

    def test_link_output_change_and_removal_fail_fresh_replay(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/observed-output.exe', executable.read_bytes())
        self.replace_link_output(trace, executable, other)
        self.collect(linker_trace=trace, audited_executable=executable)
        other.write_bytes(b'changed')
        with self.assertRaises(ValueError): facts.project(self.private)
        other.unlink()
        with self.assertRaises((ValueError, OSError)): facts.project(self.private)

    def test_candidate_path_link_rejected_before_read(self):
        trace, executable = self.link_fixture(output='root')
        alias = executable.parent / 'deps/linked-output.exe'
        try:
            alias.symlink_to(executable)
        except OSError as error:
            if os.name == 'nt' and getattr(error, 'winerror', None) == 1314:
                self.skipTest('Windows fixture cannot create a symlink without privilege')
            raise
        self.replace_link_output(trace, executable, alias)
        with self.assertRaisesRegex(ValueError, 'linked or reparse'):
            self.collect(linker_trace=trace, audited_executable=executable)

    def test_candidate_count_and_hash_budgets_are_bounded(self):
        trace, executable = self.link_fixture(output='root')
        with patch.object(facts, 'MAX_OUTPUT_CANDIDATES', 0), self.assertRaisesRegex(ValueError, 'candidate output budget'):
            facts.final_link_facts(facts.Collector(self.root, {}), trace, executable)
        with patch.object(facts, 'MAX_OUTPUT_HASH_BYTES', executable.stat().st_size - 1), self.assertRaisesRegex(ValueError, 'candidate hash budget'):
            facts.final_link_facts(facts.Collector(self.root, {}), trace, executable)

    def test_output_association_schema_is_closed(self):
        trace, executable = self.link_fixture()
        value = self.collect(linker_trace=trace, audited_executable=executable)
        for change in ('method', 'raw', 'count', 'bool', 'missing'):
            modified = copy.deepcopy(value)
            association = modified['final_link']['output_association']
            if change == 'method': association['method'] = 'basename'
            if change == 'raw': association['raw_path'] = 'private'
            if change == 'count': association['scan']['candidate_commands'] += 1
            if change == 'bool': association['scan']['candidate_commands'] = True
            if change == 'missing': association['scan'] = None
            with self.subTest(change=change), self.assertRaises(ValueError):
                facts.validate_projection(modified)

    def test_legacy_unknown_result_is_not_reclassified_by_new_association(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/observed-output.exe', executable.read_bytes())
        self.replace_link_output(trace, executable, other)
        original = facts.final_link_facts
        def legacy(*args, **kwargs):
            return original(*args, **(kwargs | {'legacy_output_names': True}))
        with patch.object(facts, 'final_link_facts', side_effect=legacy):
            value = self.collect(linker_trace=trace, audited_executable=executable)
        self.assertNotIn('output_association', value['final_link'])
        self.assertEqual(value['final_link']['reason'], 'no_matching_command')
        self.assertEqual(facts.project(self.private), value)

    def test_new_association_cannot_be_resealed_as_legacy_observation(self):
        trace, executable = self.link_fixture(output='root')
        other = self.write(executable.parent / 'deps/observed-output.exe', executable.read_bytes())
        self.replace_link_output(trace, executable, other)
        self.collect(linker_trace=trace, audited_executable=executable)
        path = self.private / facts.PRIVATE_NAME
        manifest = json.loads(path.read_bytes())
        del manifest['final_link']['output_association']
        path.write_bytes(facts.canonical(manifest))
        with self.assertRaisesRegex(ValueError, 'foreign final linker output'):
            facts.project(self.private, recheck_installed=False)

    def test_new_association_retains_unique_fingerprint_requirement(self):
        trace, executable = self.link_fixture(output='root')
        directory = executable.parent / '.fingerprint'
        fingerprint = next(directory.glob('*/bin-flightsim-app.json'))
        original = fingerprint.read_bytes()
        fingerprint.unlink()
        link = facts.final_link_facts(facts.Collector(self.root, {}), trace, executable)
        self.assertEqual(link['reason'], 'missing_fingerprint')
        self.assertIsNone(link['output_association']['scan'])
        fingerprint.write_bytes(original)
        self.write(directory / 'flightsim-app-ffffffffffffffff/bin-flightsim-app.json', original)
        link = facts.final_link_facts(facts.Collector(self.root, {}), trace, executable)
        self.assertEqual(link['reason'], 'ambiguous_fingerprint')
        self.assertFalse(link['constructed_final_command_observed'])

    def test_windows_output_root_semantics_are_case_insensitive_and_bounded(self):
        executable = PureWindowsPath(r'C:\build\target\x86_64-pc-windows-msvc\release\flightsim-app.exe')
        self.assertTrue(facts.admitted_link_output(
            r'c:\BUILD\target\x86_64-pc-windows-msvc\RELEASE\deps\unexpected.EXE', executable))
        for path in (r'C:\other\release\flightsim-app.exe',
                     r'C:\build\target\x86_64-pc-windows-msvc\release\deps\..\flightsim-app.exe',
                     r'C:\build\target\x86_64-pc-windows-msvc\release\deps\unexpected.exe:stream',
                     r'\\?\C:\build\target\x86_64-pc-windows-msvc\release\flightsim-app.exe'):
            with self.subTest(path=path):
                self.assertFalse(facts.admitted_link_output(path, executable))

    def test_windows_command_debug_drive_paths_and_regular_argument_quoting(self):
        # Source-shaped regular OsString Debug formatting, not Windows shell
        # quoting. Rust 1.93.0 windows.rs Command::fmt and linker.rs /OUT:
        # https://github.com/rust-lang/rust/blob/254b59607d4417e9dffbc307138ae5c86280fe4c/library/std/src/sys/process/windows.rs#L436-L447
        # https://github.com/rust-lang/rust/blob/254b59607d4417e9dffbc307138ae5c86280fe4c/compiler/rustc_codegen_ssa/src/back/linker.rs#L1048-L1052
        command = (r'"C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Tools\\MSVC\\14.44.35207\\bin\\Hostx64\\x64\\link.exe" '
                   r'"/NOLOGO" "/OUT:D:\\work\\Private Project 日本語\\target\\x86_64-pc-windows-msvc\\release\\deps\\flightsim_app-0123456789abcdef.exe" '
                   r'"/LIBPATH:C:\\Program Files (x86)\\Windows Kits\\10\\Lib\\10.0.26100.0\\um\\x64" '
                   r'"C:\\Users\\Private Person\\.rustup\\toolchains\\1.93.0-x86_64-pc-windows-msvc\\lib\\rustlib\\x86_64-pc-windows-msvc\\lib\\libstd-0123abcd.rlib" '
                   r'"/COMMENT:embedded \"/OUT:D:\\foreign.exe\"" "kernel32.lib" ""')
        self.assertEqual(facts.debug_command_tokens(command), [
            r'C:\Program Files (x86)\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe',
            '/NOLOGO',
            r'/OUT:D:\work\Private Project 日本語\target\x86_64-pc-windows-msvc\release\deps\flightsim_app-0123456789abcdef.exe',
            r'/LIBPATH:C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64',
            r'C:\Users\Private Person\.rustup\toolchains\1.93.0-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\lib\libstd-0123abcd.rlib',
            r'/COMMENT:embedded "/OUT:D:\foreign.exe"', 'kernel32.lib', ''])

    def test_debug_parser_is_not_a_shell_and_rejects_hostile_inputs(self):
        self.assertEqual(facts.debug_command_tokens('"C:\\\\Program Files\\\\link.exe" "/OUT:C:\\\\x.exe"'),
                         ['C:\\Program Files\\link.exe', '/OUT:C:\\x.exe'])
        self.assertEqual(facts.debug_command_tokens('"\\u{65e5}\\u{672c}"'), ['日本'])
        for text in ('"link.exe"; evil', 'ENV="private" "link.exe"', '"link.exe" "\\q"', '"link.exe" "\\n"',
                     '"\\u{d800}"', '"unterminated', '"x"\t"y"'):
            with self.subTest(text=text):
                with self.assertRaises(ValueError):
                    facts.debug_command_tokens(text)


if __name__ == '__main__':
    unittest.main()
