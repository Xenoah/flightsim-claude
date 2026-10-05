#!/usr/bin/env python3
"""Authoring-schema smoke/negative tests; runtime remains authoritative."""
import copy
import json
from pathlib import Path
import unittest
from jsonschema import Draft202012Validator

ROOT=Path(__file__).resolve().parents[2]
SCHEMA=json.loads((ROOT/'schemas/aircraft-package-v1.schema.json').read_text())
MANIFEST=json.loads((ROOT/'docs/examples/aircraft-packages/swift/manifest.json').read_text())

class PackageSchema(unittest.TestCase):
    def setUp(self):
        self.validator=Draft202012Validator(SCHEMA)
    def test_original(self):
        Draft202012Validator.check_schema(SCHEMA)
        self.validator.validate(MANIFEST)
    def test_closed_contract(self):
        for key,value in [('schema_version',2),('kind','terrain_dem'),('unknown',1),('version','01.0.0')]:
            m=copy.deepcopy(MANIFEST);m[key]=value
            self.assertTrue(list(self.validator.iter_errors(m)),key)
    def test_payload_types_and_bounds(self):
        for kind,size in [('profile',131073),('model',16777217),('documentation',65537)]:
            m=copy.deepcopy(MANIFEST)
            next(f for f in m['files'] if f['kind']==kind)['size_bytes']=size
            self.assertTrue(list(self.validator.iter_errors(m)),kind)
    def test_exact_profile_model_counts(self):
        for kind in ['profile','model']:
            m=copy.deepcopy(MANIFEST);m['files']=[f for f in m['files'] if f['kind']!=kind]
            self.assertTrue(list(self.validator.iter_errors(m)),kind)
            m=copy.deepcopy(MANIFEST);m['files'].append(next(f for f in m['files'] if f['kind']==kind))
            self.assertTrue(list(self.validator.iter_errors(m)),kind)

if __name__=='__main__':unittest.main()
