#!/usr/bin/env python3
"""Exercise a complete native bundle without models, inference or developer settings."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(arguments, expected=0, *, cwd=None, input=None, environment=None):
    result = subprocess.run([str(a) for a in arguments], cwd=cwd, input=input,
                            env=environment, text=True, encoding='utf-8', errors='replace',
                            capture_output=True, timeout=90)
    if result.returncode != expected:
        raise RuntimeError(f'{Path(str(arguments[0])).name}: exit {result.returncode}, expected {expected}\n'
                           + result.stdout[-4000:] + result.stderr[-4000:])
    return result.stdout


def snapshot(directory):
    return {str(p.relative_to(directory)): ('directory',) if p.is_dir() else
            (p.stat().st_mtime_ns, hashlib.sha256(p.read_bytes()).hexdigest())
            for p in directory.rglob('*')}


def inspect_doctor(text):
    report = json.loads(text)
    if report.get('schema') != 1:
        raise RuntimeError('Doctor report schema is not 1')
    findings = report['findings']
    indexed = {item['id']: item['state'] for item in findings}
    required = {'vosk', 'gigaam', 'worker', 'archive', 'ollama_cli', 'ollama_api', 'llm_model'}
    if len(indexed) != len(findings) or not required.issubset(indexed):
        raise RuntimeError('Doctor report missing stable component IDs or has duplicates')
    if indexed['worker'] != 'ok' or indexed['vosk'] != 'fail' or indexed['gigaam'] != 'fail':
        raise RuntimeError(f'Doctor must detect installed worker and deliberately missing ASR: {indexed}')
    return indexed


def check(archive):
    archive = archive.resolve(strict=True)
    environment = {k: v for k, v in os.environ.items() if not k.startswith('LOCALVOX_')}
    windows = os.name == 'nt'
    if windows:
        shell = shutil.which('pwsh') or shutil.which('powershell')
        if not shell:
            raise RuntimeError('PowerShell is required')
        installer = [shell, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ROOT / 'scripts/install-release.ps1']
    else:
        installer = ['bash', ROOT / 'scripts/install-release.sh']
    with tempfile.TemporaryDirectory(prefix='localvox-installed-check-') as temporary:
        install = Path(temporary) / 'LocalVox with spaces'
        profile = Path(temporary) / 'isolated user'
        profile.mkdir()
        environment.update(HOME=str(profile), USERPROFILE=str(profile))

        def install_call(*flags, expected=0, input=None):
            # Never change the runner/developer's real PATH or shell profiles.
            options = ['-InstallDir', install, '-NoPathUpdate'] if windows else [f'--install-dir={install}', '--no-path-update']
            return run(installer + options + list(flags), expected, input=input, environment=environment)

        install_call('-Check' if windows else '--check', expected=2)
        if install.exists():
            raise RuntimeError('Check created the installation directory')
        install.mkdir()
        settings = {
            'LOCALVOX_LIGHT_MODEL': (install / 'models/vosk-model-ru-0.42').as_posix(),
            'LOCALVOX_ASR_MODEL_DIR': (install / 'models/gigaam-v3-e2e-ctc').as_posix(),
            'LOCALVOX_LIGHT_AUDIO_DIR': (install / 'archive').as_posix(),
            'LOCALVOX_LLM_BASE_URL': 'http://127.0.0.1:1/v1',
            'LOCALVOX_LLM_MODEL': 'fixture:missing',
            'LOCALVOX_LIGHT_YT_DLP': (install / 'not-installed-yt-dlp').as_posix(),
        }
        original = '# private smoke configuration\n' + ''.join(
            f'{key}={json.dumps(value, ensure_ascii=False)}\n' for key, value in settings.items())
        (install / '.env').write_text(original, encoding='utf-8')
        flags = ['-Archive', archive, '-SkipModels'] if windows else [f'--archive={archive}', '--skip-models']
        install_call(*flags)
        before = snapshot(install)
        binary = install / ('localvox-light.exe' if windows else 'localvox-light')
        findings = inspect_doctor(run([binary, '--doctor', '--doctor-json'], 2, cwd=install, environment=environment))
        run([binary, '--setup-check'], 2, cwd=install, environment=environment)
        # The checkbox wizard requires an actual terminal; piped numeric answers
        # must fail clearly, never accidentally accept an installation plan.
        run([binary, '--setup'], 1, cwd=install, input='q\n', environment=environment)
        native_installer = install / ('localvox-install.exe' if windows else 'localvox-install')
        run([native_installer, '--bundle-check', '--install-dir', install], cwd=install, environment=environment)
        run([native_installer, '--install-dir', install, '--no-path-update'], 1,
            cwd=install, input='\n', environment=environment)
        install_call('-Check' if windows else '--check', expected=2)
        absent = Path(temporary) / 'must-not-download-or-unpack.zip'
        repeat = ['-Archive', absent, '-Reconcile', '-SkipModels'] if windows else [
            f'--archive={absent}', '--reconcile', '--skip-models']
        install_call(*repeat)
        wizard = ['-Archive', absent, '-Reconcile'] if windows else [f'--archive={absent}', '--reconcile']
        install_call(*wizard, expected=1, input='q\n')
        if snapshot(install) != before:
            raise RuntimeError('Doctor/check/cancel/no-op/reconcile changed installed files or mtimes')
        models = Path(temporary) / 'must-not-create-models'
        fetch = ([shell, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', install / 'scripts/fetch-models.ps1',
                  '-Root', models, '-Check'] if windows else
                 ['bash', install / 'scripts/fetch-models.sh', '--root', models, '--check'])
        run(fetch, 1, environment=environment)
        if models.exists() or (install / 'archive').exists():
            raise RuntimeError('Read-only/no-op commands created model or archive directories')
        if (install / '.env').read_text(encoding='utf-8') != original:
            raise RuntimeError('Installer changed existing configuration')
        return {'doctor': findings, 'missing_asr_exit': 2, 'check_before_install': True,
                'wizard_requires_tty_without_mutation': True, 'bundle_check': True,
                'reconcile_unchanged': True, 'fetch_check_readonly': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.archive), ensure_ascii=False, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, subprocess.SubprocessError, ValueError, KeyError) as error:
        raise SystemExit(str(error)) from error
