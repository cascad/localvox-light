#!/usr/bin/env python3
"""Build a complete, relocatable release archive; never reads .env or models."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
BINARIES = ('localvox-light', 'localvox-process', 'localvox-tui')
SCRIPTS = ('install-release.ps1', 'install-release.sh', 'fetch-models.ps1',
           'fetch-models.sh', 'setup-ollama.ps1', 'setup-ollama.sh', 'models.json', 'setup-mpv.mjs')


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def run(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()


def relocate_macos(folder):
    libraries = {p.name: p for p in folder.glob('*.dylib')}
    executables = [folder / name for name in (*BINARIES, 'localvox-desktop')]
    for file in [*libraries.values(), *executables]:
        if not file.is_file():
            continue
        if file.name in libraries:
            run('install_name_tool', '-id', '@rpath/' + file.name, str(file))
        dependencies = run('otool', '-L', str(file)).splitlines()
        for line in dependencies:
            if ' (compatibility version ' not in line:
                continue
            dependency = line.strip().split(' (compatibility version ', 1)[0]
            name = Path(dependency).name
            if name in libraries:
                # An install ID is not a dependency on itself.
                if name != file.name:
                    run('install_name_tool', '-change', dependency,
                        '@loader_path/' + name, str(file))
            elif not dependency.startswith(('/System/Library/', '/usr/lib/')):
                raise RuntimeError(f'Unbundled macOS dependency: {file.name}: {dependency}')
        # Mach-O mutations invalidate existing signatures, including Rust ad-hoc ones.
        run('codesign', '--force', '--sign', '-', str(file))


def stage(source, native, folder, target, desktop=False, extra_native=()):
    for directory in extra_native:
        if not directory.is_dir():
            raise RuntimeError(f'Missing native SDK directory: {directory}')
    windows = 'windows' in target
    suffix = '.exe' if windows else ''
    for name in (*BINARIES, *(('localvox-desktop',) if desktop else ())):
        file = source / (name + suffix)
        if not file.is_file():
            raise RuntimeError(f'Missing executable: {file}')
        shutil.copy2(file, folder / file.name)
        (folder / file.name).chmod(0o755)
    patterns = ('*.dll',) if windows else (('*.dylib',) if 'darwin' in target else ('*.so', '*.so.*'))
    for directory in (native, source, *extra_native):
        for pattern in patterns:
            for file in directory.glob(pattern):
                if file.is_file():
                    shutil.copy2(file, folder / file.name)
    required = ('libvosk.dll', 'libgcc_s_seh-1.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll') if windows else (
        ('libvosk.dylib',) if 'darwin' in target else ('libvosk.so',))
    for name in required:
        if not (folder / name).is_file():
            raise RuntimeError(f'Missing native dependency: {name}')
    if 'darwin' in target:
        relocate_macos(folder)
    elif not windows:
        for file in (folder / (b + suffix) for b in (*BINARIES, *(('localvox-desktop',) if desktop else ()))):
            old = run('patchelf', '--print-rpath', str(file))
            run('patchelf', '--set-rpath', ':'.join(dict.fromkeys(['$ORIGIN', *filter(None, old.split(':'))])), str(file))
    shutil.copy2(ROOT / '.env.defaults', folder)
    shutil.copytree(ROOT / 'licenses', folder / 'licenses')
    for directory in extra_native:
        notices = directory / 'licenses'
        if notices.is_dir():
            shutil.copytree(notices, folder / 'licenses', dirs_exist_ok=True)
    shutil.copy2(ROOT / 'LICENSE', folder)
    (folder / 'scripts').mkdir()
    for script in SCRIPTS:
        shutil.copy2(ROOT / 'scripts' / script, folder / 'scripts' / script)
    (folder / 'docs').mkdir()
    for name in ('terminal-workspace.md', 'release-installation.md'):
        shutil.copy2(ROOT / 'docs' / name, folder / 'docs' / name)
    files = [{'name': p.relative_to(folder).as_posix(), 'bytes': p.stat().st_size, 'sha256': digest(p)}
             for p in sorted(folder.rglob('*')) if p.is_file()]
    (folder / 'bundle.json').write_text(json.dumps({'schema': 1, 'target': target, 'files': files}, indent=2) + '\n', encoding='utf-8')


def smoke(folder, target):
    suffix = '.exe' if 'windows' in target else ''
    for name, flag in [('localvox-light', '--help'), ('localvox-process', '--worker-protocol'), ('localvox-tui', '--version')]:
        result = subprocess.run([str(folder / (name + suffix)), flag], cwd=folder,
                                capture_output=True, text=True, timeout=30)
        if result.returncode:
            raise RuntimeError(f'Packaged {name} failed ({result.returncode}): {result.stderr}')
        if name == 'localvox-process' and 'localvox-worker/3' not in result.stdout:
            raise RuntimeError('Incompatible packaged worker')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--native', type=Path, default=ROOT / 'vosk-lib')
    parser.add_argument('--extra-native', type=Path, action='append', default=[])
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--desktop', action='store_true')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    archive = args.output.resolve() / ('localvox-' + args.target + ('.zip' if 'windows' in args.target else '.tar.gz'))
    with tempfile.TemporaryDirectory(prefix='localvox-pack-', dir=args.output) as temp:
        folder = Path(temp).resolve()
        stage(args.source.resolve(), args.native.resolve(), folder, args.target, args.desktop,
              tuple(p.resolve(strict=True) for p in args.extra_native))
        smoke(folder, args.target)
        if archive.suffix == '.zip':
            with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as out:
                for file in sorted(folder.rglob('*')):
                    if file.is_file():
                        out.write(file, file.relative_to(folder).as_posix())
        else:
            with tarfile.open(archive, 'w:gz') as out:
                for file in sorted(folder.iterdir()):
                    out.add(file, arcname=file.name)
    print(archive)


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
