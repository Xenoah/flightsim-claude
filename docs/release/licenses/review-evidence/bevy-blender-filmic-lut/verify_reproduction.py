"""Compare the one pinned Filmic candidate with the unchanged reference.

Only reads reference/run inputs. Writes a bounded factual report, never an asset,
notice-acceptance decision, or publication authorization. Requires libzstd.
"""
import argparse
import ctypes
import ctypes.util
import hashlib
import json
import math
from pathlib import Path
import struct

REFERENCE_SHA256 = "a81a2462182bc8499d1a222345a72e7c4f1fabc2cb23d9c543caa567ccba7ad7"
DECODED_SHA256 = "b8e2f0a26ee58edda1cf54e591175e8688ae21550e4d16af4c409cfdf5d919dd"
DECODED_BYTES = 64 ** 3 * 4 * 2
MAGIC = b"\xabKTX 20\xbb\r\n\x1a\n"


def record(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def decode_reference(path):
    data = path.read_bytes()
    if record(data) != {"bytes": 308905, "sha256": REFERENCE_SHA256}:
        raise ValueError("reference identity changed")
    if data[:12] != MAGIC:
        raise ValueError("unexpected KTX2 identifier")
    fields = struct.unpack_from("<13I2Q", data, 12)
    if fields != (97, 1, 64, 64, 64, 0, 1, 1, 2, 104, 12, 0, 0, 0, 0):
        raise ValueError("unexpected KTX2 layout")
    offset, compressed_length, decoded_length = struct.unpack_from("<3Q", data, 80)
    if (offset, compressed_length, decoded_length) != (116, 308789, DECODED_BYTES):
        raise ValueError("unexpected KTX2 level")
    compressed = data[offset:]
    if len(compressed) != compressed_length:
        raise ValueError("truncated or padded KTX2 level")
    lib = ctypes.CDLL(ctypes.util.find_library("zstd"))
    lib.ZSTD_decompress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t]
    lib.ZSTD_decompress.restype = ctypes.c_size_t
    lib.ZSTD_isError.argtypes = [ctypes.c_size_t]
    lib.ZSTD_isError.restype = ctypes.c_uint
    lib.ZSTD_versionString.restype = ctypes.c_char_p
    destination = ctypes.create_string_buffer(DECODED_BYTES)
    length = lib.ZSTD_decompress(destination, DECODED_BYTES, compressed, len(compressed))
    if lib.ZSTD_isError(length) or length != DECODED_BYTES:
        raise ValueError("Zstandard decoding failed")
    decoded = destination.raw
    if record(decoded)["sha256"] != DECODED_SHA256:
        raise ValueError("decoded reference identity changed")
    return data, decoded, lib.ZSTD_versionString().decode("ascii")


def compare_values(reference, candidate):
    if len(candidate) != DECODED_BYTES or len(reference) != DECODED_BYTES:
        raise ValueError("unexpected decoded stream size")
    different_values = 0
    different_pixels = set()
    maximum = total = squares = 0.0
    for index, ((left,), (right,)) in enumerate(zip(
        struct.iter_unpack("<e", reference), struct.iter_unpack("<e", candidate)
    )):
        if not math.isfinite(left) or not math.isfinite(right):
            raise ValueError("non-finite decoded value")
        if index % 4 == 3 and (left != 1.0 or right != 1.0):
            raise ValueError("unexpected alpha")
        if reference[index * 2:index * 2 + 2] != candidate[index * 2:index * 2 + 2]:
            different_values += 1
            different_pixels.add(index // 4)
        difference = abs(left - right)
        maximum = max(maximum, difference)
        total += difference
        squares += difference * difference
    values = DECODED_BYTES // 2
    return {
        "values": values, "pixels": 64 ** 3,
        "exact_bytes_equal": reference == candidate,
        "different_values": different_values, "different_pixels": len(different_pixels),
        "max_absolute_error": maximum, "mean_absolute_error": total / values,
        "rmse": math.sqrt(squares / values),
        "candidate_decoded": record(candidate), "reference_decoded": record(reference),
    }


def verify(reference_path, runs):
    initial, reference, decoder_version = decode_reference(reference_path)
    results = []
    observations = []
    for run in runs:
        observation = json.loads((run / "observation.json").read_text())
        if (observation["kind"] != "bounded_cpu_filmic_recipe_observation_not_rights_clearance"
                or observation["version"] != "3.4.1"
                or observation["build_hash"] != "55485cb379f7"
                or observation["evaluator_sha256"] != "40999207b8b7f8a43a8902535cd6e3a35724b9590cb08715b161c39c5a0119f6"
                or observation["explicit_ocio_config"] != "3.4/datafiles/colormanagement/config.ocio"
                or observation["pinned_inputs_unchanged"] is not True
                or observation["renderer_invoked"] is not False
                or observation["release_authorized"] is not False):
            raise ValueError("unexpected observation provenance")
        invocation = json.loads((run / "invocation.json").read_text())
        if (invocation["process_exit_code"] != 0
                or invocation["explicit_ocio_configuration_acknowledged"] is not True
                or invocation["ambient_ocio_variables_removed_before_start"] is not True
                or invocation["release_authorized"] is not False
                or invocation["observation_sha256"] != record((run / "observation.json").read_bytes())["sha256"]):
            raise ValueError("unexpected invocation provenance")
        expected_files = {"stimulus.exr", "filmic-srgb.exr", "linear-output.exr",
                          "output.rgba32f", "output.rgba16f"}
        if set(observation["files"]) != expected_files:
            raise ValueError("unexpected observation file set")
        for name in sorted(expected_files):
            if record((run / name).read_bytes()) != observation["files"][name]:
                raise ValueError("observed output changed")
        results.append(compare_values(reference, (run / "output.rgba16f").read_bytes()))
        observations.append(observation)
    if reference_path.read_bytes() != initial:
        raise ValueError("reference changed during comparison")
    return {
        "kind": "actual_pinned_cpu_reproduction_comparison_not_authorization",
        "reference_container": record(initial), "zstd_decoder_version": decoder_version,
        "format": "64x64x64 top-row-first little-endian RGBA16F",
        "candidate_version_count": 1, "independent_process_runs": len(runs),
        "run_comparisons": results,
        "all_recorded_outputs_deterministic": all(item == observations[0] for item in observations),
        "reference_unchanged": True, "existing_lut_modified": False,
        "existing_lut_cleared": False, "whole_release_rights_review_complete": False,
        "release_authorized": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--run", required=True, type=Path, action="append")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    report = verify(args.reference, args.run)
    args.output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    return 0 if all(item["exact_bytes_equal"] for item in report["run_comparisons"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
