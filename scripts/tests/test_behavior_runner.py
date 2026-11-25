"""The local acceptance runner must report failures without touching user state."""
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('behavior_runner', ROOT / 'scripts/test-behavior.py')
runner = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = runner
spec.loader.exec_module(runner)


class RunnerContracts(unittest.TestCase):
    def setUp(self):
        output = patch('sys.stdout', new=io.StringIO())
        output.start()
        self.addCleanup(output.stop)
        (ROOT / 'target/tmp').mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / 'target/tmp', prefix='behavior-contract-')
        self.addCleanup(self.temp.cleanup)
        self.output = Path(self.temp.name)

    def step(self, code, timeout=10):
        return runner.Step('fixture', 'tui', [sys.executable, '-c', code], timeout=timeout)

    def test_real_child_success_and_failure_keep_the_actual_exit_and_log(self):
        for code, expected in ((0, 'passed'), (7, 'failed')):
            result = runner.run_step(self.step(f'print("fixture output"); raise SystemExit({code})'),
                                     self.output, dict(os.environ))
            self.assertEqual(result['status'], expected)
            self.assertEqual(result['returncode'], code)
            self.assertIn('fixture output', (self.output / result['log']).read_text())

    def test_timeout_is_nonzero_and_stops_the_owned_process(self):
        result = runner.run_step(self.step('import time; print("started",flush=True); time.sleep(30)', 0.2),
                                 self.output, dict(os.environ))
        self.assertEqual(result['status'], 'timeout')
        self.assertNotEqual(result['returncode'], 0)
        self.assertLess(result['duration_sec'], 12)

    def test_installed_versions_are_checked_before_behavior_without_replacing_the_client(self):
        for text, status, actual in (('vox 0.2.13', 'passed', '0.2.13'),
                                     ('vox 0.2.11', 'blocked', '0.2.11'),
                                     ('not a vox version', 'blocked', None)):
            with self.subTest(text=text):
                result = runner.run_step(self.step(f'print({text!r})'), self.output,
                                         dict(os.environ), client_version=('vox', '0.2.13'))
                self.assertEqual(result['status'], status)
                self.assertEqual(result['returncode'], 0)
                self.assertEqual(result['actual_version'], actual)
                self.assertEqual(result['expected_version'], '0.2.13')
                self.assertEqual(result['command'][0], sys.executable)
        installed = self.output / 'old bundle'
        native = runner.Step('native', 'tui', ['cargo', 'test'], installed_client=True)
        source = runner.Step('source', 'tui', ['cargo', 'test'])
        self.assertIn('version preflight', runner.prerequisite(native, {}, installed, False))
        self.assertIsNone(runner.prerequisite(source, {}, installed, False))

    def test_external_shell_case_is_explicit_and_does_not_block_other_state_tests(self):
        steps = runner.manifest(system='Darwin')
        state = next(step for step in steps if step.name == 'tui-state-and-render')
        shell = next(step for step in steps if step.name == 'shell-registration')
        self.assertEqual(state.command[state.command.index('--skip') + 1], runner.SHELL_CASE)
        self.assertIn(runner.SHELL_CASE, shell.command)
        self.assertIn('--exact', shell.command)
        self.assertEqual(shell.requires, ('zsh', 'fish'))
        with patch.object(runner.shutil, 'which', side_effect=lambda name, **_: '/bin/zsh' if name == 'zsh' else None):
            self.assertIn('fish', runner.prerequisite(shell, {}))
            self.assertIsNone(runner.prerequisite(state, {}))
        self.assertNotIn('shell-registration', [step.name for step in runner.manifest(system='Windows')])

    def test_unsupported_python_writes_blocked_report_before_compilation_or_toml_import(self):
        self.assertIn('3.11+', runner.python_issue((3, 10, 16)))
        self.assertIsNone(runner.python_issue((3, 11, 0)))
        with patch.object(runner, 'ROOT', self.output), \
                patch.object(runner.sys, 'argv', ['test-behavior.py', '--group', 'tui']), \
                patch.object(runner, 'python_issue', return_value=runner.python_issue((3, 10, 16))), \
                patch.object(runner, 'workspace_version') as version, \
                patch.object(runner, 'isolated_environment') as environment, \
                patch.object(runner, 'run_step') as run:
            self.assertEqual(runner.main(), 1)
            run.assert_not_called()
            version.assert_not_called()
            environment.assert_not_called()
        output = next((self.output / 'target/behavior').iterdir())
        value = json.loads((output / 'report.json').read_text(encoding='utf-8'))
        self.assertEqual(value['results'][0]['status'], 'blocked')
        self.assertTrue(all(item['status'] == 'not-run' for item in value['results'][1:]))
        self.assertIn('3.11+', (output / 'AGENT_TASK.md').read_text(encoding='utf-8'))

    def test_missing_gui_dependencies_have_both_preparation_commands(self):
        with patch.object(runner, 'ROOT', self.output):
            reason = runner.prerequisite(runner.Step('gui-unit', 'gui', ['npm', 'test']), {})
        self.assertIn('npm ci', reason)
        self.assertIn('playwright install chromium', reason)

    def test_blocked_failures_and_unexecuted_checks_remain_distinct_in_the_final_status(self):
        for states, expected in ((['passed'], 'PASS'), (['passed', 'blocked'], 'BLOCKED'),
                                 (['blocked', 'failed'], 'FAIL'), (['timeout'], 'FAIL'),
                                 (['interrupted', 'not-run'], 'INTERRUPTED'),
                                 (['passed', 'not-run'], 'NOT-RUN'), (['unknown'], 'NOT-RUN'), ([], 'NOT-RUN')):
            self.assertEqual(runner.outcome([{'status': state} for state in states]), expected)

    def test_interrupting_the_version_probe_does_not_start_a_second_client_or_build(self):
        interrupted = {'name': 'installed-vox-version', 'group': 'tui', 'command': ['vox', '--version'],
                       'cwd': '.', 'status': 'interrupted', 'duration_sec': 0, 'log': 'version.log'}
        with patch.object(runner, 'ROOT', self.output), \
                patch.object(runner.sys, 'argv', ['test-behavior.py', '--group', 'tui', '--installed-dir', str(self.output)]), \
                patch.object(runner, 'workspace_version', return_value='0.2.13'), \
                patch.object(runner, 'isolated_environment', return_value=dict(os.environ)), \
                patch.object(runner, 'run_step', return_value=interrupted) as run:
            self.assertEqual(runner.main(), 1)
            self.assertEqual(run.call_count, 1)
        output = next((self.output / 'target/behavior').iterdir())
        value = json.loads((output / 'report.json').read_text(encoding='utf-8'))
        self.assertEqual(value['outcome'], 'INTERRUPTED')
        self.assertTrue(all(item['status'] == 'not-run' for item in value['results'][1:]))

    def test_missing_tool_has_a_log_and_actionable_agent_task_not_a_green_report(self):
        step = runner.Step('missing-cargo', 'tui', ['a-localvox-test-tool-that-does-not-exist'])
        result = runner.run_step(step, self.output, dict(os.environ))
        self.assertEqual(result['status'], 'blocked')
        self.assertTrue((self.output / result['log']).is_file())
        self.assertFalse(runner.report(self.output, [result], ['tui'], None))
        report = json.loads((self.output / 'report.json').read_text(encoding='utf-8'))
        self.assertEqual(report['results'][0]['status'], 'blocked')
        task = (self.output / 'AGENT_TASK.md').read_text(encoding='utf-8')
        self.assertIn(step.command[0], task)
        self.assertIn('не инструкции', task)
        self.assertIn('Rust toolchain', task)

    def test_excess_output_is_bounded_but_retains_the_final_failure_message(self):
        with patch.object(runner, 'LOG_LIMIT', 1024):
            result = runner.run_step(self.step('print("x"*100000); print("FINAL FAILURE"); raise SystemExit(3)'),
                                     self.output, dict(os.environ))
        log = (self.output / result['log']).read_bytes()
        self.assertEqual(result['status'], 'failed')
        self.assertTrue(result['truncated'])
        self.assertLess(len(log), 1024 + 65536 + 100)
        self.assertIn(b'FINAL FAILURE', log)

    def test_an_empty_or_ignored_rust_suite_cannot_pass_acceptance(self):
        class EmptySuite:
            pid = 123
            returncode = 0
            stdout = io.BytesIO(b'test result: ok. 0 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out\n')
            def wait(self, timeout): return 0
            def poll(self): return 0
        with patch.object(runner.shutil, 'which', return_value=sys.executable), \
                patch.object(runner.subprocess, 'Popen', return_value=EmptySuite()):
            result = runner.run_step(runner.Step('empty', 'tui', ['cargo', 'test']), self.output, dict(os.environ))
        self.assertEqual(result['status'], 'failed')
        self.assertEqual(result['ignored_tests'], 3)
        self.assertIn('No Rust behavioral test ran', result['reason'])

    def test_environment_isolates_temporary_files_and_removes_application_credentials(self):
        with patch.dict(os.environ, {'LOCALVOX_API_TOKEN': 'private', 'LOCALVOX_LIGHT_WORK_DIR': '/private/archive',
                                     'GH_TOKEN': 'private-github', 'LOCALVOX_TUI_KEYMAP': '/private/keymap'}):
            environment = runner.isolated_environment(self.output, 1)
        for key in ('LOCALVOX_API_TOKEN', 'LOCALVOX_LIGHT_WORK_DIR', 'GH_TOKEN', 'LOCALVOX_TUI_KEYMAP'):
            self.assertNotIn(key, environment)
        self.assertEqual(Path(environment['TEMP']), self.output / 'tmp')
        self.assertEqual(environment['INSTA_UPDATE'], 'no')
        self.assertEqual(environment['CARGO_BUILD_JOBS'], '1')
        self.assertEqual(environment['LOCALVOX_BEHAVIOR_PROFILE'], 'behavior')
        self.assertEqual(environment['LOCALVOX_YTDLP_AUTO_UPDATE'], 'off')

    def test_manifest_covers_all_platform_neutral_groups_and_explicit_real_mpv(self):
        steps = runner.manifest()
        self.assertEqual({s.group for s in steps}, set(runner.GROUPS))
        for step in steps:
            if step.command[0] == 'cargo':
                if step.command[1] == 'test':
                    self.assertIn('--test-threads=1', step.command)
                self.assertEqual(step.command[step.command.index('--jobs')+1], '1')
        media = next(s for s in steps if s.group == 'mpv')
        self.assertIn('--ignored', media.command)
        self.assertIn('media_playback', media.command)
        browser = next(s for s in steps if s.name == 'gui-browser')
        self.assertIn('--update-snapshots=none', browser.command)
        self.assertIn('--grep-invert', browser.command)
        visual = next(s for s in runner.manifest(visual=True) if s.name == 'gui-browser')
        self.assertNotIn('--grep-invert', visual.command)
        shared = next(s for s in steps if s.name == 'gui-and-tui-real-api')
        self.assertGreater(shared.command.index('--exact'), shared.command.index('--'))

    def test_each_platform_reuses_the_explicit_bundle_native_library(self):
        installed = self.output / 'installed bundle'
        installed.mkdir()
        for system, name in (('Windows', 'libvosk.dll'), ('Darwin', 'libvosk.dylib'), ('Linux', 'libvosk.so')):
            with self.subTest(system=system), patch.object(runner.platform, 'system', return_value=system):
                library = installed / name
                library.write_bytes(b'native fixture')
                self.assertEqual(runner.native_library(installed), library)
        environment = runner.isolated_environment(self.output, 1, installed)
        self.assertEqual(environment['LOCALVOX_BEHAVIOR_NATIVE_DIR'], str(installed))

    def test_a_clean_report_still_marks_hardware_and_visual_comparisons_not_run(self):
        result = {'name': 'fixture', 'status': 'passed', 'duration_sec': 1.0, 'log': 'fixture.log'}
        self.assertTrue(runner.report(self.output, [result], ['tui'], self.output))
        report = json.loads((self.output / 'report.json').read_text(encoding='utf-8'))
        self.assertFalse(report['visual_snapshots'])
        self.assertTrue(report['not_run'])
        self.assertEqual(report['installed_tui'], str(self.output))
        self.assertIn('отказов нет', (self.output / 'AGENT_TASK.md').read_text(encoding='utf-8'))


if __name__ == '__main__': unittest.main()
