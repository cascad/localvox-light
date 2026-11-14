"""Real PS/Bash downloads with tiny localhost payloads and an isolated user profile."""
import functools
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile
import threading
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PS = shutil.which('pwsh') or shutil.which('powershell')
BASH = os.getenv('LOCALVOX_TEST_BASH') or (r'C:\Program Files\Git\bin\bash.exe' if os.name == 'nt' else shutil.which('bash'))


class ModelStorageDelivery(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-model-paths-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.install = self.root / 'Установка с пробелами'
        self.scripts = self.install / 'scripts'
        self.scripts.mkdir(parents=True)
        self.home = self.root / 'Профиль с пробелами'
        self.home.mkdir()
        self.cwd = self.root / 'workspace'
        self.cwd.mkdir()
        self.environment = {k: v for k, v in os.environ.items() if not k.startswith('LOCALVOX_')}
        self.environment.update(HOME=str(self.home), USERPROFILE=str(self.home))
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.environment['PATH'] = str(self.bin) + os.pathsep + os.environ['PATH']
        if os.name == 'nt' and shutil.which('jq'):
            # Windows jq emits CRLF; emulate the native Unix jq used on Mac/Linux.
            jq = self.unix(Path(shutil.which('jq')))
            (self.bin / 'jq').write_text(f'#!/usr/bin/env bash\nset -o pipefail\n"{jq}" "$@" | tr -d "\\r"\n', newline='\n')
        if os.name == 'nt':
            # Exercise the native Windows curl present on Actions, including Unicode paths.
            curl = Path(os.environ['SystemRoot']) / 'System32/curl.exe'
            self.assertTrue(curl.is_file())
            (self.bin / 'curl').write_text(f'#!/usr/bin/env bash\nexec "{self.unix(curl)}" "$@"\n', newline='\n')
        for name in ('fetch-models.ps1', 'fetch-models.sh', 'model-paths.ps1', 'model-paths.sh'):
            shutil.copyfile(ROOT / 'scripts' / name, self.scripts / name)
        payloads = self.root / 'payloads'
        payloads.mkdir()
        requests = self.requests = []

        class Handler(SimpleHTTPRequestHandler):
            def do_GET(self):
                requests.append(self.path)
                super().do_GET()

            def log_message(self, *_):
                pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Handler, directory=str(payloads)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop_server)
        self.manifest = json.loads((ROOT / 'scripts/models.json').read_text(encoding='utf-8'))
        for component in self.manifest['components']:
            for i, item in enumerate(component['items']):
                if item['kind'] == 'text':
                    continue
                name = f'{component["id"]}-{i}'
                file = payloads / name
                if item['kind'] == 'zip':
                    with zipfile.ZipFile(file, 'w') as archive:
                        archive.writestr('fixture/' + item['marker'], b'tiny zip model')
                else:
                    file.write_bytes(('tiny ' + name).encode())
                item['url'] = f'http://127.0.0.1:{self.server.server_port}/{name}'
                item['sha256'] = hashlib.sha256(file.read_bytes()).hexdigest()
        self.save_manifest()

    def stop_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def save_manifest(self):
        (self.scripts / 'models.json').write_text(json.dumps(self.manifest, ensure_ascii=False), encoding='utf-8')

    @staticmethod
    def unix(path):
        value = Path(path).as_posix()
        return '/' + value[0].lower() + value[2:] if os.name == 'nt' else value

    def run_fetch(self, shell, *options, expected=0):
        if shell == 'ps':
            command = [PS, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', self.scripts / 'fetch-models.ps1', *options]
            env = self.environment
        else:
            env = {**self.environment, 'HOME': self.unix(self.home)}
            command = [BASH, '-c', 'export PATH="$1:$PATH"; shift; exec bash "$@"', 'fixture', self.unix(self.bin), self.unix(self.scripts / 'fetch-models.sh'), *options]
        result = subprocess.run(command, env=env, cwd=self.cwd, capture_output=True, timeout=120)
        self.assertEqual(result.returncode, expected, (result.stdout + result.stderr).decode('utf-8', errors='replace'))
        return result

    def shells(self):
        available = (['ps'] if PS and os.name == 'nt' else []) + (['bash'] if BASH and Path(BASH).is_file() and shutil.which('jq') else [])
        if not available:
            self.skipTest('PowerShell on Windows or Bash+jq required')
        return available

    @staticmethod
    def snapshot(folder):
        # PowerShell itself populates its AppData module-analysis cache on first start.
        # Observe product files/model placement, independently of the shell's own cache.
        return {p.relative_to(folder).as_posix(): (hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_mtime_ns)
                for p in folder.rglob('*') if p.is_file() and 'AppData' not in p.relative_to(folder).parts}

    def test_default_check_download_repeat_and_hidden_attribute(self):
        for shell in self.shells():
            with self.subTest(shell=shell):
                store = self.home / '.localvox'
                check = '-Check' if shell == 'ps' else '--check'
                self.run_fetch(shell, check, expected=1)
                self.assertFalse(store.exists())
                self.assertEqual(self.requests, [])
                self.run_fetch(shell)
                self.assertTrue((store / 'models/vosk-model-ru-0.42/am/final.mdl').is_file())
                self.assertTrue((store / 'models/gigaam-v3-e2e-ctc/v3_e2e_ctc.int8.onnx').is_file())
                if os.name == 'nt':
                    self.assertTrue(store.stat().st_file_attributes & 2)
                before = self.snapshot(self.root)
                requests = list(self.requests)
                self.run_fetch(shell, check)
                self.run_fetch(shell)
                self.assertEqual(self.requests, requests)
                self.assertEqual(self.snapshot(self.root), before)
                shutil.rmtree(store)
                self.requests.clear()

    def test_explicit_root_and_ready_portable_are_reused_without_user_store(self):
        for shell in self.shells():
            with self.subTest(shell=shell):
                root_arg = str(self.install) if shell == 'ps' else self.unix(self.install)
                self.run_fetch(shell, '-Root' if shell == 'ps' else '--root', root_arg)
                before, requests = self.snapshot(self.root), list(self.requests)
                self.run_fetch(shell)
                self.assertEqual(self.requests, requests)
                self.assertEqual(self.snapshot(self.root), before)
                self.assertFalse((self.home / '.localvox').exists())
                shutil.rmtree(self.install / 'models')

    def test_single_component_directory_is_exact_and_repeat_downloads_nothing(self):
        for shell in self.shells():
            with self.subTest(shell=shell):
                chosen = self.root / 'Точные веса без models'
                argument = str(chosen) if shell == 'ps' else self.unix(chosen)
                options = (['-Only', 'gigaam', '-ComponentDir', argument] if shell == 'ps' else
                           ['--only', 'gigaam', '--component-dir', argument])
                self.run_fetch(shell, *options)
                self.assertTrue((chosen / 'v3_e2e_ctc.int8.onnx').is_file())
                self.assertFalse((chosen / 'models').exists())
                before, requests = self.snapshot(self.root), list(self.requests)
                self.run_fetch(shell, *options)
                self.assertEqual(self.requests, requests)
                self.assertEqual(self.snapshot(self.root), before)
                self.assertFalse((self.home / '.localvox').exists())
                shutil.rmtree(chosen)

    def test_invalid_explicit_path_never_downloads_or_replaces_user_files(self):
        external = self.root / 'External model'
        external.mkdir()
        (external / 'keep.txt').write_bytes(b'owner content')
        for shell in self.shells():
            with self.subTest(shell=shell):
                self.environment['LOCALVOX_LIGHT_MODEL'] = str(external) if shell == 'ps' else self.unix(external)
                before = self.snapshot(self.root)
                self.run_fetch(shell, '-Only' if shell == 'ps' else '--only', 'vosk', expected=1)
                self.assertEqual(self.snapshot(self.root), before)
                self.assertEqual(self.requests, [])

    def test_config_file_without_native_resolver_requires_explicit_root(self):
        (self.install / '.env').write_bytes(b'# preserve arbitrary dotenv syntax\n')
        for shell in self.shells():
            with self.subTest(shell=shell):
                before = self.snapshot(self.root)
                result = self.run_fetch(shell, '-Check' if shell == 'ps' else '--check', expected=1)
                self.assertIn(b'resolver', result.stderr)
                self.assertEqual(self.snapshot(self.root), before)
                self.assertEqual(self.requests, [])

    def test_failed_hash_never_publishes_ready_model(self):
        self.manifest['components'][0]['items'][0]['sha256'] = '0' * 64
        self.save_manifest()
        for shell in self.shells():
            with self.subTest(shell=shell):
                root = self.root / ('hash-' + shell)
                self.run_fetch(shell, '-Root' if shell == 'ps' else '--root', str(root) if shell == 'ps' else self.unix(root),
                               '-Only' if shell == 'ps' else '--only', 'vosk', expected=1)
                self.assertFalse((root / 'models/vosk-model-ru-0.42').exists())

    @unittest.skipUnless(os.name == 'nt', 'native Windows directory attributes')
    def test_windows_equivalent_user_root_paths_and_existing_attributes(self):
        probe = self.root / 'hidden-probe.ps1'
        probe.write_text(r'''param($Helper,$Root)
$ErrorActionPreference='Stop'
. $Helper
$userRoot=Join-Path $Root 'profile/../profile/.localvox'
$target=[IO.Path]::GetFullPath((Join-Path $userRoot 'models/vosk/am/final.mdl'))
$actual=[IO.Path]::GetFullPath($userRoot)
New-ModelParent $target $userRoot
if (-not ((Get-Item -Force -LiteralPath $actual).Attributes -band [IO.FileAttributes]::Hidden)) { throw 'New normalized user root is not Hidden' }
$directory=Get-Item -Force -LiteralPath $actual
$directory.Attributes=$directory.Attributes -band (-bnot [IO.FileAttributes]::Hidden)
$before=(Get-Item -Force -LiteralPath $actual).Attributes
New-ModelParent (Join-Path $actual 'models/gigaam/model.onnx') $userRoot
if ((Get-Item -Force -LiteralPath $actual).Attributes -ne $before) { throw 'Existing root attributes changed' }
$custom=Join-Path $Root 'custom/model.onnx'
New-ModelParent $custom $userRoot
if ((Get-Item -Force -LiteralPath (Split-Path -Parent $custom)).Attributes -band [IO.FileAttributes]::Hidden) { throw 'Custom root was hidden' }
''', encoding='ascii')
        engines = list(dict.fromkeys(p for p in [shutil.which('pwsh'), shutil.which('powershell')] if p))
        for i, engine in enumerate(engines):
            with self.subTest(engine=engine):
                result = subprocess.run([engine, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', probe,
                                         self.scripts / 'model-paths.ps1', self.root / f'engine-{i}'],
                                        env=self.environment, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))

    @unittest.skipIf(os.name == 'nt', 'Unix symlink semantics; no Windows symlink privilege assumed')
    def test_symlinked_ancestors_work_but_model_symlink_is_never_replaced(self):
        alias = self.root / 'profile-alias'
        alias.symlink_to(self.home, target_is_directory=True)
        self.home = alias
        before = self.snapshot(self.root)
        self.run_fetch('bash', '--check', expected=1)
        self.assertEqual(self.snapshot(self.root), before)
        self.assertEqual(self.requests, [])
        self.run_fetch('bash')
        requests = list(self.requests)
        before = self.snapshot(self.root)
        self.run_fetch('bash')
        self.assertEqual(self.snapshot(self.root), before)
        self.assertEqual(self.requests, requests)
        model = alias / '.localvox/models/vosk-model-ru-0.42'
        shutil.rmtree(model)
        foreign = self.root / 'owner-files'
        foreign.mkdir()
        (foreign / 'keep.txt').write_bytes(b'owner content')
        model.symlink_to(foreign, target_is_directory=True)
        before = self.snapshot(self.root)
        self.run_fetch('bash', '--only', 'vosk', '--repair-zip', expected=1)
        self.assertEqual(self.requests, requests)
        self.assertEqual(self.snapshot(self.root), before)
        self.assertTrue(model.is_symlink())

    def test_bash_guard_accepts_physical_ancestor_alias_and_refuses_other_destinations(self):
        if 'bash' not in self.shells():
            self.skipTest('Bash required')
        real = self.root / 'physical-root'
        real.mkdir()
        link = self.root / 'root-alias'
        self.directory_alias(link, real)
        expected = real / 'models/vosk'
        logical = link / 'models/vosk'
        script = 'source "$1"; actual="$(assert_model_replacement "$2" "$3")" || exit; if command -v cygpath >/dev/null; then cygpath -w "$actual"; else printf "%s\\n" "$actual"; fi'
        def guard(target, selected):
            return subprocess.run([BASH, '-c', script, 'guard', self.unix(self.scripts / 'model-paths.sh'),
                                   self.unix(target), self.unix(selected)], env=self.environment,
                                  capture_output=True, timeout=30)
        before = self.snapshot(self.root)
        result = guard(logical, logical)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertEqual(Path(result.stdout.decode('utf-8').strip()).resolve(), expected.resolve())
        result = guard(logical, expected)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertFalse(expected.exists())
        self.assertEqual(self.snapshot(self.root), before)
        self.assertNotEqual(guard(logical, self.root / 'different-model').returncode, 0)
        expected.parent.mkdir()
        foreign = self.root / 'foreign-owner-data'
        foreign.mkdir()
        (foreign / 'keep.txt').write_bytes(b'owner data')
        self.directory_alias(expected, foreign)
        before = self.snapshot(self.root)
        self.assertNotEqual(guard(logical, expected).returncode, 0)
        self.assertEqual(self.snapshot(self.root), before)
        self.assertEqual((foreign / 'keep.txt').read_bytes(), b'owner data')

    def directory_alias(self, link, target):
        if os.name != 'nt':
            link.symlink_to(target, target_is_directory=True)
            return
        # NTFS junctions need no symlink privilege; both paths belong to this fixture.
        helper = self.root / 'junction.ps1'
        helper.write_text('param($Link,$Target)\nNew-Item -ItemType Junction -Path $Link -Target $Target -ErrorAction Stop | Out-Null\n')
        result = subprocess.run([PS, '-NoProfile', '-File', helper, link, target], env=self.environment,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))

    @unittest.skipUnless(os.name == 'nt', 'Windows Hidden through a user profile alias')
    def test_bash_parent_normalizes_user_root_before_hidden_check(self):
        if 'bash' not in self.shells():
            self.skipTest('Git Bash required')
        store = self.home / '.localvox'
        equivalent = self.home / '..' / self.home.name / '.localvox'
        command = 'source "$1"; USER_MODEL_ROOT="$2"; new_model_parent "$3"'
        result = subprocess.run([BASH, '-c', command, 'fixture', self.unix(self.scripts / 'model-paths.sh'),
                                 self.unix(equivalent), self.unix(store / 'models/vosk')],
                                env=self.environment, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertTrue(store.stat().st_file_attributes & 2)

    @unittest.skipUnless(os.name == 'nt', 'Windows Hidden through a user profile alias')
    def test_bash_hides_user_store_with_native_short_home_path(self):
        if 'bash' not in self.shells():
            self.skipTest('Git Bash required')
        import ctypes
        short_path = ctypes.windll.kernel32.GetShortPathNameW
        short_path.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32]
        short_path.restype = ctypes.c_uint32
        buffer = ctypes.create_unicode_buffer(32768)
        if not short_path(str(self.home), buffer, len(buffer)):
            raise ctypes.WinError()
        if buffer.value.casefold() == str(self.home).casefold():
            self.skipTest('8.3 aliases are disabled on the fixture volume')
        actual = self.home
        self.home = Path(buffer.value)
        self.run_fetch('bash', '--only', 'vosk')
        self.assertTrue((actual / '.localvox').stat().st_file_attributes & 2)

    @unittest.skipUnless(os.name == 'nt', 'Windows Hidden through a user profile alias')
    def test_bash_hides_new_user_root_through_home_alias_and_preserves_existing_attributes(self):
        if 'bash' not in self.shells():
            self.skipTest('Git Bash required')
        real_home = self.home
        self.home = self.root / 'profile-alias'
        self.directory_alias(self.home, real_home)
        store = real_home / '.localvox'
        # ZIP replacement resolves a physical target; individual files retain the HOME alias.
        for component in ('vosk', 'gigaam'):
            with self.subTest(component=component):
                try:
                    self.requests.clear()
                    before = self.snapshot(self.root)
                    self.run_fetch('bash', '--only', component, '--check', expected=1)
                    self.assertFalse(store.exists())
                    self.assertEqual(self.snapshot(self.root), before)
                    self.assertEqual(self.requests, [])
                    self.run_fetch('bash', '--only', component)
                    self.assertTrue(store.stat().st_file_attributes & 2)
                    before, requests = self.snapshot(self.root), list(self.requests)
                    self.run_fetch('bash', '--only', component)
                    self.assertEqual(self.snapshot(self.root), before)
                    self.assertEqual(self.requests, requests)
                finally:
                    if store.exists():
                        shutil.rmtree(store)
        store.mkdir()
        attributes = store.stat().st_file_attributes
        self.run_fetch('bash', '--only', 'vosk')
        self.assertEqual(store.stat().st_file_attributes, attributes)

    @unittest.skipUnless(os.getenv('LOCALVOX_MODEL_RESOLVER_TEST_BINARY') or os.getenv('LOCALVOX_DELIVERY_ARCHIVE'), 'built native bundle/resolver for installed .env parity')
    def test_native_resolver_reads_dotenv_and_both_downloaders_reuse_external_models(self):
        if os.getenv('LOCALVOX_DELIVERY_ARCHIVE'):
            archive = Path(os.environ['LOCALVOX_DELIVERY_ARCHIVE']).resolve(strict=True)
            if archive.suffix == '.zip':
                with zipfile.ZipFile(archive) as bundle:
                    bundle.extractall(self.install)
            else:
                with tarfile.open(archive) as bundle:
                    bundle.extractall(self.install, filter='data')
        else:
            original = Path(os.environ['LOCALVOX_MODEL_RESOLVER_TEST_BINARY']).resolve(strict=True)
            shutil.copyfile(original, self.install / original.name)
            (self.install / original.name).chmod(0o755)
            for library in original.parent.iterdir():
                if library.suffix in ('.dll', '.dylib', '.so'):
                    shutil.copyfile(library, self.install / library.name)
        settings = []
        for component in self.manifest['components']:
            directory = self.root / 'Внешние веса' / component['id']
            for item in component['items']:
                if item['kind'] == 'text':
                    continue  # Ready external weights do not grant permission to add licence sidecars.
                relative = item['marker'] if item['kind'] == 'zip' else str(Path(item['dest']).relative_to(component['directory']))
                destination = directory / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(b'ready external fixture')
            settings.append(component['env'] + '=' + json.dumps(str(directory), ensure_ascii=False))
        (self.install / '.env').write_text('\n'.join(settings) + '\n', encoding='utf-8')
        before = self.snapshot(self.root)
        for shell in self.shells():
            with self.subTest(shell=shell):
                self.run_fetch(shell, '-Check' if shell == 'ps' else '--check')
                self.run_fetch(shell)
                self.assertEqual(self.requests, [])
                self.assertEqual(self.snapshot(self.root), before)
                self.assertFalse((self.home / '.localvox').exists())


if __name__ == '__main__':
    unittest.main()
