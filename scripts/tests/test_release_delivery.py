"""Delivery contracts. No downloads, models, devices, or developer configuration."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('package_release', ROOT / 'scripts/package-release.py')
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
WINDOWS = 'x86_64-pc-windows-msvc'
POWERSHELL = shutil.which('pwsh') or shutil.which('powershell')
BASH = os.getenv('LOCALVOX_TEST_BASH') or (r'C:\Program Files\Git\bin\bash.exe' if os.name == 'nt' else shutil.which('bash'))


class PackageContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-delivery-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source, self.native, self.out = [self.root / name for name in ('source', 'native', 'out')]
        for folder in (self.source, self.native, self.out):
            folder.mkdir()

    def windows_input(self):
        for name in package.BINARIES:
            (self.source / (name + '.exe')).write_bytes(b'fake executable: ' + name.encode())
        for name in ('libvosk.dll', 'libgcc_s_seh-1.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll'):
            (self.native / name).write_bytes(b'native fixture')

    def test_bundle_contains_consistent_helpers_native_and_hashes_but_no_user_env(self):
        self.windows_input()
        (self.source / 'DirectML.dll').write_bytes(b'optional native fixture')
        (self.source / '.env').write_text('private configuration must not be packaged')
        package.stage(self.source, self.native, self.out, WINDOWS)
        manifest = json.loads((self.out / 'bundle.json').read_text())
        self.assertEqual(manifest['target'], WINDOWS)
        self.assertFalse((self.out / '.env').exists())
        self.assertIn('DirectML.dll', [p['name'] for p in manifest['files']])
        self.assertTrue((self.out / 'scripts/fetch-models.ps1').is_file())
        for entry in manifest['files']:
            self.assertEqual(entry['sha256'], package.digest(self.out / entry['name']))
            self.assertEqual(entry['bytes'], (self.out / entry['name']).stat().st_size)

    def test_missing_worker_is_not_a_valid_package(self):
        self.windows_input()
        (self.source / 'localvox-process.exe').unlink()
        with self.assertRaisesRegex(RuntimeError, 'Missing executable'):
            package.stage(self.source, self.native, self.out, WINDOWS)

    def test_missing_native_dependency_is_not_a_valid_package(self):
        self.windows_input()
        (self.native / 'libvosk.dll').unlink()
        with self.assertRaisesRegex(RuntimeError, 'Missing native dependency'):
            package.stage(self.source, self.native, self.out, WINDOWS)

    def test_macos_loader_references_are_relative_and_mutated_files_are_signed(self):
        for name in (*package.BINARIES, 'libvosk.dylib'):
            (self.out / name).write_bytes(b'fixture')
        calls = []

        def command(*args):
            calls.append(args)
            return f'{args[-1]}:\n\t/dev/build/libvosk.dylib (compatibility version 0.0.0, current version 0.0.0)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)' if args[0] == 'otool' else ''

        with patch.object(package, 'run', side_effect=command):
            package.relocate_macos(self.out)
        changes = [c for c in calls if c[:2] == ('install_name_tool', '-change')]
        self.assertEqual(len(changes), 3)
        self.assertTrue(all(c[3] == '@loader_path/libvosk.dylib' for c in changes))
        self.assertEqual(len([c for c in calls if c[0] == 'codesign']), 4)

    def test_missing_extra_native_sdk_is_rejected_before_staging(self):
        with self.assertRaisesRegex(RuntimeError, 'Missing native SDK directory'):
            package.stage(self.source, self.native, self.out, WINDOWS, extra_native=(self.root / 'missing SDK',))
        self.assertEqual(list(self.out.iterdir()), [])

    def test_extra_native_sdk_is_relocated_signed_and_licensed_in_bundle_manifest(self):
        runtime = self.root / 'runtime SDK'
        runtime.mkdir()
        library = 'libonnxruntime.1.24.2.dylib'
        (runtime / library).write_bytes(b'native SDK fixture')
        notices = runtime / 'licenses/onnxruntime'
        notices.mkdir(parents=True)
        (notices / 'LICENSE').write_text('upstream runtime license')
        for name in package.BINARIES: (self.source / name).write_bytes(b'executable fixture')
        (self.native / 'libvosk.dylib').write_bytes(b'Vosk fixture')
        calls = []

        def command(*args):
            calls.append(args)
            if args[0] != 'otool': return ''
            file = Path(args[-1]).name
            dependencies = [file] if file.endswith('.dylib') else ['libvosk.dylib', library]
            return str(args[-1]) + ':\n' + ''.join(f'\t/private/build/{name} (compatibility version 0.0.0)\n' for name in dependencies)

        with patch.object(package, 'run', side_effect=command):
            package.stage(self.source, self.native, self.out, 'x86_64-apple-darwin', extra_native=(runtime,))
        self.assertEqual((self.out / library).read_bytes(), b'native SDK fixture')
        self.assertEqual((self.out / 'licenses/onnxruntime/LICENSE').read_text(), 'upstream runtime license')
        self.assertTrue(any(c[0] == 'install_name_tool' and '@loader_path/' + library in c for c in calls))
        self.assertTrue(any(c[0] == 'codesign' and Path(c[-1]).name == library for c in calls))
        manifest = json.loads((self.out / 'bundle.json').read_text())
        files = {entry['name']: entry['sha256'] for entry in manifest['files']}
        self.assertEqual(files[library], package.digest(self.out / library))
        self.assertIn('licenses/onnxruntime/LICENSE', files)

    def test_macos_unbundled_non_system_dependency_fails(self):
        (self.out / 'localvox-light').write_bytes(b'fixture')
        with patch.object(package, 'run', return_value='app:\n\t/opt/brew/missing.dylib (compatibility version 0.0.0, current version 0.0.0)'):
            with self.assertRaisesRegex(RuntimeError, 'Unbundled macOS'):
                package.relocate_macos(self.out)

    @unittest.skipUnless(os.name == 'nt' and POWERSHELL, 'Windows installer execution')
    def test_windows_rejects_wrong_platform_and_corruption_before_touching_installation(self):
        self.windows_input()
        package.stage(self.source, self.native, self.out, WINDOWS)
        install = self.root / 'existing install'
        install.mkdir()
        (install / '.env').write_bytes(b'# preserved settings\n')
        for failure in ('platform', 'checksum', 'traversal'):
            manifest = json.loads((self.out / 'bundle.json').read_text())
            if failure == 'platform':
                manifest['target'] = 'aarch64-apple-darwin'
            elif failure == 'checksum':
                manifest['files'][0]['sha256'] = '0' * 64
            archive = self.root / f'{failure}.zip'
            with zipfile.ZipFile(archive, 'w') as out:
                for file in self.out.rglob('*'):
                    if file.is_file() and file.name != 'bundle.json':
                        out.write(file, file.relative_to(self.out).as_posix())
                out.writestr('bundle.json', json.dumps(manifest))
                if failure == 'traversal':
                    out.writestr('../escaped.txt', 'refuse this')
            result = subprocess.run([POWERSHELL, '-NoProfile', '-File', str(ROOT / 'scripts/install-release.ps1'),
                                     '-Archive', str(archive), '-InstallDir', str(install), '-SkipModels'],
                                    capture_output=True, timeout=45)
            self.assertNotEqual(result.returncode, 0, failure)
            self.assertEqual(list(p.name for p in install.iterdir()), ['.env'])
            self.assertEqual((install / '.env').read_bytes(), b'# preserved settings\n')
            self.assertFalse((self.root / 'escaped.txt').exists())

    @unittest.skipUnless(os.name == 'nt' and POWERSHELL, 'Windows release HTTP boundary')
    def test_windows_old_release_and_network_failure_never_report_installation_success(self):
        # Replace external HTTP cmdlets in the isolated invoking process only.
        wrapper = self.root / 'release-fixture.ps1'
        wrapper.write_text('''param($Installer,$Destination,$Mode)
function Invoke-RestMethod {
 param($Uri,$Headers)
 if ($Mode -eq 'network') { throw 'synthetic network failure' }
 return @{tag_name='old';assets=@(@{name='localvox-light-x86_64-pc-windows-msvc.exe';browser_download_url='https://example.invalid/old'})}
}
function Invoke-WebRequest { throw 'Unexpected download of an incomplete release' }
& $Installer -InstallDir $Destination -SkipModels
''', encoding='utf-8-sig')
        install = self.root / 'must not be created'
        for mode in ('old', 'network'):
            result = subprocess.run([POWERSHELL, '-NoProfile', '-File', str(wrapper),
                                     '-Installer', str(ROOT / 'scripts/install-release.ps1'),
                                     '-Destination', str(install), '-Mode', mode],
                                    capture_output=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            expected = 'synthetic network failure' if mode == 'network' else 'has no complete'
            self.assertIn(expected, result.stderr.decode(errors='replace'))
            self.assertFalse(install.exists())

    @unittest.skipUnless(os.name == 'nt' and POWERSHELL, 'Windows readonly installer')
    def test_windows_check_before_install_does_not_create_anything(self):
        destination = self.root / 'absent installation'
        before = sorted(str(p) for p in self.root.rglob('*'))
        result = subprocess.run([POWERSHELL, '-NoProfile', '-File', str(ROOT / 'scripts/install-release.ps1'),
                                 '-InstallDir', str(destination), '-Check'], capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 2, result.stderr.decode(errors='replace'))
        self.assertFalse(destination.exists())
        self.assertEqual(before, sorted(str(p) for p in self.root.rglob('*')))

    @unittest.skipUnless(os.name == 'nt' and os.getenv('LOCALVOX_DELIVERY_ARCHIVE'), 'Explicit complete native Windows archive')
    def test_actual_windows_archive_installs_starts_and_preserves_settings(self):
        install = self.root / 'real LocalVox install'
        wrapper = self.root / 'direct-call.ps1'
        wrapper.write_text('''param($Installer,$Archive,$Destination)
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$true
& $Installer -Archive $Archive -InstallDir $Destination -SkipModels
if ($LASTEXITCODE -ne 0) { throw "Installer leaked native exit code $LASTEXITCODE" }
Set-Content -LiteralPath (Join-Path $Destination 'direct-call-completed') -Value 'ok'
''', encoding='utf-8-sig')
        for iteration in range(2):
            arguments = ([str(ROOT / 'scripts/install-release.ps1'), '-Archive', os.environ['LOCALVOX_DELIVERY_ARCHIVE'], '-InstallDir', str(install), '-SkipModels']
                         if iteration == 0 else [str(wrapper), '-Installer', str(ROOT / 'scripts/install-release.ps1'), '-Archive', os.environ['LOCALVOX_DELIVERY_ARCHIVE'], '-Destination', str(install)])
            result = subprocess.run([POWERSHELL, '-NoProfile', '-File', *arguments],
                                    capture_output=True, timeout=90)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            self.assertTrue((install / 'localvox-process.exe').is_file())
            self.assertTrue((install / 'libvosk.dll').is_file())
            if iteration == 0:
                (install / '.env').write_bytes(b'# owner override\nLOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE=off\n')
        self.assertEqual((install / '.env').read_bytes(), b'# owner override\nLOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE=off\n')
        self.assertTrue((install / 'direct-call-completed').is_file())
        before = {p.relative_to(install): (p.read_bytes(), p.stat().st_mtime_ns) for p in install.rglob('*') if p.is_file()}
        result = subprocess.run([POWERSHELL, '-NoProfile', '-File', str(ROOT / 'scripts/install-release.ps1'),
                                 '-InstallDir', str(install), '-Archive', str(self.root / 'must-not-open.zip'), '-Reconcile', '-SkipModels'],
                                capture_output=True, timeout=90)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertIn('download and unpack skipped', result.stdout.decode(errors='replace'))
        self.assertEqual(before, {p.relative_to(install): (p.read_bytes(), p.stat().st_mtime_ns) for p in install.rglob('*') if p.is_file()})
        result = subprocess.run([POWERSHELL, '-NoProfile', '-File', str(ROOT / 'scripts/install-release.ps1'),
                                 '-InstallDir', str(install), '-Check'], capture_output=True, timeout=90)
        self.assertEqual(result.returncode, 2)  # Models deliberately skipped.
        self.assertEqual(before, {p.relative_to(install): (p.read_bytes(), p.stat().st_mtime_ns) for p in install.rglob('*') if p.is_file()})



@unittest.skipUnless(BASH and Path(BASH).is_file() and shutil.which('jq'), 'bash and jq for portable installer fixtures')
class UnixInstallerContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-unix-delivery-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'commands'
        self.bin.mkdir()
        self.install = self.root / 'LocalVox with spaces'
        self.environment = {**os.environ, 'PATH': str(self.bin) + os.pathsep + os.environ['PATH']}
        self.command('uname', 'if [ "$1" = -s ]; then echo Darwin; else echo arm64; fi')
        self.command('curl', 'echo "unexpected network" >&2; exit 99')
        if os.name == 'nt':
            # Git Bash runs the Windows jq executable; model POSIX jq's LF output.
            jq = Path(shutil.which('jq')).as_posix()
            jq = '/' + jq[0].lower() + jq[2:]
            self.command('jq', 'set -o pipefail\n"' + jq + '" "$@" | tr -d "\\r"')

    def command(self, name, text):
        file = self.bin / name
        file.write_text('#!/usr/bin/env bash\n' + text + '\n', encoding='utf-8', newline='\n')
        file.chmod(0o755)

    def archive(self, target='aarch64-apple-darwin', corrupt=False):
        folder = self.root / ('bundle-' + target)
        folder.mkdir(exist_ok=True)
        for name, content in {
            'localvox-light': '#!/usr/bin/env bash\nexit 0\n',
            'localvox-process': '#!/usr/bin/env bash\necho localvox-worker/3\n',
            'localvox-tui': '#!/usr/bin/env bash\necho localvox-tui\n',
            'libvosk.dylib': 'native fixture', '.env.defaults': 'LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE=on\n',
            'scripts/models.json': '{}',
            'scripts/setup-ollama.sh': '#!/usr/bin/env bash\nexit 99\n',
            'scripts/fetch-models.sh': '#!/usr/bin/env bash\nprintf "%s\\n" "$@" > "$2/model-args"\n',
            'licenses/rs-morphy4/NOTICE': 'fixture notice',
        }.items():
            file = folder / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(content, encoding='utf-8', newline='\n')
            file.chmod(0o755)
        entries = [{'name': p.relative_to(folder).as_posix(), 'sha256': package.digest(p)} for p in folder.rglob('*') if p.is_file() and p.name != 'bundle.json']
        if corrupt:
            entries[0]['sha256'] = '0' * 64
        (folder / 'bundle.json').write_text(json.dumps({'schema': 1, 'target': target, 'files': entries}), encoding='utf-8')
        archive = self.root / (target + '.tar.gz')
        with tarfile.open(archive, 'w:gz') as out:
            for file in folder.rglob('*'):
                if file.is_file():
                    out.add(file, arcname=file.relative_to(folder).as_posix())
        return archive

    def install_archive(self, archive, *extra):
        prefix = 'export PATH="$(cygpath -u "$1"):$PATH"; shift; exec bash "$@"' if os.name == 'nt' else 'export PATH="$1:$PATH"; shift; exec bash "$@"'
        def unix_path(file):
            value = file.as_posix()
            return '/' + value[0].lower() + value[2:] if os.name == 'nt' else value
        return subprocess.run([BASH, '-c', prefix, 'fixture', self.bin.as_posix(), unix_path(ROOT / 'scripts/install-release.sh'),
                               '--archive=' + unix_path(archive), '--install-dir=' + unix_path(self.install), *extra],
                              env=self.environment, capture_output=True, timeout=45)

    def test_mac_check_before_install_is_readonly_and_needs_no_network(self):
        result = self.install_archive(self.root / 'absent.tar.gz', '--check')
        self.assertEqual(result.returncode, 2, result.stderr.decode(errors='replace'))
        self.assertFalse(self.install.exists())
        self.assertNotIn('unexpected network', result.stderr.decode(errors='replace'))

    def test_mac_reconcile_reuses_bundle_even_if_archive_is_not_available(self):
        result = self.install_archive(self.archive(), '--skip-models')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        before = {p.relative_to(self.install): (p.read_bytes(), p.stat().st_mtime_ns)
                  for p in self.install.rglob('*') if p.is_file()}
        result = self.install_archive(self.root / 'must-not-open.tar.gz', '--skip-models', '--reconcile')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertIn('download and unpack skipped', result.stdout.decode(errors='replace'))
        after = {p.relative_to(self.install): (p.read_bytes(), p.stat().st_mtime_ns)
                 for p in self.install.rglob('*') if p.is_file()}
        self.assertEqual(before, after)
        result = self.install_archive(self.root / 'must-not-open.tar.gz', '--check')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertEqual(after, {p.relative_to(self.install): (p.read_bytes(), p.stat().st_mtime_ns)
                                for p in self.install.rglob('*') if p.is_file()})
        (self.install / 'localvox-process').write_text('corrupted')
        result = self.install_archive(self.root / 'absent.tar.gz', '--check')
        self.assertEqual(result.returncode, 2)

    def test_mac_wizard_does_not_fetch_models_or_pin_env_before_user_choices(self):
        result = self.install_archive(self.archive(), '--wizard')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertFalse((self.install / 'model-args').exists())
        self.assertFalse((self.install / '.env').exists())

    def test_mac_arm64_local_archive_and_required_only_preserve_existing_env(self):
        self.install.mkdir()
        (self.install / '.env').write_bytes(b'# owner setting\n')
        result = self.install_archive(self.archive(), '--required-only')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertEqual((self.install / '.env').read_bytes(), b'# owner setting\n')
        self.assertEqual((self.install / 'model-args').read_text().splitlines()[-1], '--required-only')

    def test_mac_intel_archive_creates_quoted_absolute_paths_without_model_download(self):
        self.command('uname', 'if [ "$1" = -s ]; then echo Darwin; else echo x86_64; fi')
        result = self.install_archive(self.archive('x86_64-apple-darwin'), '--skip-models')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertIn('LocalVox with spaces/archive"', (self.install / '.env').read_text())
        self.assertFalse((self.install / 'model-args').exists())

    def test_mac_wrong_platform_or_corrupt_archive_is_rejected_before_copy(self):
        for target, corrupt in [('x86_64-apple-darwin', False), ('aarch64-apple-darwin', True)]:
            result = self.install_archive(self.archive(target, corrupt), '--skip-models')
            self.assertNotEqual(result.returncode, 0)
            self.assertRegex(result.stderr.decode(errors='replace'), 'platform/schema mismatch|SHA-256 mismatch')
            self.assertFalse(self.install.exists())

    def test_mac_archive_links_are_rejected_before_extraction(self):
        archive = self.archive()
        with tarfile.open(archive, 'r:gz') as old:
            entries = [(info, old.extractfile(info).read()) for info in old if info.isfile()]
        with tarfile.open(archive, 'w:gz') as out:
            for info, data in entries:
                import io
                out.addfile(info, io.BytesIO(data))
            link = tarfile.TarInfo('escape')
            link.type = tarfile.SYMTYPE
            link.linkname = '../outside'
            out.addfile(link)
        result = self.install_archive(archive, '--skip-models')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Archive links', result.stderr.decode(errors='replace'))
        self.assertFalse(self.install.exists())


if __name__ == '__main__':
    unittest.main()
