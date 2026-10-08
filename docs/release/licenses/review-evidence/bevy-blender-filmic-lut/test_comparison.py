"""Small adversarial checks for the factual comparison, using synthetic data."""
import struct
import tempfile
import unittest
from pathlib import Path
from verify_reproduction import DECODED_BYTES, compare_values, decode_reference


class ComparisonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.reference = struct.pack('<4e', 0.0, 0.5, 1.0, 1.0) * (64 ** 3)

    def test_exact_stream(self):
        report = compare_values(self.reference, self.reference)
        self.assertTrue(report['exact_bytes_equal'])
        self.assertEqual(report['different_values'], 0)

    def test_single_finite_change_is_counted(self):
        altered = struct.pack('<e', 0.25) + self.reference[2:]
        report = compare_values(self.reference, altered)
        self.assertFalse(report['exact_bytes_equal'])
        self.assertEqual((report['different_values'], report['different_pixels']), (1, 1))
        self.assertEqual(report['max_absolute_error'], 0.25)

    def test_signed_zero_is_not_byte_equality(self):
        altered = b'\x00\x80' + self.reference[2:]
        report = compare_values(self.reference, altered)
        self.assertFalse(report['exact_bytes_equal'])
        self.assertEqual(report['different_values'], 1)
        self.assertEqual(report['max_absolute_error'], 0.0)

    def test_wrong_size_rejected(self):
        with self.assertRaisesRegex(ValueError, 'size'):
            compare_values(self.reference, self.reference[:-1])

    def test_nan_and_alpha_rejected(self):
        for altered in (b'\x00\x7e' + self.reference[2:],
                        self.reference[:6] + b'\x00\x00' + self.reference[8:]):
            with self.subTest(kind=altered[:8]), self.assertRaises(ValueError):
                compare_values(self.reference, altered)

    def test_reference_identity_rejected_before_decode(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'synthetic-invalid.ktx2'
            path.write_bytes(b'not the reference')
            with self.assertRaisesRegex(ValueError, 'identity'):
                decode_reference(path)


if __name__ == '__main__':
    unittest.main()
