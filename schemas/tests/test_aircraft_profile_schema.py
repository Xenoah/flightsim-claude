"""Exercise the public schema against the same fixtures as AircraftProfile::load.

Requires Python 3 and jsonschema >= 4.18 (CI pins the tested 4.26.0).
Validation performs no downloads or app startup and adds no Rust dependency.
Run from any directory: python3 schemas/tests/test_aircraft_profile_schema.py
"""
import copy
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "schemas/aircraft-profile-v1.schema.json"
CORPUS = ROOT / "schemas/tests/aircraft-profile-v1-cases.json"


def materialize(base, case):
    """Apply the corpus's small fixture recipe, not runtime validation rules."""
    value = copy.deepcopy(base)
    for pointer, replacement in case.get("set", {}).items():
        parts = pointer.lstrip("/").split("/")
        parent = value
        for part in parts[:-1]:
            parent = parent[int(part)] if isinstance(parent, list) else parent[part]
        key = int(parts[-1]) if isinstance(parent, list) else parts[-1]
        parent[key] = replacement
    for pointer in case.get("remove", []):
        parts = pointer.lstrip("/").split("/")
        parent = value
        for part in parts[:-1]:
            parent = parent[part]
        del parent[parts[-1]]
    text = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    if "raw_replace" in case:
        old, new = case["raw_replace"]
        assert text.count(old) == 1, case["name"]
        text = text.replace(old, new, 1)
    if "pad_to_bytes" in case:
        padding = case["pad_to_bytes"] - len(text.encode("utf-8"))
        assert padding >= 0, case["name"]
        text += " " * padding
    return text


def reject_non_json_constant(token):
    raise ValueError(f"Not a JSON number: {token}")


class AircraftProfileSchemaContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
        cls.validator = Draft202012Validator(schema)

    def test_shared_acceptance_and_rejection_corpus(self):
        corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
        base = json.loads((ROOT / corpus["base"]).read_text(encoding="utf-8"))
        for case in corpus["cases"]:
            with self.subTest(case=case["name"]):
                # Schema validates the parsed JSON data model. Duplicate keys,
                # raw token spelling and serialized bytes are runtime checks.
                try:
                    value = json.loads(materialize(base, case),
                                       parse_constant=reject_non_json_constant)
                except ValueError:
                    self.assertFalse(case["schema_valid"])
                    continue
                errors = list(self.validator.iter_errors(value))
                self.assertEqual(not errors, case["schema_valid"],
                                 "; ".join(error.message for error in errors))

    def test_documented_examples_and_unchanged_builtins(self):
        profiles = list((ROOT / "docs/examples/aircraft-profiles").glob("*.json"))
        profiles += [ROOT / "assets/aircraft" / name
                     for name in ("light_single.json", "swift_sport.json")]
        self.assertEqual(len(profiles), 4)
        for path in profiles:
            with self.subTest(profile=path.name):
                self.validator.validate(json.loads(path.read_text(encoding="utf-8")))


if __name__ == "__main__":
    unittest.main()
