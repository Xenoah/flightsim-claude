"""Shared v4 schema/loader boundary corpus. Runtime-only rules are explicit.

Run with Python 3 and jsonschema>=4.18; no downloads or runtime asset loads.
"""
import copy
import json
from pathlib import Path
import unittest
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]


def materialize(base, case):
    value = copy.deepcopy(base)
    def assign(pointer, replacement):
        parts = pointer.lstrip('/').split('/')
        parent = value
        for part in parts[:-1]:
            parent = parent[int(part)] if isinstance(parent, list) else parent[part]
        key = int(parts[-1]) if isinstance(parent, list) else parts[-1]
        parent[key] = replacement
    for pointer, replacement in case.get('set', {}).items():
        assign(pointer, replacement)
    for recipe in case.get('repeat', []):
        assign(recipe['pointer'], [copy.deepcopy(recipe['value']) for _ in range(recipe['count'])])
    for pointer in case.get('remove', []):
        parts = pointer.lstrip('/').split('/')
        parent = value
        for part in parts[:-1]:
            parent = parent[int(part)] if isinstance(parent, list) else parent[part]
        del parent[parts[-1]]
    text = json.dumps(value, ensure_ascii=False, separators=(',', ':'))
    if 'raw_replace' in case:
        old, new = case['raw_replace']
        assert text.count(old) == 1, case['name']
        text = text.replace(old, new, 1)
    if 'pad_to_bytes' in case:
        padding = case['pad_to_bytes'] - len(text.encode('utf-8'))
        assert padding >= 0
        text += ' ' * padding
    return text


class ProfileV4Schema(unittest.TestCase):
    def test_shared_corpus(self):
        schema = json.loads((ROOT / 'schemas/aircraft-profile-v4.schema.json').read_text())
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        corpus = json.loads((ROOT / 'schemas/tests/aircraft-profile-v4-cases.json').read_text())
        base = json.loads((ROOT / corpus['base']).read_text())
        for case in corpus['cases']:
            with self.subTest(case=case['name']):
                value = json.loads(materialize(base, case))
                errors = list(validator.iter_errors(value))
                self.assertEqual(not errors, case['schema_valid'], '; '.join(error.message for error in errors))


if __name__ == '__main__':
    unittest.main()
