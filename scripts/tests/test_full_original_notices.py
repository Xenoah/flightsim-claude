"""Original-byte publication boundaries; native builds are deliberately mocked."""
import base64
import copy
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('full_original_notice_tests', ROOT / 'scripts/export-full-original-notices.py')
api = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(api)


class ContentPolicyTests(unittest.TestCase):
    # Public source paths observed in the genuine 550 Windows capture. These
    # are ordering fixtures only, not original inventory/native evidence.
    WINDOWS_NOTICE_ORDER = {
        'cosmic-text@0.16.0': (
            'fonts/FiraMono-LICENSE', 'fonts/Inter-LICENSE', 'fonts/NotoSans-LICENSE',
            'LICENSE-APACHE', 'LICENSE-MIT'),
        'zune-jpeg@0.5.15': (
            'FLIGHTSIM-MODIFICATION-NOTICE.txt', 'LICENSE-APACHE', 'LICENSE-FLIGHTSIM-APACHE',
            'LICENSE-FLIGHTSIM-MIT', 'LICENSE-MIT', 'LICENSE-ZLIB',
            'third-party-notices/libjpeg-turbo-acknowledgement-NOTICE.txt',
            'third-party-notices/libjpeg-turbo-adaptation-NOTICE.txt',
            'third-party-notices/libjpeg-turbo-jdhuff-copyright-header.txt',
            'third-party-notices/libjpeg-turbo-LICENSE.ijg',
            'third-party-notices/libultrahdr-Apache-2.0-LICENSE.txt',
            'third-party-notices/libultrahdr-copyright-headers.txt',
            'third-party-notices/libultrahdr-upstream-Adobe-NOTICE.txt',
            'third-party-notices/STANFORD-NOTICE.txt',
            'third-party-notices/stb-MIT-NOTICE.txt',
            'third-party-notices/stb-upstream-dual-license.txt'),
    }

    def ordering_fixture(self):
        policy = api.content_policy(ROOT)
        projection = {'packages': [], 'embedded_assets': []}
        for item in policy['packages']:
            row = item['inventory_fields']
            projection['packages'].append({
                'id': row['id'], 'name': row['name'], 'version': row['version'],
                'declared_license': row['license_expression'],
                'conservative_features': ['default'] if row['name'] == 'flightsim-app' else [],
                'notices': [{k: n[k] for k in ('path', 'upstream_path', 'sha256', 'bytes')}
                            for n in row['notices']]})
        projection['embedded_assets'] = [{k: a[k] for k in
            ('id', 'package', 'version', 'feature', 'source_path', 'sha256', 'notices')}
            for a in policy['embedded_assets']]
        build = {'bindings': {'lock': policy['source_files']['Cargo.lock']},
                 'builds': {'ordinary': {'metadata': api.bytes_record(b'synthetic metadata, not a capture')}}}
        return policy, projection, build

    def test_pinned_windows_notice_order_matches_pathlib_target_semantics(self):
        policy, projection, build = self.ordering_fixture()
        by_id = {r['inventory_fields']['id']: r['inventory_fields'] for r in policy['packages']}
        for identity, expected in self.WINDOWS_NOTICE_ORDER.items():
            actual = tuple(n['upstream_path'] for n in by_id[identity]['notices'])
            self.assertEqual(actual, expected)
            self.assertEqual(actual, tuple(p.as_posix() for p in sorted(map(PureWindowsPath, actual))))
            self.assertNotEqual(actual, tuple(p.as_posix() for p in sorted(map(PurePosixPath, actual))))
        self.assertEqual(api.expected_inventory(policy, projection, build)['review_status'], 'not_reviewed')

    def test_historical_posix_policy_reproduces_the_real_order_rejection(self):
        policy, projection, build = self.ordering_fixture()
        for row in policy['packages']:
            if row['inventory_fields']['id'] in self.WINDOWS_NOTICE_ORDER:
                row['inventory_fields']['notices'].sort(key=lambda n: PurePosixPath(n['upstream_path']))
        with self.assertRaisesRegex(ValueError, 'native notice set differs from pinned original content'):
            api.expected_inventory(policy, projection, build)

    def test_windows_order_fix_does_not_accept_other_notice_permutations_or_bytes(self):
        policy, projection, build = self.ordering_fixture()
        rows = {r['id']: r for r in projection['packages']}
        for identity in self.WINDOWS_NOTICE_ORDER:
            original = copy.deepcopy(rows[identity]['notices'])
            mutations = [lambda n: n.reverse(), lambda n: n.append(copy.deepcopy(n[0])),
                         lambda n: n.pop(), lambda n: n[0].update(path='licenses/injected/LICENSE'),
                         lambda n: n[0].update(upstream_path='unreviewed/LICENSE'),
                         lambda n: n[0].update(sha256='0' * 64), lambda n: n[0].update(bytes=n[0]['bytes'] + 1),
                         lambda n: n[0].update(extra='unreviewed field')]
            for mutate in mutations:
                changed = copy.deepcopy(original); mutate(changed); rows[identity]['notices'] = changed
                with self.subTest(identity=identity, mutate=mutate), self.assertRaisesRegex(ValueError, 'pinned original content'):
                    api.expected_inventory(policy, projection, build)
            rows[identity]['notices'] = original

    def test_pinned_public_corpus_and_source_are_closed_and_current(self):
        policy = api.content_policy(ROOT)
        self.assertEqual(len(policy['packages']), 359)
        self.assertEqual(len(policy['content_sources']), 735)
        self.assertEqual(policy['source_sha'], '55094fa928fbb8f907a2103740ee6824171c26e7')
        self.assertEqual(policy['source_tree'], 'fdd9fc4eeb8eacc8aae9e4a5c6e18da6bdd00cf5')
        self.assertFalse(policy['release_authorized'])
        self.assertFalse(policy['dependency_review_approved'])
        self.assertFalse(policy['whole_target_review_complete'])
        self.assertEqual({a['id'] for a in policy['embedded_assets']}, api.native.EMBEDDED_IDS)
        references = {n['path']: n for item in policy['packages'] for n in item['inventory_fields']['notices']}
        references.update({n['path']: n for a in policy['embedded_assets'] for n in a['notices']})
        self.assertEqual(set(references), set(policy['content_sources']))
        for name, origin in policy['content_sources'].items():
            api.relative(name)
            self.assertIn(set(origin), ({'package', 'registry_checksum', 'crate_member'},
                                        {'package', 'source', 'source_revision', 'upstream_path'}, {'source_file'}))
            if 'source_file' in origin:
                raw = (ROOT / origin['source_file']).read_bytes()
                self.assertEqual(api.bytes_record(raw), {k: references[name][k] for k in ('sha256', 'bytes')})
                api.text_bytes(raw)
            else:
                packages = {row['inventory_fields']['id']: row['inventory_fields'] for row in policy['packages']}
                package = packages[origin['package']]
                if 'registry_checksum' in origin:
                    self.assertEqual(origin['registry_checksum'], package['source_checksum'])
                    self.assertEqual(origin['crate_member'], references[name]['upstream_path'])
                else:
                    self.assertEqual({key: origin[key] for key in ('source', 'source_revision', 'upstream_path')},
                                     {key: references[name][key] for key in ('source', 'source_revision', 'upstream_path')})

    def test_policy_unknown_bytes_cannot_reseal_the_pinned_hash(self):
        raw = (ROOT / 'scripts' / api.POLICY_NAME).read_bytes()
        policy = json.loads(raw); policy['extra'] = 'private string'
        with patch.object(api, 'read_bytes', return_value=api.canonical(policy)):
            with self.assertRaisesRegex(ValueError, 'content policy changed'):
                api.content_policy(ROOT)


class OriginalNoticeTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.repo = self.root / 'repo'; self.repo.mkdir()
        self.private = self.root / 'build-private'; self.private.mkdir()
        self.build_text = self.root / 'build-evidence'; self.build_text.mkdir()
        self.bundle = self.root / 'bundle'; self.bundle.mkdir()
        self.native_path = self.root / 'native-review.json'
        self.output = self.root / api.EXPORT_NAME
        self.notice_root = self.private / 'capture/ordinary/notices'; self.notice_root.mkdir(parents=True)
        self.policy = api.content_policy(ROOT)
        # Small fixture retains real pinned Apache/MIT grants and current source
        # notices. Source/build/native audits alone are mocked, never text gates.
        self.policy['packages'] = [row for row in self.policy['packages'] if
                                   row['inventory_fields']['name'] in ('flightsim-app', 'bevy_core_pipeline')]
        wanted = {n['path'] for row in self.policy['packages'] for n in row['inventory_fields']['notices']}
        wanted |= {n['path'] for asset in self.policy['embedded_assets'] for n in asset['notices']}
        self.policy['content_sources'] = {name: value for name, value in self.policy['content_sources'].items() if name in wanted}
        for name, origin in self.policy['content_sources'].items():
            path = self.notice_root / name; path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((ROOT / origin['source_file']).read_bytes())
        self.sha = 'a' * 40
        self.source = {'source_sha': self.sha, 'source_tree': self.policy['source_tree']}
        source_record = api.bytes_record(api.canonical(self.source))
        self.build = {'status': api.capture.PASS, 'source_recipe': api.native.SOURCE_RECIPE,
                      'bindings': {'source': source_record, 'lock': self.policy['source_files']['Cargo.lock']},
                      'builds': {'ordinary': {'metadata': api.bytes_record(b'private metadata'),
                                              'inventory': {}, 'lut_payloads_found': 2}}}
        self.projection = {'source_sha': self.sha, 'source_tree': self.source['source_tree'],
                           'bindings': {}, 'packages': [], 'embedded_assets': []}
        for item in self.policy['packages']:
            row = item['inventory_fields']
            self.projection['packages'].append({
                'id': row['id'], 'name': row['name'], 'version': row['version'],
                'declared_license': row['license_expression'],
                'conservative_features': ['default'] if row['name'] == 'flightsim-app' else ['tonemapping_luts'],
                'notices': [{k: n[k] for k in ('path', 'upstream_path', 'sha256', 'bytes')} for n in row['notices']]})
        for asset in self.policy['embedded_assets']:
            self.projection['embedded_assets'].append({k: asset[k] for k in
                ('id', 'package', 'version', 'feature', 'source_path', 'sha256', 'notices')})
        self.inventory = copy.deepcopy(api.expected_inventory(self.policy, self.projection, self.build))
        (self.notice_root / 'README.txt').write_bytes(api.README)
        self.save_inventory()
        self.addCleanup(patch.stopall)
        self.source_mock = patch.object(api.capture, 'source_evidence', return_value=self.source).start()
        self.build_mock = patch.object(api.native, 'validate_build', return_value=self.build).start()
        self.native_mock = patch.object(api.native, 'project', return_value=self.projection).start()
        patch.object(api, 'content_policy', return_value=self.policy).start()

    def save_inventory(self, raw=None, crlf=False):
        if raw is None:
            raw = (json.dumps(self.inventory, indent=2, ensure_ascii=False) + '\n').encode('utf-8')
        if crlf: raw = raw.replace(b'\n', b'\r\n')
        (self.notice_root / 'dependency-inventory.json').write_bytes(raw)
        self.build['builds']['ordinary']['inventory'] = api.bytes_record(raw)
        build_raw = api.canonical(self.build)
        (self.build_text / api.capture.EXPORT_NAME).write_bytes(build_raw)
        self.projection['bindings'] = {
            'inventory': api.bytes_record(raw), 'metadata': self.build['builds']['ordinary']['metadata'],
            'build_summary': api.bytes_record(build_raw)}
        self.native_path.write_bytes(api.canonical(self.projection))

    def prepare(self):
        return api.prepare(self.repo, self.sha, self.private, self.build_text, self.native_path, bundle=self.bundle)

    def verify(self):
        return api.verify(self.output, self.repo, self.sha, self.private, self.build_text, self.native_path, bundle=self.bundle)

    def test_exact_original_bytes_deterministic_packet_and_independent_rechecks(self):
        first = self.prepare(); second = self.prepare()
        self.assertEqual(api.canonical(first), api.canonical(second))
        for name, row in first['files'].items():
            self.assertEqual(base64.b64decode(row['content_base64']), (self.notice_root / name).read_bytes())
        self.assertTrue(all(first[name] is False for name in api.FLAGS))
        self.assertEqual(set(first['files']), {'dependency-inventory.json', 'README.txt', *self.policy['content_sources']})
        self.output.write_bytes(api.canonical(first))
        self.assertEqual(self.verify(), first)
        self.assertEqual(self.native_mock.call_count, 3)
        self.assertEqual(self.build_mock.call_count, 6)
        self.assertEqual(self.source_mock.call_count, 6)
        self.assertNotIn(str(self.private).encode(), self.output.read_bytes())

    def test_windows_crlf_inventory_and_readme_are_preserved_without_reconstruction(self):
        self.save_inventory(crlf=True)
        (self.notice_root / 'README.txt').write_bytes(api.README.replace(b'\n', b'\r\n'))
        packet = self.prepare()
        for name in ('dependency-inventory.json', 'README.txt'):
            self.assertEqual(base64.b64decode(packet['files'][name]['content_base64']),
                             (self.notice_root / name).read_bytes())
            self.assertIn(b'\r\n', base64.b64decode(packet['files'][name]['content_base64']))

    def test_native_failure_stops_packet_preparation(self):
        self.native_mock.side_effect = ValueError('native mismatch')
        with self.assertRaisesRegex(ValueError, 'native mismatch'): self.prepare()
        self.assertFalse(self.output.exists())

    def test_independent_source_and_build_failures_are_not_self_authenticated(self):
        for stub in (self.source_mock, self.build_mock):
            with self.subTest(stub=stub):
                stub.side_effect = ValueError('independent audit rejected')
                with self.assertRaisesRegex(ValueError, 'independent audit rejected'): self.prepare()
                stub.side_effect = None

    def test_unreviewed_text_and_binary_renamed_as_license_fail(self):
        name = sorted(self.policy['content_sources'])[0]; path = self.notice_root / name; original = path.read_bytes()
        for raw in (b'unreviewed plain text', b'MZ\x00fake binary', b'\xff\xfe\x00', b'private /home/alice/passwords',
                    b'private C:\\Users\\alice\\secret', b'private file:///tmp/key', b'-----BEGIN PRIVATE KEY-----'):
            with self.subTest(raw=raw):
                path.write_bytes(raw)
                with self.assertRaises((ValueError, UnicodeError)): self.prepare()
        path.write_bytes(original)

    def test_resealed_inventory_cannot_admit_unknown_notice_content(self):
        row = self.inventory['packages'][0]['notices'][0]
        (self.notice_root / row['path']).write_bytes(b'plausible license plus private content')
        row.update(api.bytes_record(b'plausible license plus private content'))
        self.projection['packages'][0]['notices'][0].update({k: row[k] for k in ('sha256', 'bytes')})
        self.save_inventory()
        with self.assertRaisesRegex(ValueError, 'pinned original content'): self.prepare()

    def test_unknown_nested_fields_and_private_source_strings_rejected(self):
        original = copy.deepcopy(self.inventory)
        mutations = [lambda v: v.update(secret='private data'),
                     lambda v: v['packages'][0].update(metadata={'workspace_root': '/home/alice'}),
                     lambda v: v['packages'][0].update(repository='file:///C:/Users/alice'),
                     lambda v: v['packages'][0].update(source_path_in_repository='/home/alice/source'),
                     lambda v: v['packages'][0]['notices'][0].update(secret='private data'),
                     lambda v: v['embedded_assets'][0].update(extra='/home/alice'),
                     lambda v: v.update(schema_version=True),
                     lambda v: v['packages'][0].update(unresolved=['private failure /tmp/token']),
                     lambda v: v['packages'][0].update(repository='https://github.com/user/private-secret')]
        for mutate in mutations:
            self.inventory = copy.deepcopy(original); mutate(self.inventory); self.save_inventory()
            with self.subTest(mutation=mutate), self.assertRaisesRegex(ValueError, 'unknown or changed fields'):
                self.prepare()

    def test_duplicate_inventory_keys_and_hidden_whitespace_rejected(self):
        raw = (self.notice_root / 'dependency-inventory.json').read_bytes()
        duplicate = raw.replace(b'"schema_version": 1', b'"schema_version": 1, "schema_version": 1')
        self.save_inventory(raw=duplicate)
        with self.assertRaisesRegex(ValueError, 'duplicate JSON key'): self.prepare()
        self.save_inventory(raw=raw + b' \t\t ')
        with self.assertRaisesRegex(ValueError, 'exact collector JSON encoding'): self.prepare()

    def test_missing_extra_empty_directory_and_casefold_aliases_rejected(self):
        p = self.notice_root / 'README.txt'; p.unlink()
        with self.assertRaisesRegex(ValueError, 'missing original'): self.prepare()
        p.write_bytes(api.README)
        for name in ('metadata.json', 'flightsim-app.exe', 'readme.TXT', 'licenses/extra.txt'):
            p = self.notice_root / name; p.write_bytes(b'extra')
            with self.subTest(name=name), self.assertRaises(ValueError): self.prepare()
            p.unlink()
        p = self.notice_root / 'private-empty'; p.mkdir()
        with self.assertRaisesRegex(ValueError, 'unreferenced notice directory'): self.prepare()

    def test_missing_reference_and_tampered_readme_are_rejected(self):
        path = self.notice_root / sorted(self.policy['content_sources'])[0]; raw = path.read_bytes(); path.unlink()
        with self.assertRaisesRegex(ValueError, 'missing original'): self.prepare()
        path.write_bytes(raw)
        (self.notice_root / 'README.txt').write_bytes(b'synthetic README')
        with self.assertRaisesRegex(ValueError, 'collector README changed'): self.prepare()

    def test_symlink_and_hardlink_are_not_original_regular_files(self):
        path = self.notice_root / 'README.txt'; path.unlink(); target = self.root / 'readme'; target.write_bytes(api.README)
        try: path.symlink_to(target)
        except OSError as error:
            if os.name == 'nt' and getattr(error, 'winerror', None) == 1314:
                self.skipTest('Windows symlink privilege unavailable')
            raise
        with self.assertRaisesRegex(ValueError, 'linked/reparse'): self.prepare()
        path.unlink(); os.link(target, path)
        with self.assertRaisesRegex(ValueError, 'independent bounded regular'): self.prepare()

    def test_path_alias_and_cross_platform_escape_rejected(self):
        for name in ('../secret', '/tmp/secret', 'licenses//LICENSE', 'licenses/./LICENSE',
                     'C:/Users/secret', 'licenses/CON', 'licenses/file.', 'licenses\\LICENSE'):
            with self.subTest(name=name), self.assertRaises(ValueError): api.relative(name)
        with self.assertRaisesRegex(ValueError, 'canonical absolute'):
            api.prepare(self.repo / '..' / 'repo', self.sha, self.private, self.build_text,
                        self.native_path, bundle=self.bundle)

    def test_feature_tokens_are_closed_even_if_native_projection_is_resealed(self):
        self.projection['packages'][0]['conservative_features'].append('private_secret')
        self.native_path.write_bytes(api.canonical(self.projection))
        with self.assertRaisesRegex(ValueError, 'known public package features'): self.prepare()

    def test_oversized_original_inventory_and_notice_rejected_before_read(self):
        for name in ('dependency-inventory.json', sorted(self.policy['content_sources'])[0]):
            path = self.notice_root / name; original = path.read_bytes()
            with path.open('wb') as stream: stream.truncate(api.MAX_TEXT_BYTES + 1)
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'bounded regular'): self.prepare()
            path.write_bytes(original)

    def test_native_source_and_inventory_binding_changes_are_rejected(self):
        original = copy.deepcopy(self.projection)
        for mutate in (lambda p: p.update(source_sha='b' * 40),
                       lambda p: p['bindings'].update(inventory=api.bytes_record(b'other')),
                       lambda p: p['bindings'].update(build_summary=api.bytes_record(b'other'))):
            mutate(self.projection); self.native_path.write_bytes(api.canonical(self.projection))
            with self.assertRaisesRegex(ValueError, 'native original bindings'): self.prepare()
            self.projection.clear(); self.projection.update(copy.deepcopy(original))
        self.native_path.write_bytes(b'{}\n')
        with self.assertRaisesRegex(ValueError, 'independent validation'): self.prepare()

    def test_packet_unknown_flags_content_manifest_and_binding_reseals_fail(self):
        packet = self.prepare()
        mutations = [lambda p: p.update(release_authorized=True), lambda p: p.update(extra='private'),
                     lambda p: p['bindings'].update(source=api.bytes_record(b'forged source')),
                     lambda p: p.update(file_count=True), lambda p: p['files'].pop('README.txt'),
                     lambda p: p['files']['README.txt'].update(content_base64=base64.b64encode(b'changed').decode())]
        for mutate in mutations:
            value = copy.deepcopy(packet); mutate(value); self.output.write_bytes(api.canonical(value))
            with self.subTest(mutate=mutate), self.assertRaises(ValueError): self.verify()
        raw = api.canonical(packet)
        self.output.write_bytes(raw.replace(b'"schema_version": 1', b'"schema_version": 1, "schema_version": 1'))
        with self.assertRaisesRegex(ValueError, 'duplicate JSON key'): self.verify()
        self.output.write_bytes(raw + b' ')
        with self.assertRaisesRegex(ValueError, 'noncanonical original packet'): self.verify()

    def test_all_packet_acceptance_flags_must_be_false(self):
        packet = self.prepare()
        for flag in api.FLAGS:
            for value in (True, 0, None):
                changed = {**packet, flag: value}
                with self.subTest(flag=flag, value=value), self.assertRaisesRegex(ValueError, 'cannot grant'):
                    api.validate_shape(changed)

    def test_private_original_mutation_after_preparation_invalidates_verification(self):
        self.output.write_bytes(api.canonical(self.prepare()))
        raw = (self.notice_root / 'dependency-inventory.json').read_bytes()
        (self.notice_root / 'dependency-inventory.json').write_bytes(raw + b'\n')
        with self.assertRaisesRegex(ValueError, 'original inventory bytes changed'): self.verify()

    def test_public_apache_urls_and_exact_source_notice_words_are_allowed(self):
        grant = (ROOT / 'LICENSE-APACHE').read_bytes()
        self.assertIn(b'http://www.apache.org/licenses/', grant)
        self.assertEqual(api.text_bytes(grant).encode(), grant)
        self.prepare()

    def test_actual_collector_schema_and_original_bytes_pass_the_content_gate(self):
        # Execute the real collector on a small synthetic graph with real
        # workspace/vendor sources. This is not a native build or graph claim.
        packages, nodes = [], []
        for item in self.policy['packages']:
            row = item['inventory_fields']; name = row['name']
            manifest = ROOT / ('crates' if name == 'flightsim-app' else 'vendor') / name / 'Cargo.toml'
            packages.append({'id': row['id'], 'name': name, 'version': row['version'],
                             'manifest_path': str(manifest), 'source': None,
                             'license': row['license_expression'], 'repository': row['repository']})
            nodes.append({'id': row['id'], 'features': ['default'] if name == 'flightsim-app'
                          else ['tonemapping_luts'], 'deps': []})
        app = next(row for row in nodes if row['id'].startswith('flightsim-app@'))
        core = next(row for row in nodes if row is not app)
        app['deps'] = [{'pkg': core['id'], 'dep_kinds': [{'kind': None, 'target': None}]}]
        metadata = {'workspace_root': str(ROOT), 'workspace_members': [app['id']],
                    'packages': packages, 'resolve': {'nodes': nodes}}
        metadata_path = self.root / 'synthetic-metadata.json'; metadata_path.write_bytes(api.canonical(metadata))
        output = self.root / 'actual-collector-notices'
        inventory = api.native.check.collector.collect(metadata_path, ROOT, output, api.native.check.TARGET, 'flightsim-app')
        policy = copy.deepcopy(self.policy)
        policy['embedded_assets'] = [row for row in policy['embedded_assets'] if row['package'] == 'bevy_core_pipeline']
        wanted = {n['path'] for row in inventory['packages'] + inventory['embedded_assets'] for n in row['notices']}
        policy['content_sources'] = {name: row for name, row in policy['content_sources'].items() if name in wanted}
        projection = copy.deepcopy(self.projection)
        projection['embedded_assets'] = [row for row in projection['embedded_assets'] if row['package'] == 'bevy_core_pipeline']
        build = copy.deepcopy(self.build)
        build['builds']['ordinary']['metadata'] = api.bytes_record(metadata_path.read_bytes())
        self.assertEqual(api.canonical(inventory), api.canonical(api.expected_inventory(policy, projection, build)))
        raw = (output / 'dependency-inventory.json').read_bytes()
        exported = api.original_files(output, inventory, policy)
        self.assertEqual(exported['dependency-inventory.json'], raw)
        self.assertEqual(len(exported), 15)

    def test_packet_and_decoded_budgets_are_enforced(self):
        packet = self.prepare()
        self.output.write_bytes(api.canonical(packet))
        with patch.object(api, 'MAX_PACKET_BYTES', self.output.stat().st_size - 1):
            with self.assertRaisesRegex(ValueError, 'bounded regular'): self.verify()
        with patch.object(api, 'MAX_TOTAL_BYTES', packet['decoded_bytes'] - 1):
            with self.assertRaisesRegex(ValueError, 'decoded original notice budget'): self.prepare()

    def test_second_independent_recheck_detects_late_source_mutation(self):
        changed = {**self.source, 'late_mutation': True}
        self.source_mock.side_effect = [self.source, changed]
        with self.assertRaisesRegex(ValueError, 'frozen originals changed'): self.prepare()


if __name__ == '__main__': unittest.main()
