#!/usr/bin/env python3
"""Check the final supplemented private bundle against an existing frozen build.

No rebuild, inventory substitution, receipt generation or publication. The same
strict runtime body checks the newly staged/archive/extracted bytes and their
post-run integrity. A genuine completed dependency review must already exist.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

SPEC = importlib.util.spec_from_file_location('reviewed_bundle_qualification', Path(__file__).with_name('qualify-analytical-swift-windows.py'))
q = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(q)
assembler = q.load('reviewed_bundle_notices', 'assemble-analytical-review-notices.py')
require = q.require
IDENTITY = 'analytical-reviewed-bundle-runtime-evidence-v1'
EXPORT = 'reviewed-bundle.json'
COMMANDS = ['runtime-facts', 'stage', 'distribution-first', 'distribution-second', 'extracted-readiness', *q.SCENES,
            'absent-light-single', 'legacy-fixture', 'default-rejects-legacy', 'legacy-no-model']


def private_inputs(repo, expected, private):
    value = q.capture.read_private_json(private / 'inputs.json')
    require(set(value) == {'build_private', 'build_text', 'assembly', 'review'}, 'unexpected reviewed input field')
    paths = {key: Path(path) for key, path in value.items()}
    for path in paths.values():
        require(path.is_absolute(), 'reviewed inputs must use absolute paths'); q.capture.no_links(path)
    build = q.capture.validate_export(paths['build_text'], repo=repo, expected=expected, private=paths['build_private'])
    require(build['status'] == q.capture.PASS, 'frozen native build audit required')
    notices, source = assembler.validate_reviewed_assembly(repo, expected, paths['assembly'], paths['review'])
    require(q.record(notices / 'dependency-inventory.json') == build['builds']['analytic']['inventory'],
            'review inventory differs from the original frozen native build')
    return paths, notices, source


def verify_export(directory, repo, expected, private):
    for path in (directory, repo, private): q.capture.no_links(path)
    path = directory / EXPORT
    require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= q.MAX_JSON, 'invalid reviewed-bundle text export')
    raw = path.read_bytes(); value = json.loads(raw.decode('ascii'))
    require(set(value) == {'schema_version', 'identity', 'source_sha', 'source_tree', 'status', 'release_authorized',
                         'appearance_accepted', 'distribution_qualified', 'commands', 'images', 'bindings', 'runtime_facts', 'ui_capabilities'}, 'unexpected reviewed-bundle fields')
    require(type(value['schema_version']) is int and value['schema_version'] == 1 and value['identity'] == IDENTITY
            and value['source_sha'] == expected and q.capture.hex_string(expected, 40), 'wrong reviewed-bundle identity')
    require(all(value[key] is False for key in ('release_authorized', 'appearance_accepted', 'distribution_qualified')),
            'runtime evidence cannot authorize publication or subjective acceptance')
    require(value['status'] in ('failed', 'final_bundle_runtime_observed_review_required'), 'wrong reviewed-bundle status')
    require(raw == (json.dumps(value, sort_keys=True, indent=2) + '\n').encode('ascii')
            and value == q.capture.read_private_json(private / EXPORT), 'export differs from canonical private result')
    require(isinstance(value['commands'], list) and len(value['commands']) <= len(COMMANDS), 'unbounded commands')
    paths, notices, source = private_inputs(repo, expected, private)
    require(value['source_tree'] == source['source_tree'], 'reviewed-bundle source changed')
    bindings = {'original_build': q.record(paths['build_text'] / q.capture.EXPORT_NAME),
                'assembly': q.record(paths['assembly'] / 'assembly.json'), 'review': q.record(paths['review'])}
    specs = q.command_specifications(repo, private, 'runtime', expected, build_private=paths['build_private'],
                                    build_text=paths['build_text'], notices=notices, review_path=paths['review'])
    for index, command in enumerate(value['commands']):
        require(set(command) == {'id', 'outcome', 'exit_code', 'stdout', 'stderr'} and command['id'] == COMMANDS[index], 'wrong runtime command order')
        outcome, code = command['outcome'], command['exit_code']
        require(outcome in ('succeeded', 'failed', 'timed_out', 'termination_failed', 'launch_error'), 'invalid actual command outcome')
        require((type(code) is int and -(2**32) <= code <= 2**32) or
                (code is None and outcome in ('termination_failed', 'launch_error')), 'invalid actual command status')
        require(outcome not in ('succeeded', 'failed') or (outcome == 'succeeded') == (code == 0), 'command outcome/status disagree')
        require(all(q.capture.valid_record(command[key]) and command[key]['bytes'] <= 64 * 1024 * 1024
                    for key in ('stdout', 'stderr')), 'invalid command output binding')
        base = private / 'commands' / command['id']; journal = q.capture.read_private_json(base / 'journal.json')
        require(journal['command'] == specs[command['id']], 'runtime command differs from reviewed-bundle recipe')
        expected_cwd = private / 'unrelated-cwd' if command['id'] not in ('runtime-facts', 'stage', 'extracted-readiness', 'legacy-fixture') else repo
        require(journal['cwd'] == str(expected_cwd), 'reviewed runtime CWD changed')
        require(all(journal[key] == command[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr'))
                and all(q.record(base / key) == command[key] for key in ('stdout', 'stderr')), 'runtime journal/stream changed')
        if value['status'] != 'failed' or index < len(value['commands']) - 1:
            codes = (0,) if command['id'] in ('runtime-facts', 'stage', 'extracted-readiness') else q.accepted_codes(command['id'])
            require(command['outcome'] in ('succeeded', 'failed') and command['exit_code'] in codes, 'unaccepted command cannot advance')
    require(set(value['images']) <= {name + '.png' for name in q.SCENES}, 'unexpected reviewed-bundle image')
    optional = set()
    for field, module, folder in (('runtime_facts', q.runtime_facts, 'runtime-facts'), ('ui_capabilities', q.ui_capabilities, 'ui-capabilities')):
        binding = value[field]
        require(binding is None or (q.capture.valid_record(binding) and binding['bytes'] <= module.MAX_PUBLIC), 'invalid factual projection binding')
        if binding is not None:
            name = module.PROJECTION_NAME if field == 'runtime_facts' else module.EXPORT
            optional.add(name); path = directory / name; q.capture.no_links(path)
            require(path.is_file() and path.stat().st_nlink == 1 and q.record(path) == binding == q.record(private / folder / name), 'changed factual export')
            actual = (q.validate_runtime_facts(private, repo, expected, paths['build_private'], paths['build_text']) if field == 'runtime_facts'
                      else q.ui_capabilities.project(private / folder, repo, expected))
            require(path.read_bytes() == (json.dumps(actual, sort_keys=True, indent=2) + '\n').encode('ascii'), 'factual export differs from original inputs')
    require({item.name for item in directory.iterdir()} == {EXPORT, *value['images'], *optional}, 'unexpected reviewed-bundle export')
    total = 0
    for name, image in value['images'].items():
        path = directory / name; q.capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and q.candidate.validate_png(path) == image
                and q.record(path) == q.record(private / name), 'changed reviewed-bundle PNG')
        total += path.stat().st_size
    require(total <= q.MAX_IMAGES, 'reviewed-bundle PNG budget exceeded')
    if value['status'] != 'failed':
        require(all(value[field] is not None for field in ('runtime_facts', 'ui_capabilities')), 'missing runtime factual observations')
        require(len(value['commands']) == len(COMMANDS) and set(value['images']) == {name + '.png' for name in q.SCENES}, 'incomplete final runtime evidence')
        q.validate_runtime_observations(repo, private, expected, build_private=paths['build_private'], build_text=paths['build_text'],
                                        notices=notices, review_path=paths['review'])
        bindings['runtime'] = q.record(private / 'runtime-bindings.json')
    else:
        require(not value['images'], 'failed reviewed-bundle run cannot export a partial image pass')
    require(value['bindings'] == bindings, 'reviewed-bundle bindings changed')
    return value


def run(repo, expected, build_private, build_text, assembly, review, private, evidence):
    require(sys.platform == 'win32', 'native Windows execution required')
    roots = (repo, build_private, build_text, assembly, private, evidence)
    for root in roots:
        require(root.is_absolute(), 'absolute paths required'); q.capture.no_links(root)
    q.capture.disjoint(*roots)
    require(not private.exists() and not evidence.exists(), 'fresh final runtime directories required')
    private.mkdir(parents=True)
    q.write_json(private / 'inputs.json', {key: str(path) for key, path in
                 (('build_private', build_private), ('build_text', build_text), ('assembly', assembly), ('review', review))})
    paths, notices, source = private_inputs(repo, expected, private)
    q.capture.reject_configuration(repo, os.environ)
    executable = build_private / 'target-analytic' / q.check.TARGET / 'release/flightsim-app.exe'
    result = {'schema_version': 1, 'identity': IDENTITY, 'source_sha': expected, 'source_tree': source['source_tree'],
              'status': 'failed', 'release_authorized': False, 'appearance_accepted': False, 'distribution_qualified': False,
              'commands': [], 'images': {}, 'runtime_facts': None, 'ui_capabilities': None, 'bindings': {'original_build': q.record(build_text / q.capture.EXPORT_NAME),
              'assembly': q.record(assembly / 'assembly.json'), 'review': q.record(review)}}
    with q.capture.private_console(private):
        env = {**os.environ, 'RUSTFLAGS': '-D warnings', 'CARGO_INCREMENTAL': '0', 'CARGO_TERM_COLOR': 'never'}
        runner = q.Runner(repo, private, env, 'runtime'); runner.deadline = time.monotonic() + 3600
        try:
            q.collect_runtime_observations(runner, source, build_private, build_text)
            command = q.command_specifications(repo, private, 'runtime', expected, build_private=build_private,
                                               build_text=build_text, notices=notices, review_path=review)['stage']
            runner.run('stage', command, timeout=180)
            staged = private / 'swift-candidate'
            checks, images = q.check_staged_runtime(runner, source, build_private, build_text, executable, notices, staged, review)
            require(checks == {'extracted_bundle_isolation': 'observed_pass', 'extracted_runtime_acceptance': 'observed_pass'}, 'missing final runtime outcome')
            require(q.capture.source_evidence(repo, expected) == source, 'source changed during final runtime')
            result.update(status='final_bundle_runtime_observed_review_required', images=images)
            result['bindings']['runtime'] = q.record(private / 'runtime-bindings.json')
        except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
            q.write_json(private / 'failure.json', {'type': type(error).__name__, 'message': str(error)})
        for field, path in (('runtime_facts', private / 'runtime-facts' / q.runtime_facts.PROJECTION_NAME),
                            ('ui_capabilities', private / 'ui-capabilities' / q.ui_capabilities.EXPORT)):
            if path.is_file(): result[field] = q.record(path)
        result['commands'] = runner.commands
        q.write_json(private / EXPORT, result)
    evidence.mkdir(parents=True); q.write_json(evidence / EXPORT, result)
    for name in result['images']: shutil.copyfile(private / name, evidence / name)
    if result['runtime_facts']: shutil.copyfile(private / 'runtime-facts' / q.runtime_facts.PROJECTION_NAME, evidence / q.runtime_facts.PROJECTION_NAME)
    if result['ui_capabilities']: shutil.copyfile(private / 'ui-capabilities' / q.ui_capabilities.EXPORT, evidence / q.ui_capabilities.EXPORT)
    verify_export(evidence, repo, expected, private)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1]); parser.add_argument('--source-sha', required=True)
    for name in ('build-private', 'build-text', 'assembly', 'review', 'private', 'evidence', 'validate-evidence'):
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    try:
        require(args.private is not None, 'private evidence root required')
        if args.validate_evidence:
            verify_export(args.validate_evidence, args.repo.resolve(), args.source_sha, args.private)
            print('Final reviewed-bundle evidence validated; release_authorized=false.'); return 0
        require(all((args.build_private, args.build_text, args.assembly, args.review, args.evidence)), 'reviewed final-bundle inputs required')
        result = run(args.repo.resolve(), args.source_sha, args.build_private, args.build_text, args.assembly, args.review, args.private, args.evidence)
        print('Final reviewed bundle: ' + result['status'] + '; release_authorized=false.')
        return 0 if result['status'] != 'failed' else 1
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print('Final reviewed-bundle check blocked; no review/approval generated.', file=sys.stderr); return 1


if __name__ == '__main__': raise SystemExit(main())
