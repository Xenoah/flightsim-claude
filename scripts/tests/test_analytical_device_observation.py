"""P3's additive bindings, privacy allowlist and unchanged one-audit contract."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

ROOT=Path(__file__).resolve().parents[2]
def load(name, relative):
    spec=importlib.util.spec_from_file_location(name,ROOT/relative)
    result=importlib.util.module_from_spec(spec); spec.loader.exec_module(result); return result
p=load('device_observe_test','scripts/observe-device-failure.py')
fixture=load('device_p2_fixture','scripts/tests/test_analytical_present_observation.py')


class DeviceObservationTests(unittest.TestCase):
    def fixture(self,root):
        # Existing P2 synthetic authority fixture; it exercises the real binding
        # checks while platform/source validation are represented by mocks.
        args,context,report,exe=fixture.PresentObservationTests().fixture(root)
        stderr=args[2]/'commands/default-swift/stderr'
        stderr.write_bytes(b'ERROR wgpu_hal::auxil::dxgi::result: Signal fence failed: 0x887A0005\n'+stderr.read_bytes())
        journal=p.capture.read_private_json(stderr.parent/'journal.json')
        journal['stderr']=p.capture.file_record(stderr)
        p.capture.write_json(stderr.parent/'journal.json',journal)
        report['commands'][-1]['stderr']=journal['stderr']
        for path in (args[2]/'result.json',args[3]/'qualification.json'):p.capture.write_json(path,report)
        return args,context,report,exe

    def test_original_validator_once_and_no_repeated_audit_or_collection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);args,context,_,_=self.fixture(root);evidence=root/'evidence'
            with mock.patch.object(p.p2,'source_context',return_value=context),mock.patch.object(p.capture,'validate_summary'),mock.patch.object(p.capture,'validate_export',side_effect=AssertionError('no repeated target audit')):
                p.p2.validate_source(*args);p.observe(*args,evidence=evidence);p.validate_export(evidence,*args)
                self.assertEqual(context[0].validate_export.call_count,1)
            self.assertEqual({x.name for x in evidence.iterdir()},{p.ORIGIN,p.FACTS,p.p2.ORIGIN,p.p2.EXCERPT,p.p2.RECEIPT,p.p2.failure.EXPORT})
            facts=json.loads((evidence/p.FACTS).read_bytes())
            self.assertEqual(facts['rows'][0]['facts'],{'operation':'signal_fence','hresult':'0x887A0005'})
            origin=json.loads((evidence/p.ORIGIN).read_bytes())
            self.assertEqual(origin['device_error'],p.capture.file_record(evidence/p.FACTS))
            self.assertEqual(origin['present_observation'],p.capture.file_record(evidence/p.p2.ORIGIN))
            self.assertFalse(origin['complete_build_audit_repeated']);self.assertFalse(origin['platform_facts_recollected'])
            self.assertFalse(origin['runtime_accepted']);self.assertFalse(origin['release_authorized'])

    def test_executable_stream_journal_build_and_original_report_mutations_rejected(self):
        for target in ('executable','stderr','journal','build','report','receipt','source'):
            with self.subTest(target=target),tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary);args,context,_,exe=self.fixture(root)
                with mock.patch.object(p.p2,'source_context',return_value=context),mock.patch.object(p.capture,'validate_summary'):
                    p.p2.validate_source(*args)
                    paths={'executable':exe,'stderr':args[2]/'commands/default-swift/stderr','journal':args[2]/'commands/default-swift/journal.json',
                           'build':args[2]/'build-private/frozen-trees.json','report':args[3]/'qualification.json','receipt':args[-1]/p.p2.RECEIPT,'source':args[2]/'source.json'}
                    paths[target].write_bytes(paths[target].read_bytes()+b'CHANGED')
                    with self.assertRaises((ValueError,json.JSONDecodeError)):p.project(*args)

    def test_all_six_exports_reject_changed_facts_or_unexpected_payload(self):
        for filename in (p.ORIGIN,p.FACTS,p.p2.ORIGIN,p.p2.EXCERPT,p.p2.RECEIPT,p.p2.failure.EXPORT,'raw.log'):
            with self.subTest(filename=filename),tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary);args,context,_,_=self.fixture(root);evidence=root/'evidence'
                with mock.patch.object(p.p2,'source_context',return_value=context),mock.patch.object(p.capture,'validate_summary'):
                    p.p2.validate_source(*args);p.observe(*args,evidence=evidence)
                    (evidence/filename).write_bytes(p.canonical({'private':'injected','runtime_accepted':True}))
                    with self.assertRaises(ValueError):p.validate_export(evidence,*args)

    def test_old_scanner_count_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);args,context,_,_=self.fixture(root)
            with mock.patch.object(p.p2,'source_context',return_value=context),mock.patch.object(p.capture,'validate_summary'):
                p.p2.validate_source(*args)
                project=p.device.project_stream
                def tamper(*a,**kw):
                    value=project(*a,**kw);value['marker_lines_total']+=1;return value
                with mock.patch.object(p.device,'project_stream',side_effect=tamper),self.assertRaisesRegex(ValueError,'marker accounting differs'):p.project(*args)


if __name__=='__main__':unittest.main()
