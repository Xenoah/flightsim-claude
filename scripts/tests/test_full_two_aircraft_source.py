"""Negative boundary tests for the proposed full source recipe; no real approval."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT/'scripts'/(name+'.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
policy = load('check-source-archive')
candidate = load('check-full-two-aircraft-source')

class FullSourceBoundaryTests(unittest.TestCase):
    def test_explicit_original_policy_requires_exact_bytes_profile_and_provenance(self):
        source = Path(__file__).resolve().parents[2]
        for mutation in ('valid', 'mesh', 'profile', 'provenance', 'rights-record', 'unexpected-policy', 'renamed-historical'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                repo = Path(tmp)
                def git(*args):
                    return subprocess.check_output(['git', '-C', str(repo), *args], stderr=subprocess.DEVNULL)
                git('init')
                for name in set(policy.REQUIRED + policy.ORIGINAL_HIGHWING_REQUIRED + (policy.DENIED_PATH,)):
                    target = repo/name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes((source/name).read_bytes())
                if mutation == 'mesh':
                    (repo/policy.DENIED_PATH).write_bytes(b'not the reviewed original mesh')
                elif mutation == 'profile':
                    with (repo/'assets/aircraft/light_single.json').open('ab') as f:
                        f.write(b' ')
                elif mutation == 'provenance':
                    (repo/'tools/original-highwing/evidence/source-manifest.json').unlink()
                elif mutation == 'rights-record':
                    path = repo/'docs/release/asset-rights-manifest.json'
                    record = json.loads(path.read_text())
                    next(a for a in record['assets'] if a['path'] == policy.DENIED_PATH)['review_state'] = 'unresolved'
                    path.write_text(json.dumps(record))
                elif mutation == 'renamed-historical':
                    (repo/'renamed.dat').write_bytes(b'historical-model-fixture')
                git('add', '.')
                git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'fixture')
                for fmt in ('zip', 'tar'):
                    with self.assertRaises(ValueError):
                        policy.verify(repo, 'HEAD', fmt=fmt)
                    kwargs = {'fmt': fmt, 'policy': policy.ORIGINAL_HIGHWING_POLICY}
                    if mutation == 'valid':
                        result = policy.verify(repo, 'HEAD', **kwargs)
                        self.assertTrue(result['original_highwing_included'])
                        self.assertFalse(result['publication_authorized'])
                        self.assertFalse(result['hosted_origin_attested'])
                    elif mutation == 'renamed-historical':
                        with patch.object(policy, 'DENIED_SHA256', policy.sha(b'historical-model-fixture')):
                            with self.assertRaises(ValueError):
                                policy.verify(repo, 'HEAD', **kwargs)
                    else:
                        if mutation == 'unexpected-policy':
                            kwargs['policy'] = 'allow-any-current-mesh'
                        with self.assertRaises(ValueError):
                            policy.verify(repo, 'HEAD', **kwargs)


    def test_source_preparation_cannot_be_reclassified_as_release_admission(self):
        # Only the manifest is materialized: every case must fail before source reads.
        original = json.loads((ROOT/candidate.MAP).read_text())
        mutations = [
            ('recipe', 'old-three-lut-positive-control'),
            ('release_admitted', True),
            ('publication_authorized', True),
            ('remaining_gates', []),
            ('base_commit', '0'*40),
        ]
        for field, value in mutations:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = dict(original)
                manifest[field] = value
                path = root/candidate.MAP
                path.parent.mkdir(parents=True)
                path.write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, 'wrong source proposal identity|source proposal overstates admission'):
                    candidate.verify(root)

    def test_light_single_recipe_cannot_silently_become_swift_or_reinhard(self):
        original = json.loads((ROOT/candidate.MAP).read_text())
        for field, value in [('default_aircraft','swift-sport'), ('tonemapping','Reinhard'),
                             ('bundled_aircraft',['swift-sport']), ('additional_features',['analytic-tonemapping'])]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = json.loads(json.dumps(original))
                manifest['build_recipe'][field] = value
                path = root/candidate.MAP
                path.parent.mkdir(parents=True)
                path.write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, 'wrong full release proposal recipe'):
                    candidate.verify(root)
