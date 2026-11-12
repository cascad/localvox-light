#!/usr/bin/env python3
"""Build the pinned CPU ONNX Runtime SDK on Intel macOS; reuse verified output."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

VERSION = '1.24.2'
COMMIT = '058787ceead760166e3c50a0a4cba8a833a6f53f'
SOURCE = 'https://github.com/microsoft/onnxruntime.git'
LIBRARY = f'libonnxruntime.{VERSION}.dylib'
STAMP = 'runtime.json'
IDENTITY = {'schema': 1, 'version': VERSION, 'commit': COMMIT,
            'arch': 'x86_64', 'deployment': '13.3'}
LICENSES = ('LICENSE', 'ThirdPartyNotices.txt')


def digest(file):
    with file.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verified(output):
    """Reject stale, incomplete or altered caches before linking/executing them."""
    try:
        stamp = json.loads((output / STAMP).read_text(encoding='utf-8'))
        if not isinstance(stamp, dict):
            return False
        if any(stamp.get(k) != v for k, v in IDENTITY.items()):
            return False
        entries = stamp['files']
        names = {entry['name'] for entry in entries}
        required = {LIBRARY, 'libonnxruntime.dylib', *(f'licenses/onnxruntime/{name}' for name in LICENSES)}
        actual = {p.relative_to(output).as_posix() for p in output.rglob('*') if p.is_file()}
        if len(names) != len(entries) or not required.issubset(names) or actual != names | {STAMP}:
            return False
        for entry in entries:
            file = output / entry['name']
            if not file.resolve().is_relative_to(output.resolve()) or file.is_symlink():
                return False
            if digest(file) != entry['sha256']:
                return False
        return True
    except (OSError, ValueError, KeyError, TypeError):
        return False


def run(arguments, *, capture=False, timeout=900):
    result = subprocess.run([str(a) for a in arguments], check=True, text=True,
                            stdout=subprocess.PIPE if capture else None, timeout=timeout)
    return result.stdout.strip() if capture else ''


def build(output, jobs=2):
    if platform.system() != 'Darwin' or platform.machine() != 'x86_64':
        raise RuntimeError('This source build requires a native Intel macOS host')
    if jobs < 1:
        raise ValueError('jobs must be positive')
    output = output.resolve()
    if verified(output):
        return {'reused': True, 'output': str(output), **IDENTITY}
    if output.exists():
        raise RuntimeError(f'Unverified runtime output: {output}; choose an empty cache directory')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='localvox-ort-build-', dir=output.parent) as temporary:
        work = Path(temporary)
        source, compiled, stage = work / 'source', work / 'build', work / 'sdk'
        run(['git', 'clone', '--depth', '1', '--branch', f'v{VERSION}',
             '--recurse-submodules', '--shallow-submodules', SOURCE, source])
        if run(['git', '-C', source, 'rev-parse', 'HEAD'], capture=True, timeout=30) != COMMIT:
            raise RuntimeError('ONNX Runtime tag does not match the pinned official commit')
        run([sys.executable, source / 'tools/ci_build/build.py', '--build_dir', compiled,
             '--config', 'Release', '--update', '--build', '--skip_tests',
             '--skip_submodule_sync', '--build_shared_lib', '--parallel', str(jobs),
             '--compile_no_warning_as_error', '--cmake_extra_defines',
             'CMAKE_OSX_ARCHITECTURES=x86_64', 'CMAKE_OSX_DEPLOYMENT_TARGET=13.3',
             'onnxruntime_BUILD_UNIT_TESTS=OFF'], timeout=7200)
        candidates = [p for p in compiled.rglob(LIBRARY) if not p.is_symlink()]
        if len(candidates) != 1:
            raise RuntimeError(f'Expected exactly one built {LIBRARY}, got {len(candidates)}')
        library = candidates[0]
        if run(['lipo', '-archs', library], capture=True, timeout=30) != 'x86_64':
            raise RuntimeError('Built runtime has the wrong architecture')
        stage.mkdir()
        # Materialize upstream linker/SONAME aliases as ordinary files. Our
        # bundles deliberately contain no links, but need every Mach-O name.
        for file in library.parent.glob('*.dylib'):
            if file.is_file():
                shutil.copy2(file, stage / file.name)
        notices = stage / 'licenses/onnxruntime'
        notices.mkdir(parents=True)
        for name in LICENSES:
            shutil.copy2(source / name, notices / name)
        files = [{'name': p.relative_to(stage).as_posix(), 'sha256': digest(p)}
                 for p in sorted(stage.rglob('*')) if p.is_file()]
        (stage / STAMP).write_text(json.dumps({**IDENTITY, 'files': files}, indent=2) + '\n', encoding='utf-8')
        if not verified(stage):
            raise RuntimeError('Incomplete built runtime SDK')
        os.rename(stage, output)
    return {'reused': False, 'output': str(output), **IDENTITY}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--jobs', type=int, default=2)
    args = parser.parse_args()
    print(json.dumps(build(args.output, args.jobs)))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
