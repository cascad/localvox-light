"""Bootstrap stand: localhost releases, tiny payloads, no models or OS installers.

Run on each native OS: python -m unittest discover -s scripts/tests -p test_bootstrap_delivery.py -v
The real wizard is covered separately by the native ConPTY/PTY integration test.
"""
import functools
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import io
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import threading
import time
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PS = shutil.which('powershell') if os.name == 'nt' else None
BASH = os.getenv('LOCALVOX_TEST_BASH') or (r'C:\Program Files\Git\bin\bash.exe' if os.name == 'nt' else shutil.which('bash'))


class BootstrapDelivery(unittest.TestCase):
    @unittest.skipUnless(PS, 'Windows asynchronous download wait')
    def test_windows_ready_reads_do_not_pay_a_progress_timer_per_block(self):
        wrapper = self.root / 'ready-reads.ps1'
        wrapper.write_text('''param($Bootstrap)
$ErrorActionPreference='Stop'
function Show-DownloadProgress { $script:updates++ }
$script:updates=0
$tokens=$null; $errors=$null
$ast=[System.Management.Automation.Language.Parser]::ParseInput([IO.File]::ReadAllText($Bootstrap),[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Bootstrap syntax error' }
$helper=$ast.Find({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-DownloadTask'},$true)
if (-not $helper) { throw 'Download task wait not found' }
Invoke-Expression $helper.Extent.Text
$null=Wait-DownloadTask ([Threading.Tasks.Task]::Delay(5))
$watch=[Diagnostics.Stopwatch]::StartNew()
for ($i=0; $i -lt 100; $i++) { $null=Wait-DownloadTask ([Threading.Tasks.Task]::Delay(5)) }
@{elapsed_ms=$watch.ElapsedMilliseconds;updates=$script:updates} | ConvertTo-Json -Compress
''', encoding='utf-8-sig')
        for shell in dict.fromkeys(filter(None, (PS, shutil.which('pwsh')))):
            with self.subTest(shell=shell):
                result = subprocess.run([shell, '-NoProfile', '-File', str(wrapper),
                                         '-Bootstrap', str(ROOT / 'scripts/install.ps1')],
                                        env=self.environment, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
                sample = json.loads(result.stdout)
                self.assertGreater(sample['updates'], 0, 'waiting must still refresh progress')
                # The task chain needs ~0.5–1.6 s, with generous scheduling room.
                # A fixed 100 ms sleep per ready read introduces ~10 s instead.
                self.assertLess(sample['elapsed_ms'], 6000, sample)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-bootstrap-fixture-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / 'isolated home'
        self.install = self.root / 'installed with spaces'
        self.scratch = self.root / 'temp'
        self.bin = self.root / 'bin'
        self.web = self.root / 'web'
        for directory in (self.home, self.scratch, self.bin, self.web):
            directory.mkdir()
        self.log = self.root / 'launch.log'
        self.environment = {k: v for k, v in os.environ.items() if not k.startswith('LOCALVOX_')}
        self.environment.update(HOME=str(self.home), USERPROFILE=str(self.home), LOCALAPPDATA=str(self.home),
                                TEMP=str(self.scratch), TMP=str(self.scratch),
                                LOCALVOX_BOOTSTRAP_FIXTURE_LOG=str(self.log))
        self.requests = []
        requests = self.requests
        self.release = release = {'tag': 'v9.8.7'}
        self.transfer = transfer = {}

        class Handler(SimpleHTTPRequestHandler):
            def do_HEAD(self):
                requests.append(self.path)
                if self.path == '/releases/latest':
                    if release.get('unavailable'):
                        self.send_error(503)
                        return
                    self.send_response(302)
                    self.send_header('Location', '/releases/tag/' + release['tag'])
                    self.end_headers()
                elif self.path.startswith('/releases/tag/'):
                    self.send_response(200)
                    self.end_headers()
                else:
                    super().do_HEAD()

            def do_GET(self):
                requests.append(self.path)
                if transfer and self.path.endswith(('.zip', '.tar.gz')):
                    if transfer.get('redirect') and not self.path.startswith('/payload/'):
                        body = b'<html>redirecting</html>'
                        self.send_response(302)
                        self.send_header('Location', '/payload' + self.path)
                        self.send_header('Content-Length', str(len(body)))
                        self.end_headers()
                        self.wfile.write(body)
                        return
                    if transfer.get('deny'):
                        self.send_error(503)
                        return
                    path = self.path.removeprefix('/payload')
                    payload = Path(self.translate_path(path)).read_bytes()
                    self.send_response(200)
                    if not transfer.get('unknown'):
                        self.send_header('Content-Length', str(len(payload)))
                    self.send_header('Connection', 'close')
                    self.end_headers()
                    limit = len(payload) // 2 if transfer.get('cut') else len(payload)
                    chunk = max(1, (len(payload) + 7) // 8)
                    try:
                        for offset in range(0, limit, chunk):
                            self.wfile.write(payload[offset:min(offset + chunk, limit)])
                            self.wfile.flush()
                            time.sleep(transfer.get('delay', 0))
                    except ConnectionError:
                        pass
                    self.close_connection = True
                    return
                super().do_GET()

            def log_message(self, *_):
                pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Handler, directory=str(self.web)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop)
        self.url = f'http://127.0.0.1:{self.server.server_port}'

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    @staticmethod
    def unix(path):
        value = Path(path).as_posix()
        return '/' + value[0].lower() + value[2:] if os.name == 'nt' else value

    def command(self, name, text):
        path = self.bin / name
        path.write_text('#!/usr/bin/env bash\n' + text + '\n', encoding='utf-8', newline='\n')
        path.chmod(0o755)

    def archive(self, target, *, corrupt=False, unsafe=False, padding=0, tag='v9.8.7'):
        folder = self.web / 'releases/download' / tag
        folder.mkdir(parents=True, exist_ok=True)
        windows = 'windows' in target
        name = f'localvox-{target}' + ('.zip' if windows else '.tar.gz')
        archive = folder / name
        files = {'bundle.json': b'{}', 'fixture-tag': tag.encode('ascii')}
        if padding:
            files['payload.bin'] = os.urandom(padding)
        if windows:
            # .NET Framework stub tests delivery only; the Rust test drives the real wizard.
            source = self.root / 'Fixture.cs'
            source.write_text('''using System; using System.IO;
class Fixture {
 static int Main(string[] args) {
  string dest="", tag=""; bool check=false;
  foreach(string a in args) if(a=="--bundle-check") check=true;
  for(int i=0;i+1<args.Length;i++) if(args[i]=="--install-dir") dest=args[i+1];
  for(int i=0;i+1<args.Length;i++) if(args[i]=="--release-tag") tag=args[i+1];
  if(check) {
   if(!File.Exists(Path.Combine(dest,"installed.ok"))) return 2;
   if(File.Exists(Path.Combine(dest,"fixture-newer"))) return 3;
   string version=Path.Combine(dest,"fixture-tag");
   return File.Exists(version) && File.ReadAllText(version)==tag?0:2;
  }
  foreach(string a in args) if(a=="--check") return 2;
  File.AppendAllText(Environment.GetEnvironmentVariable("LOCALVOX_BOOTSTRAP_FIXTURE_LOG"), String.Join("\\n",args)+"\\n");
  Directory.CreateDirectory(dest);
  string source=AppDomain.CurrentDomain.BaseDirectory;
  if(Path.GetFullPath(source).TrimEnd('\\\\')!=Path.GetFullPath(dest).TrimEnd('\\\\'))
   foreach(string file in Directory.GetFiles(source)) File.Copy(file,Path.Combine(dest,Path.GetFileName(file)),true);
  File.WriteAllText(Path.Combine(dest,"installed.ok"),"fixture"); return 0;
 }
}''', encoding='utf-8')
            binary = self.root / 'fixture.exe'
            if binary.exists():
                binary.unlink()
            compiler = self.root / 'compile.ps1'
            compiler.write_text("param($Source,$Output)\n$ErrorActionPreference='Stop'\nAdd-Type -Path $Source -OutputAssembly $Output -OutputType ConsoleApplication\n")
            result = subprocess.run([PS, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', compiler,
                                     '-Source', source, '-Output', binary], env=self.environment, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            files['localvox-install.exe'] = binary.read_bytes()
            for dll in ('libvosk.dll', 'libgcc_s_seh-1.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll'):
                files[dll] = b'loader presence fixture, not loaded by managed stub'
            with zipfile.ZipFile(archive, 'w') as out:
                for member, data in files.items():
                    out.writestr(member, data)
                if unsafe:
                    out.writestr('../outside', b'must not escape')
        else:
            files['libvosk.dylib' if 'darwin' in target else 'libvosk.so'] = b'fixture'
            files['localvox-install'] = b'''#!/usr/bin/env bash
set -eu
DEST='' CHECK=0 TAG=''
for arg in "$@"; do [[ "$arg" != --bundle-check ]] || CHECK=1; done
ARGS=("$@")
while [[ $# -gt 0 ]]; do case "$1" in --install-dir) DEST="$2"; shift 2;; --release-tag) TAG="$2"; shift 2;; --check) exit 2;; *) shift;; esac; done
if [[ "$CHECK" = 1 ]]; then
 [[ -f "$DEST/installed.ok" && -f "$DEST/fixture-tag" ]] || exit 2
 [[ ! -f "$DEST/fixture-newer" ]] || exit 3
 [[ "$(cat "$DEST/fixture-tag")" = "$TAG" ]] || exit 2
 exit 0
fi
printf '%s\\n' "${ARGS[@]}" >> "$LOCALVOX_BOOTSTRAP_FIXTURE_LOG"
mkdir -p "$DEST"
SOURCE="$(cd "$(dirname "$0")" && pwd)"
if [[ "$SOURCE" != "$DEST" ]]; then cp -R "$SOURCE/." "$DEST/"; fi
echo fixture > "$DEST/installed.ok"
'''
            with tarfile.open(archive, 'w:gz') as out:
                for member, data in files.items():
                    info = tarfile.TarInfo(member)
                    info.size = len(data)
                    info.mode = 0o755 if member == 'localvox-install' else 0o644
                    out.addfile(info, io.BytesIO(data))
                if unsafe:
                    link = tarfile.TarInfo('escape')
                    link.type, link.linkname = tarfile.SYMTYPE, '../outside'
                    out.addfile(link)
        digest = '0' * 64 if corrupt else hashlib.sha256(archive.read_bytes()).hexdigest()
        (folder / (name + '.sha256')).write_text(f'{digest}  {name}\n')
        return name

    def run_bootstrap(self, shell, *flags, platform=('Linux', 'x86_64'), tty=False, powershell=None,
                      force_ansi=False):
        env = self.environment.copy()
        if shell == 'ps':
            command = [powershell or PS, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ROOT / 'scripts/install.ps1',
                       '-InstallDir', self.install, '-ReleaseBase', self.url, '-NoPathUpdate', *flags]
            if force_ansi:
                env = {key: value for key, value in env.items() if key.upper() != 'NO_COLOR'}
                env.update(TERM='xterm-256color', FORCE_COLOR='1')
                wrapper = self.root / 'forced-style.ps1'
                wrapper.write_text('''param($Script,$InstallDir,$ReleaseBase)
$ErrorActionPreference='Stop'
$PSStyle.OutputRendering='Ansi'
& $Script -InstallDir $InstallDir -ReleaseBase $ReleaseBase -NoPathUpdate -Update
exit $LASTEXITCODE
''', encoding='ascii')
                command = [powershell, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', wrapper,
                           '-Script', ROOT / 'scripts/install.ps1', '-InstallDir', self.install,
                           '-ReleaseBase', self.url]
        else:
            self.command('uname', f'if [[ "$1" = -s ]]; then echo {platform[0]}; else echo {platform[1]}; fi')
            self.command('jq', 'echo "bootstrap must not require jq" >&2; exit 97')
            env.update(HOME=self.unix(self.home), TMPDIR=self.unix(self.scratch),
                       LOCALVOX_BOOTSTRAP_FIXTURE_LOG=self.unix(self.log))
            command = [BASH, '-c', 'export PATH="$1:$PATH"; shift; exec bash "$@"', 'fixture',
                       self.unix(self.bin), self.unix(ROOT / 'scripts/install.sh'),
                       '--install-dir', self.unix(self.install), '--release-base', self.url, '--no-path-update', *flags]
        if tty:
            if os.getenv('LOCALVOX_TEST_PTY_RUNNER'):
                transcript = self.root / 'terminal.ansi'
                result = subprocess.run([os.environ['LOCALVOX_TEST_PTY_RUNNER'], str(transcript),
                                         *[str(a) for a in command]], env=env, capture_output=True, timeout=45)
                result = subprocess.CompletedProcess(command, result.returncode,
                                                     transcript.read_bytes(), result.stderr)
            else:
                result = self.run_unix_pty(command, env)
        else:
            result = subprocess.run([str(a) for a in command], env=env, capture_output=True, timeout=45)
        self.assertFalse(list(self.scratch.glob('localvox-bootstrap*')), 'bootstrap cleans only its own staging directory')
        return result

    @staticmethod
    def run_unix_pty(command, env):
        import pty
        # Supply a controlling terminal for the production </dev/tty handoff.
        # The stub never asks for input; genuine prompt keys are tested in Rust.
        output = bytearray()
        previous = os.environ.copy()
        try:
            os.environ.clear()
            os.environ.update(env)
            status = pty.spawn([str(a) for a in command], lambda fd: output.extend(data := os.read(fd, 8192)) or data)
        finally:
            os.environ.clear()
            os.environ.update(previous)
        return subprocess.CompletedProcess(command, os.waitstatus_to_exitcode(status), bytes(output), b'')

    @unittest.skipUnless(PS, 'Windows PowerShell fixture')
    def test_windows_download_apply_reuse_and_readonly_check(self):
        absent = self.run_bootstrap('ps', '-Check')
        self.assertEqual(absent.returncode, 2)
        self.assertFalse(self.install.exists())
        self.assertEqual(self.requests, [])
        name = self.archive('x86_64-pc-windows-msvc')
        result = self.run_bootstrap('ps')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertTrue((self.install / 'installed.ok').is_file())
        self.assertIn('/releases/download/v9.8.7/' + name, self.requests)
        before = list(self.requests)
        result = self.run_bootstrap('ps')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertEqual(self.requests[len(before):], ['/releases/latest', '/releases/tag/v9.8.7'],
                         'same release needs only metadata, not another archive')

    def test_ordinary_retry_updates_old_identity_and_preserves_newer_installation(self):
        shells = (['ps'] if PS else []) + (['bash'] if os.getenv('LOCALVOX_TEST_PTY_RUNNER') else [])
        if not shells:
            self.skipTest('Windows PowerShell or built terminal-capture runner')
        for shell in shells:
            with self.subTest(shell=shell):
                self.release.clear()
                self.release['tag'] = 'v9.8.7'
                target = 'x86_64-pc-windows-msvc' if shell == 'ps' else 'x86_64-unknown-linux-gnu'
                name = self.archive(target)
                launch = lambda *args: self.run_bootstrap(shell, *args, tty=(shell == 'bash'))
                result = launch()
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.release['tag'] = 'v9.8.8'
                self.archive(target, tag='v9.8.8')
                result = launch()
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((self.install / 'fixture-tag').read_text(), 'v9.8.8')
                archive = '/releases/download/v9.8.8/' + name
                self.assertEqual(self.requests.count(archive), 1)
                result = launch()
                self.assertEqual(result.returncode, 0)
                self.assertEqual(self.requests.count(archive), 1, 'same version must not download again')
                # A legacy/unknown identity and a damaged bundle both require repair.
                for missing in ('fixture-tag', 'installed.ok'):
                    (self.install / missing).unlink()
                    result = launch()
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(self.requests.count(archive), 3)
                # Failure to check latest cannot silently run the old installer.
                before = self.log.read_bytes()
                self.release['unavailable'] = True
                result = launch()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.log.read_bytes(), before)
                requests = list(self.requests)
                result = launch('-Check' if shell == 'ps' else '--check')
                self.assertEqual(result.returncode, 2)
                self.assertEqual(self.requests, requests, 'Check must stay offline')
                self.release.pop('unavailable')
                (self.install / 'fixture-newer').write_text('native version comparison returned 3')
                result = launch()
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(self.requests.count(archive), 3, 'newer installed version is retained')
                result = launch('-Tag' if shell == 'ps' else '--tag', 'v9.8.7')
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((self.install / 'fixture-tag').read_text(), 'v9.8.7')
                self.assertTrue(self.install.resolve().is_relative_to(self.root.resolve()))
                shutil.rmtree(self.install)

    @unittest.skipUnless(PS, 'Windows PowerShell fixture')
    def test_windows_checksum_and_traversal_fail_before_destination_creation(self):
        for corrupt, unsafe in ((True, False), (False, True)):
            for file in self.root.glob('fixture.exe'):
                file.unlink()
            self.archive('x86_64-pc-windows-msvc', corrupt=corrupt, unsafe=unsafe)
            result = self.run_bootstrap('ps')
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.install.exists())
            self.assertFalse(self.log.exists())
            self.assertFalse((self.root / 'outside').exists())

    @unittest.skipUnless(PS, 'Windows PowerShell fixture')
    def test_windows_progress_known_unknown_and_failures_are_readable(self):
        self.archive('x86_64-pc-windows-msvc', padding=2 * 1024 * 1024)
        engines = list(dict.fromkeys(engine for engine in (PS, shutil.which('pwsh')) if engine))
        modes = ({'delay': 0.1}, {'delay': 0.1, 'unknown': True}, {'cut': True}, {'deny': True})
        for engine, mode in itertools.product(engines, modes):
            with self.subTest(engine=engine, mode=mode):
                self.transfer.clear()
                self.transfer.update(mode)
                result = self.run_bootstrap('ps', '-Update', powershell=engine)
                output = result.stderr.decode('utf-8', errors='strict')
                self.assertIn('MiB', output)
                self.assertNotIn('\x1b', output, 'redirected logs must not contain ANSI sequences')
                if mode.get('cut') or mode.get('deny'):
                    self.assertNotEqual(result.returncode, 0)
                    self.assertNotIn('100%', output)
                    self.assertNotIn('Downloaded', output)
                    self.assertFalse(self.log.exists(), 'failed download never launches installer')
                else:
                    self.assertEqual(result.returncode, 0, output)
                    self.assertIn('Downloaded' if mode.get('unknown') else '100%', output)
                    self.assertTrue(self.log.is_file())
                if self.log.exists():
                    self.log.unlink()

    @unittest.skipUnless(BASH and Path(BASH).is_file(), 'Bash fixture')
    def test_bash_progress_known_unknown_and_failures_are_readable(self):
        # A deliberately wrong digest stops before the native /dev/tty handoff.
        self.archive('x86_64-unknown-linux-gnu', corrupt=True, padding=2 * 1024 * 1024)
        for mode in ({'delay': 0.1}, {'delay': 0.1, 'unknown': True}, {'cut': True}, {'deny': True}):
            with self.subTest(mode=mode):
                self.transfer.clear()
                self.transfer.update(mode)
                result = self.run_bootstrap('bash')
                output = result.stderr.decode('utf-8', errors='strict')
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('\x1b', output)
                self.assertIn('MiB', output)
                if mode.get('cut') or mode.get('deny'):
                    self.assertNotIn('100%', output)
                    self.assertNotIn('Downloaded', output)
                    self.assertNotIn('SHA-256 mismatch', output)
                else:
                    self.assertIn('Downloaded' if mode.get('unknown') else '100%', output)
                    self.assertIn('SHA-256 mismatch', output)
                self.assertFalse(self.log.exists())

    @unittest.skipUnless(os.name == 'nt' and shutil.which('pwsh'), 'PowerShell 7 error renderer')
    def test_redirected_errors_stay_plain_with_forced_powershell_ansi(self):
        self.archive('x86_64-pc-windows-msvc')
        for mode in ({'cut': True}, {'deny': True}):
            with self.subTest(mode=mode):
                self.transfer.clear()
                self.transfer.update(mode)
                result = self.run_bootstrap('ps', powershell=shutil.which('pwsh'), force_ansi=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn(b'\x1b', result.stderr)
                self.assertFalse(self.log.exists())

    @unittest.skipUnless(os.getenv('LOCALVOX_TEST_PTY_RUNNER'), 'built terminal-capture fixture runner')
    def test_terminal_progress_redraws_and_ctrl_c_cleans_staging(self):
        shells = (['ps'] if os.name == 'nt' else []) + (['bash'] if BASH else [])
        for shell in shells:
            with self.subTest(shell=shell):
                self.check_terminal_progress(shell)

    def check_terminal_progress(self, shell):
        self.environment.pop('LOCALVOX_TEST_CANCEL_ON', None)
        self.transfer.clear()
        target = 'x86_64-pc-windows-msvc' if shell == 'ps' else 'x86_64-unknown-linux-gnu'
        self.archive(target, corrupt=True, padding=2 * 1024 * 1024)
        for unknown in (False, True):
            with self.subTest(unknown=unknown):
                self.transfer.update(delay=0.15, unknown=unknown, redirect=True)
                result = self.run_bootstrap(shell, tty=True)
                output = result.stdout.decode('utf-8', errors='strict')
                self.assertNotEqual(result.returncode, 0)  # Deliberate digest mismatch.
                self.assertGreater(output.count('MiB'), 3, output)
                self.assertIn('\r', output)
                self.assertIn('2.0', output.replace(',', '.'))
                self.assertIn('Downloaded' if unknown else '100%', output)
                self.assertFalse(self.log.exists())
                if folder := os.getenv('LOCALVOX_TEST_CAPTURE_DIR'):
                    Path(folder).mkdir(parents=True, exist_ok=True)
                    name = shell + ('-unknown.ansi' if unknown else '-known.ansi')
                    (Path(folder) / name).write_bytes(result.stdout)
        self.transfer.update(delay=0.3, unknown=False)
        self.environment['LOCALVOX_TEST_CANCEL_ON'] = '% '
        result = self.run_bootstrap(shell, tty=True)
        # Windows PowerShell reports an interrupted -File pipeline as exit 0;
        # cancellation must still clean staging and never launch the installer.
        if shell != 'ps':
            self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(b'100%', result.stdout)
        self.assertFalse(self.log.exists())

    @unittest.skipUnless(BASH and Path(BASH).is_file(), 'Bash fixture')
    def test_all_unix_target_routes_and_hash_failure_need_no_jq_or_terminal(self):
        for system, arch, target in [('Darwin', 'arm64', 'aarch64-apple-darwin'),
                                     ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
                                     ('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
                                     ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu')]:
            with self.subTest(target=target):
                name = self.archive(target, corrupt=True)
                result = self.run_bootstrap('bash', platform=(system, arch))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('SHA-256 mismatch', result.stderr.decode(errors='replace'))
                self.assertIn('/releases/download/v9.8.7/' + name, self.requests)
                self.assertFalse(self.install.exists())

    @unittest.skipIf(os.name == 'nt' or not BASH, 'native Unix controlling terminal')
    def test_unix_download_apply_reuse_and_archive_links(self):
        for system, arch, target in [('Darwin', 'arm64', 'aarch64-apple-darwin'),
                                     ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
                                     ('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
                                     ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu')]:
            with self.subTest(target=target):
                self.archive(target, unsafe=True)
                rejected = self.run_bootstrap('bash', platform=(system, arch), tty=True)
                self.assertNotEqual(rejected.returncode, 0)
                self.assertFalse(self.install.exists())
                self.archive(target)
                result = self.run_bootstrap('bash', platform=(system, arch), tty=True)
                self.assertEqual(result.returncode, 0, result.stdout.decode(errors='replace'))
                before = list(self.requests)
                result = self.run_bootstrap('bash', platform=(system, arch), tty=True)
                self.assertEqual(result.returncode, 0, result.stdout.decode(errors='replace'))
                self.assertEqual(self.requests[len(before):], ['/releases/latest', '/releases/tag/v9.8.7'])
                # Only the fixture owns and removes this explicitly resolved directory.
                self.assertTrue(self.install.resolve().is_relative_to(self.root.resolve()))
                shutil.rmtree(self.install)


if __name__ == '__main__':
    unittest.main()
