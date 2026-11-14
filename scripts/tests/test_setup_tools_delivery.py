"""OS-manager adapters with fake commands and tiny signed-by-hash payloads only."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PS = shutil.which('pwsh') or shutil.which('powershell')


@unittest.skipUnless(os.name == 'nt' and PS, 'Windows adapter fixture')
class SetupToolsDelivery(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-tools-fixture-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.install = self.root / 'installed'
        self.archive = self.root / 'mpv.zip'
        with zipfile.ZipFile(self.archive, 'w') as out:
            out.writestr('mpv.exe', b'player fixture')
            out.writestr('LICENSE.GPL', b'license fixture')
        self.metadata = self.root / 'metadata.json'
        self.log = self.root / 'managers.log'
        self.wrapper = self.root / 'fixture.ps1'
        self.wrapper.write_text('''param($Only,$InstallDir)
$ErrorActionPreference='Stop'
function global:winget.exe {
 [IO.File]::AppendAllText($env:FIXTURE_LOG, ($args -join '|')+"`n")
 $global:LASTEXITCODE=0
}
function global:Invoke-RestMethod { Get-Content -Raw -LiteralPath $env:FIXTURE_METADATA | ConvertFrom-Json }
function global:Invoke-WebRequest { param($Uri,$OutFile,[switch]$UseBasicParsing) [IO.File]::Copy($env:FIXTURE_ARCHIVE,$OutFile,$true) }
function global:Install-Module { throw 'Unexpected real module installation' }
function global:Install-PackageProvider { throw 'Unexpected real provider installation' }
& $env:FIXTURE_SCRIPT -Only $Only -InstallDir $InstallDir
''', encoding='utf-8')
        self.environment = {**os.environ, 'FIXTURE_LOG': str(self.log), 'FIXTURE_METADATA': str(self.metadata),
                            'FIXTURE_ARCHIVE': str(self.archive), 'FIXTURE_SCRIPT': str(ROOT / 'scripts/setup-tools.ps1')}

    def run_tools(self, only, digest=None):
        digest = digest or hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.metadata.write_text(json.dumps({'assets': [{'name': 'mpv-fixture-x86_64-w64-mingw32-full.zip',
                                                       'digest': 'sha256:' + digest, 'size': self.archive.stat().st_size,
                                                       'browser_download_url': 'https://fixture.invalid/mpv.zip'}]}))
        return subprocess.run([PS, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', self.wrapper,
                               '-Only', only, '-InstallDir', self.install], env=self.environment,
                              capture_output=True, timeout=30)

    def test_selected_manager_ids_and_verified_player_license(self):
        result = self.run_tools('ffmpeg,yt-dlp,mpv')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        commands = self.log.read_text()
        self.assertIn('Gyan.FFmpeg', commands)
        self.assertIn('yt-dlp.yt-dlp', commands)
        self.assertIn('--scope|user', commands)
        receipt = json.loads((self.install / 'tools/mpv/installed.json').read_text())
        self.assertEqual(receipt['executable_sha256'], hashlib.sha256(b'player fixture').hexdigest())
        self.assertTrue((self.install / 'tools/mpv/LICENSE.GPL').is_file())
        self.assertFalse((self.install / '.env').exists())

    def test_bad_download_hash_does_not_publish_player(self):
        result = self.run_tools('mpv', '0' * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.install / 'tools/mpv').exists())
        self.assertFalse(self.log.exists())

    def test_ollama_uses_official_publisher_and_unattended_options(self):
        self.wrapper.write_text('''param($CacheDir)
$ErrorActionPreference='Stop'
function global:Get-Command { param($Name,$ErrorAction) return $null }
function global:Get-AuthenticodeSignature { param($LiteralPath) return [pscustomobject]@{Status='Valid';SignerCertificate=[pscustomobject]@{Subject=$env:FIXTURE_SUBJECT}} }
function global:Invoke-WebRequest { param($Uri,$OutFile) [IO.File]::WriteAllText($OutFile,'installer fixture') }
function global:Start-Process {
 param($FilePath,$ArgumentList,$WindowStyle,[switch]$PassThru,[switch]$Wait)
 [IO.File]::WriteAllText($env:FIXTURE_LOG, ($ArgumentList -join '|')+'|'+$WindowStyle)
 return [pscustomobject]@{ExitCode=0}
}
& $env:FIXTURE_SCRIPT -CacheDir $CacheDir
''', encoding='utf-8')
        for index, organization in enumerate(('Ollama Inc.', 'Not Ollama Inc.', 'Ollama Inc')):
            with self.subTest(organization=organization):
                if self.log.exists():
                    self.log.unlink()
                env = {**self.environment, 'FIXTURE_SCRIPT': str(ROOT / 'scripts/setup-ollama.ps1'),
                       'LOCALAPPDATA': str(self.root / 'isolated profile'),
                       'FIXTURE_SUBJECT': f'CN=Ollama Inc., O={organization}, C=US'}
                result = subprocess.run([PS, '-NoProfile', '-File', self.wrapper,
                                         '-CacheDir', self.root / f'cache-{index}'], env=env,
                                        capture_output=True, timeout=30)
                if index == 0:
                    self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
                    flags = self.log.read_text()
                    self.assertIn('/VERYSILENT', flags)
                    self.assertIn('/NORESTART', flags)
                    self.assertIn('/SUPPRESSMSGBOXES', flags)
                    self.assertIn('Hidden', flags)
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(self.log.exists(), 'untrusted publisher must not run')


if __name__ == '__main__':
    unittest.main()
