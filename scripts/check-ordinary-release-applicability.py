#!/usr/bin/env python3
"""Enforce a separately completed content-scoped review's explicit conditions.

No review document is generated here. Native facts must come from the same-build
projector; the unchanged ordinary authorization gate remains independently required.
"""
from __future__ import annotations

from datetime import datetime
import importlib.util
from pathlib import Path


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


payload = load('project-ordinary-release-payload')
release, capture, require = payload.release, payload.capture, payload.require
PATH = 'docs/release/full-review-applicability.json'
KIND = 'ordinary_full_content_applicability_review_v1'
SECTIONS = {'source', 'packages', 'graph', 'headers', 'embedded_assets',
            'build_script_link_requests', 'platform', 'runtime_rust',
            'runtime_microsoft', 'runtime_final_link', 'runtime_query_outcomes'}


def digest(value):
    return payload.record(payload.canonical(value))['sha256']


def source_content(repo):
    """All canonical tracked content except four self-bound evidence destinations.

    This is a separate review applicability scope, not the ordinary gate's source
    inventory. The latter continues to cover EVERYTHING except its own receipt.
    Exact inventory/review/notices are independently bound by that gate and the
    payload projector; this record cannot recursively include its own digest.
    """
    _, tracked = release.source_inventory(repo)
    excluded = {PATH, release.AUTHORIZATION, release.DEPENDENCY_REVIEW}
    notice_root = str(Path(release.DEPENDENCY_INVENTORY).parent).replace('\\', '/') + '/'
    rows = []
    for name in sorted(tracked):
        if name in excluded or name.startswith(notice_root):
            continue
        path = payload.absolute(repo / payload.member_name(name))
        payload.independent_file(path)
        rows.append({'path': name, **capture.file_record(path)})
    require(rows, 'empty reviewed source-content boundary')
    return rows


def binding(value):
    require(type(value) is dict and set(value) == {'sha256', 'bytes'}
            and capture.valid_record(value), 'invalid review input binding')


def text(value):
    require(isinstance(value, str) and 0 < len(value.strip()) <= 8192,
            'missing bounded substantive review text')


def validate(repo, native):
    """Return matched-condition facts only; caller owns native recomputation."""
    repo = payload.absolute(repo)
    gate, _ = release.inspect(repo)
    require(gate['authorized'] is True and not gate['blockers'], 'ordinary authorization gate is blocked')
    _, tracked = release.source_inventory(repo)
    require(PATH in tracked, 'completed applicability record is not committed')
    review, review_record = payload.read_object(repo / release.DEPENDENCY_REVIEW)
    value, value_record = payload.read_object(repo / PATH)
    require(set(value) == {'schema_version', 'kind', 'status', 'reviewed_by', 'reviewed_at', 'scope',
                           'basis', 'dependency_review', 'source_content_files', 'native_content_sha256',
                           'inventory_policy', 'coverage'}
            and type(value['schema_version']) is int and value['schema_version'] == 1
            and value['kind'] == KIND and value['status'] == 'reviewed', 'completed named content review required')
    for key in ('reviewed_by', 'reviewed_at', 'scope'):
        text(value[key])
    parsed = datetime.fromisoformat(value['reviewed_at'].replace('Z', '+00:00'))
    require(parsed.tzinfo is not None, 'review date requires an explicit timezone')
    basis = value['basis']
    require(type(basis) is dict and set(basis) == {'source_sha', 'source_tree', 'native_projection',
                                                 'original_inventory', 'metadata_sha256'}, 'invalid review basis')
    for key in ('source_sha', 'source_tree'):
        require(capture.hex_string(basis[key], 40), 'review basis needs genuine original source identities')
    binding(basis['native_projection']); binding(basis['original_inventory'])
    require(capture.hex_string(basis['metadata_sha256'], 64), 'invalid original metadata identity')
    binding(value['dependency_review'])
    require(value['dependency_review'] == review_record and review['status'] == 'reviewed',
            'applicability record belongs to another dependency review')
    require(payload.canonical(value['source_content_files']) == payload.canonical(source_content(repo)), 'reviewed source content changed')
    content = native['content_view']
    require(type(content) is dict and set(content) == {'schema_version', 'kind', 'sections'}
            and type(content['schema_version']) is int and content['schema_version'] == 2
            and content['kind'] == 'ordinary_native_content_view_not_approval'
            and set(content['sections']) == SECTIONS, 'unknown native content view')
    expected = value['native_content_sha256']
    require(type(expected) is dict and set(expected) == SECTIONS
            and all(capture.hex_string(item, 64) for item in expected.values()),
            'every complete native-content section must have a reviewed condition')
    actual = {key: digest(item) for key, item in content['sections'].items()}
    require(expected == actual, 'native/source/runtime content differs from reviewed applicability')
    comparison = native['payload_projection']['inventory_comparison']
    require(basis['original_inventory'] == comparison['original']
            and basis['metadata_sha256'] == comparison['original_metadata_sha256'],
            'review basis is not the preserved original inventory')
    require(review['inventory_sha256'] == comparison['original']['sha256'], 'dependency review inventory changed')
    require(comparison['all_other_fields_equal'] is True
            and set(comparison['different_fields']) <= {'metadata_sha256'}, 'inventory obligations changed')
    policy = value['inventory_policy']
    require(type(policy) is dict and set(policy) == {'allow_metadata_digest_change', 'allow_encoding_change',
                                                   'rationale', 'evidence'}
            and type(policy['allow_metadata_digest_change']) is bool
            and type(policy['allow_encoding_change']) is bool, 'invalid explicit inventory applicability policy')
    text(policy['rationale'])
    require(type(policy['evidence']) is list and 0 < len(policy['evidence']) <= 64, 'review evidence references required')
    for reference in policy['evidence']:
        text(reference)
    if comparison['different_fields']:
        require(policy['allow_metadata_digest_change'], 'raw metadata digest change is outside reviewed conditions')
    if not comparison['raw_bytes_equal']:
        require(policy['allow_encoding_change'], 'raw inventory encoding change is outside reviewed conditions')
    coverage = value['coverage']
    require(type(coverage) is dict and set(coverage) == {'source_provenance_complete', 'native_runtime_coverage_complete',
                                                      'unresolved_conditions', 'reason', 'evidence'}
            and coverage['source_provenance_complete'] is True
            and coverage['native_runtime_coverage_complete'] is True
            and coverage['unresolved_conditions'] == [], 'substantive source/native conditions remain unresolved')
    text(coverage['reason'])
    require(type(coverage['evidence']) is list and 0 < len(coverage['evidence']) <= 64, 'native coverage evidence required')
    for reference in coverage['evidence']:
        text(reference)
    # Re-read all committed input bindings; this API never amends the originals.
    require(payload.read_object(repo / PATH)[1] == value_record
            and payload.read_object(repo / release.DEPENDENCY_REVIEW)[1] == review_record
            and payload.canonical(value['source_content_files']) == payload.canonical(source_content(repo))
            and release.inspect(repo)[0] == gate, 'review/source changed during applicability comparison')
    return {'schema_version': 1, 'kind': 'ordinary_review_conditions_matched_not_approval',
            'conditions_matched': True, 'applicability_record': value_record,
            'dependency_review': review_record, 'basis': basis,
            'native_content_sha256': actual, 'inventory_comparison': comparison,
            'metadata_difference_cause': comparison['metadata_difference_cause'],
            'release_authorized': False, 'dependency_review_approved': False,
            'runtime_accepted': False, 'appearance_accepted': False}
