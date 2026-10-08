"""UI boundary fixtures only. No test launches or claims Windows acceptance."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('windows_ui', Path(__file__).parents[1] / 'observe-analytical-windows-ui.py')
ui = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(ui)


class ObservableStateTests(unittest.TestCase):
    def test_map_needs_header_start_and_actual_selected_coordinate(self):
        self.assertFalse(ui.text_state('WORLD EXPLORER', 'map'))
        self.assertFalse(ui.text_state('Alps Tokyo WORLD EXPLORER START NEW FLIGHT LAT +35.55000 deg', 'map-alps'))
        self.assertTrue(ui.text_state('WORLD EXPLORER START NEW FLIGHT LAT +46.58000 deg LON +8.00000 deg', 'map-alps'))
        self.assertTrue(ui.text_state('WORLD EXPLORER START NEW FLIGHT LAT +35.55000 deg LON +139.78000 deg', 'map-tokyo'))

    def test_hud_and_pause_cannot_be_confused_with_map_or_camera_help(self):
        hud = 'EAS 0 kt ALT 100 ft VIEW COCKPIT'
        self.assertTrue(ui.text_state(hud, 'flight', 'COCKPIT'))
        self.assertFalse(ui.text_state(hud + ' CHASE camera option', 'flight', 'CHASE'))
        self.assertFalse(ui.text_state(hud + ' PAUSED', 'flight', 'COCKPIT'))
        self.assertTrue(ui.text_state(hud + ' PAUSED', 'paused-view', 'COCKPIT'))
        self.assertFalse(ui.text_state(hud + ' PAUSED WORLD EXPLORER START NEW FLIGHT', 'paused'))

    def test_legacy_notice_requires_the_missing_identity_disclosure(self):
        self.assertFalse(ui.text_state('LEGACY PARTIAL IDENTITY', 'legacy'))
        self.assertTrue(ui.text_state('LEGACY PARTIAL IDENTITY: historical yaw_rate_p was not recorded or verified', 'legacy'))

    def test_log_witnesses_distinguish_commit_pause_resume_and_restart(self):
        text = 'INFO flightsim_app: paused\nINFO flightsim_app: resumed\nINFO flightsim_app: restarted\n'
        text += 'INFO world flight: 35.55000,139.78000\nINFO new-flight aircraft: Swift Sport (swift-sport); model aircraft/swift_sport.glb\n'
        self.assertEqual(ui.log_counts(text), {'world_flight': 1, 'swift_commits': 1, 'paused': 1, 'resumed': 1, 'restarted': 1})
        self.assertEqual(ui.log_counts('info: replay: paused but not a live pause\n')['paused'], 0)

    def test_ocr_projection_rejects_boxes_outside_the_owned_client(self):
        value = {'schema_version': 1, 'engine': 'Windows.Media.Ocr', 'language': 'en-US', 'text': 'EAS ALT',
                 'words': [{'text': 'ALT', 'x': 5, 'y': 5, 'width': 20, 'height': 10}]}
        ui.ocr_value(json.dumps(value).encode(), 640, 480)
        for bad in (-1, float('nan'), 1000):
            value['words'][0]['x'] = bad
            with self.assertRaises(ValueError): ui.ocr_value(json.dumps(value).encode(), 640, 480)
        value['words'] = []; value['private_payload'] = 'not allowed'
        with self.assertRaises(ValueError): ui.ocr_value(json.dumps(value).encode(), 640, 480)


class NativeBoundaryTests(unittest.TestCase):
    def test_blank_capture_cannot_count_as_valid_rendering(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError): ui.write_png(root / 'blank.png', 640, 360, bytes(640 * 360 * 4))
            pixels = bytes(range(256)) * (640 * 360 * 4 // 256)
            result = ui.write_png(root / 'pattern.png', 640, 360, pixels)
            self.assertEqual((result['width'], result['height']), (640, 360))
            self.assertEqual(ui.q.candidate.validate_png(root / 'pattern.png'), result)

    @unittest.skipIf(sys.platform == 'win32', 'native tests require a separately reviewed actual desktop run')
    def test_non_windows_refuses_before_any_process_launch(self):
        with self.assertRaises(ValueError): ui.NativeWindow(12345, Path('/synthetic/flightsim-app.exe'))

    def test_driver_has_no_global_input_screen_capture_or_security_bypass(self):
        text = (Path(__file__).parents[1] / 'observe-analytical-windows-ui.py').read_text()
        for forbidden in ('.SendInput(', '.BitBlt(', '.SetThreadDesktop(', '.SwitchDesktop(', '.AttachThreadInput(',
                          '.GetDC(None)', '.GetDC(0)', 'ExecutionPolicy', 'shell=True', 'pyautogui'):
            self.assertNotIn(forbidden, text)
        helper = (Path(__file__).parents[1] / 'windows-ui-ocr.ps1').read_text()
        for forbidden in ('Invoke-WebRequest', 'Invoke-RestMethod', 'Set-ExecutionPolicy', 'Install-', 'Add-WindowsCapability'):
            self.assertNotIn(forbidden, helper)

    def test_close_does_not_require_the_closed_window_to_survive(self):
        class Native:
            def SendMessageTimeoutW(self, hwnd, message, *args):
                self.seen = (hwnd, message); return 1
            def IsWindow(self, hwnd): return False
        window = ui.NativeWindow.__new__(ui.NativeWindow); window.u = Native(); window.hwnd = 123
        calls = []; window.assert_owned = lambda: calls.append('owned')
        window.close()
        self.assertEqual(calls, ['owned']); self.assertEqual(window.u.seen, (123, 0x10))

    def test_destination_uses_observed_button_not_unimplemented_digit_shortcut(self):
        self.assertNotIn('tokyo', ui.KEYS); self.assertNotIn('alps', ui.KEYS)
        observer = ui.Observer.__new__(ui.Observer)
        clicks = []
        class Window:
            def click(self, *args): clicks.append(args)
        observer.window = Window()
        value = {'text': 'WORLD EXPLORER START NEW FLIGHT',
                 'words': [{'text': 'Alps', 'x': 100, 'y': 110, 'width': 40, 'height': 20}]}
        observer.last_ocr = (value, {'width': 1280, 'height': 900})
        observer.click_destination('ALPS')
        self.assertEqual(clicks, [(120, 120, (1280, 900))]); self.assertIsNone(observer.last_ocr)
        value['words'] *= 2; observer.last_ocr = (value, {'width': 1280, 'height': 900})
        with self.assertRaises(ValueError): observer.click_destination('ALPS')


class ExportValidatorTests(unittest.TestCase):
    def empty_fixture(self, root):
        private = root / 'private'; private.mkdir()
        qualification = root / 'qualification'; (qualification / 'extracted/swift-candidate').mkdir(parents=True)
        executable = qualification / 'extracted/swift-candidate/flightsim-app.exe'; executable.write_bytes(b'synthetic executable')
        prior = root / 'prior'; prior.mkdir(); (prior / ui.reviewed.EXPORT).write_bytes(b'synthetic prior evidence')
        value = {'schema_version': 1, 'identity': ui.IDENTITY, 'source_sha': 'a' * 40, 'source_tree': 'b' * 40,
                 'scenario': 'live', 'status': 'blocked', 'executable': ui.q.record(executable),
                 'prior_runtime_evidence': ui.q.record(prior / ui.reviewed.EXPORT), 'release_authorized': False,
                 'appearance_accepted': False, 'lifecycle_accepted': False, 'observations': [], 'normal_close_exit': None,
                 'cancellation_observation': 'not_established', 'limits': ui.LIMITS, 'final_streams': None}
        ui.q.write_json(private / 'ui-observation.json', value)
        return private, qualification, prior, value

    def test_blocked_public_export_has_closed_actual_exit_schema(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); private, qualification, prior, value = self.empty_fixture(root)
            with mock.patch.object(ui.q.capture, 'source_evidence', return_value={'source_tree': 'b' * 40}), \
                    mock.patch.object(ui.reviewed, 'verify_export', return_value={'status': 'final_bundle_runtime_observed_review_required'}):
                ui.export_observation(private, root / 'valid', root, 'a' * 40, qualification, prior)
                for index, payload in enumerate(({'private': 'payload'}, ['private'], 'private', True, 2**40)):
                    value['normal_close_exit'] = payload; ui.q.write_json(private / 'ui-observation.json', value)
                    ui.q.write_json(root / 'valid/ui-observation.json', value)
                    with self.subTest(payload=payload), self.assertRaises(ValueError):
                        ui.export_observation(private, root / ('invalid-' + str(index)), root, 'a' * 40, qualification, prior)
                    with self.assertRaises(ValueError): ui.verify_export(root / 'valid', private, root, 'a' * 40, qualification, prior)

    def test_complete_stream_tail_change_is_rejected_even_on_blocked_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); private, qualification, prior, value = self.empty_fixture(root)
            for name in ('app.stdout', 'app.stderr'): (private / name).write_bytes(b'')
            value['final_streams'] = {name: ui.q.record(private / name) for name in ('app.stdout', 'app.stderr')}
            ui.q.write_json(private / 'ui-observation.json', value)
            with mock.patch.object(ui.q.capture, 'source_evidence', return_value={'source_tree': 'b' * 40}), \
                    mock.patch.object(ui.reviewed, 'verify_export', return_value={'status': 'final_bundle_runtime_observed_review_required'}):
                ui.export_observation(private, root / 'valid', root, 'a' * 40, qualification, prior)
                (private / 'app.stderr').write_bytes(b'ERROR after last visual checkpoint\n')
                with self.assertRaises(ValueError): ui.verify_export(root / 'valid', private, root, 'a' * 40, qualification, prior)
                with self.assertRaises(ValueError): ui.read_log(private)

    def test_public_validator_rejects_relabelled_state_and_stale_resize_dimensions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); private, qualification, prior, value = self.empty_fixture(root)
            row = {'checkpoint': 'map-initial', 'state': 'flight', 'view': None, 'attempt': 1,
                   'image': {'width': 1280, 'height': 900, 'sha256': 'a' * 64}, 'ocr': {}, 'log_counts': {},
                   'log_prefixes': {}, 'source': 'owned_client_PrintWindow_and_Windows_OCR', 'visual_acceptance': False}
            value['observations'] = [row]
            with mock.patch.object(ui.q.capture, 'source_evidence', return_value={'source_tree': 'b' * 40}), \
                    mock.patch.object(ui.reviewed, 'verify_export', return_value={'status': 'final_bundle_runtime_observed_review_required'}):
                for state, width in (('flight', 1280), ('map-tokyo', 640)):
                    row['state'] = state; row['image']['width'] = width
                    ui.q.write_json(private / 'ui-observation.json', value)
                    with self.assertRaisesRegex(ValueError, 'fixed qualification case'):
                        ui.validate_observation(private, root, 'a' * 40, qualification, prior)
            self.assertEqual(ui.CHECKPOINTS['resize-640'], ('paused-view', 'COCKPIT', 640, 480))

    def test_duplicate_attempt_and_missing_commit_witness_cannot_complete_sequence(self):
        zero = {'world_flight': 0, 'swift_commits': 0, 'paused': 0, 'resumed': 0, 'restarted': 0}
        rows = [{'checkpoint': 'map-initial', 'attempt': 1, 'log_counts': zero},
                {'checkpoint': 'map-edit-alps', 'attempt': 1, 'log_counts': zero}]
        with self.assertRaisesRegex(ValueError, 'distinct'): ui.validate_sequence(rows, 'live', 'not_established', False)
        rows = [{'checkpoint': 'start-once', 'attempt': 5, 'log_counts': zero}]
        with self.assertRaisesRegex(ValueError, 'log witnesses'): ui.validate_sequence(rows, 'live', 'not_established', True)


if __name__ == '__main__': unittest.main()
