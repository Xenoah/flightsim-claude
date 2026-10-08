"""Run only the selected official Blender evaluator with explicit OCIO input."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-directory', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    package = root / 'blender-3.4.1-linux-x64'
    executable = package / 'blender'
    config = package / '3.4/datafiles/colormanagement/config.ocio'
    script = root / 'reproduce.py'
    output = args.output_directory.resolve()
    if output.exists():
        raise ValueError('output directory must not exist')
    environment = {key: value for key, value in os.environ.items() if not key.startswith('OCIO')}
    environment['OCIO'] = str(config)
    command = [str(executable), '--background', '--factory-startup', '-noaudio',
               '--python-exit-code', '1', '--python', str(script), '--', str(output)]
    result = subprocess.run(command, env=environment, text=True, capture_output=True, check=True)
    acknowledgement = 'Color management: Using ' + str(config) + ' as a configuration file'
    if acknowledgement not in result.stdout.splitlines():
        raise ValueError('Blender did not acknowledge the exact pinned OCIO configuration')
    observation = output / 'observation.json'
    report = {
        'kind': 'actual_pinned_evaluator_invocation_not_authorization',
        'process_exit_code': result.returncode,
        'explicit_ocio_configuration_acknowledged': True,
        'ambient_ocio_variables_removed_before_start': True,
        'command_template': ['blender-3.4.1-linux-x64/blender', '--background', '--factory-startup',
                             '-noaudio', '--python-exit-code', '1', '--python', 'reproduce.py',
                             '--', '<fresh-output-directory>'],
        'ocio_configuration_relative_path': '3.4/datafiles/colormanagement/config.ocio',
        'reproduction_script_sha256': hashlib.sha256(script.read_bytes()).hexdigest(),
        'observation_sha256': hashlib.sha256(observation.read_bytes()).hexdigest(),
        'stimulus_sha256': hashlib.sha256((root / 'stimulus.rgba32f').read_bytes()).hexdigest(),
        'release_authorized': False,
    }
    (output / 'invocation.json').write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    print(json.dumps(report, sort_keys=True))


if __name__ == '__main__':
    main()
