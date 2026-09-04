#!/usr/bin/env python3
"""Explicit local Rust gateway gate; never provision or execute fixtures on host."""
import json
import os
import pathlib
import platform
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    if sys.platform != 'darwin' or platform.machine() != 'arm64':
        raise RuntimeError('Rust gateway is calibrated only on macOS ARM64/Docker Linux ARM64')
    if not os.environ.get('RUST_MCP_TEST_SOCKET'):
        raise RuntimeError('RUST_MCP_TEST_SOCKET required; no automatic socket selection')
    allowed = {'HOME', 'PATH', 'TMPDIR', 'CARGO_HOME', 'RUSTUP_HOME',
               'SDKROOT', 'DEVELOPER_DIR', 'CARGO_TARGET_DIR', 'RUST_MCP_TEST_SOCKET'}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL='0', MCP_TEST_SYNTHETIC_SECRET='synthetic-not-a-secret')
    # Query an installed toolchain, never a shim that might install on first use.
    cargo = pathlib.Path(subprocess.check_output(
        ['rustup', 'which', '--toolchain', '1.98.1', 'cargo'], env=env, text=True).strip())
    rustc = cargo.with_name('rustc')
    env['PATH'] = str(cargo.parent) + os.pathsep + env.get('PATH', '')
    env['RUSTC'] = str(rustc)
    for binary, prefix in [(cargo, 'cargo 1.98.1 '), (rustc, 'rustc 1.98.1 ')]:
        actual = subprocess.check_output([str(binary), '--version'], env=env, text=True)
        if not actual.startswith(prefix):
            raise RuntimeError('Rust/Cargo1.98.1 required; no runtime substitution')
    output = ROOT / 'target/rust-security'
    output.mkdir(parents=True, exist_ok=True)
    tests = [
        ('rust-engineering-execution', ['--lib'],
         'rust_gateway::tests::benign_source_transfer_compiles_with_empty_directory'),
        ('rust-engineering-execution', ['--lib'],
         'rust_calibration::tests::observed_descendants_are_cleaned_on_timeout_cancel_and_overflow'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'toolchain_inspect_observes_installed_runtime_with_shared_calibration'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'eof_and_cancellation_during_calibration_join_workers_and_leave_no_owned_objects'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'check_reports_success_and_borrow_errors_with_live_log_resources'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'check_cancellation_and_eof_join_active_cargo_jobs'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'format_reports_workspace_diffs_without_source_writes'),
        ('rust-engineering-execution', ['--lib'],
         'rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'clippy_profiles_report_findings_and_verified_log_resources'),
        ('rust-engineering-execution', ['--lib'],
         'rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'test_reports_results_selections_and_verified_harness_logs'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'test_cancellation_and_eof_join_active_test_binaries'),
        ('rust-engineering-execution', ['--lib'],
         'rust_gateway::test_runtime::actual_proc_macro_forgery_cannot_hide_later_cargo_failure'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'audit_runtime::audit_real_rsa_and_lock_generations_are_captured_without_building'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'audit_runtime::audit_snapshot_freshness_and_advisory_classification_are_distinct'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'audit_runtime::audit_missing_integrity_path_and_symlink_fail_closed'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'explain_runtime::compiler_explanation_requires_no_project_and_preserves_actual_evidence'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'quality_runtime::quality_fast_retains_format_and_strict_clippy_failures_and_readonly_source'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'quality_runtime::quality_standard_distinguishes_pass_test_failure_and_unavailable_audit'),
        ('rust-engineering-mcp', ['--test', 'inspection_runtime'],
         'quality_runtime::quality_cancellation_and_eof_join_the_active_test_stage'),
    ]
    for number, (package, target, test) in enumerate(tests):
        command = [str(cargo), 'test', '--locked', '--offline', '-p', package,
                   *target, test, '--',
                   '--exact', '--ignored', '--nocapture', '--test-threads=1']
        log = output / f'{number}.log'
        print(f'RUST SECURITY {test}', flush=True)
        with log.open('wb') as stream:
            result = subprocess.run(command, cwd=ROOT, env=env,
                                    stdout=stream, stderr=subprocess.STDOUT)
        content = log.read_text()
        if result.returncode or 'test result: ok. 1 passed;' not in content:
            raise RuntimeError(f'Rust security test failed or did not execute: {log}')
        if number == 1:
            marker = '{"scope":"rust-cargo-source-profile-v1"'
            reports = [json.loads(marker + line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if len(reports) != 1 or not reports[0]['verified']:
                raise RuntimeError('Missing successful actual Rust calibration receipt')
            (output / 'calibration.json').write_text(json.dumps(reports[0], indent=2) + '\n')
        if number == 2:
            marker = 'M1_INSPECTION_RECEIPT '
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if len(reports) != 1 or reports[0]['status'] != 'passed':
                raise RuntimeError('Missing successful MCP inspection receipt')
            (output / 'mcp-inspection.json').write_text(json.dumps(reports[0], indent=2) + '\n')
            marker = 'M1_TOOLCHAIN_RECEIPT '
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if len(reports) != 1 or reports[0]['status'] != 'passed':
                raise RuntimeError('Missing successful MCP toolchain receipt')
            (output / 'mcp-toolchain.json').write_text(json.dumps(reports[0], indent=2) + '\n')
        if number == 4:
            marker = 'M1_CHECK_RECEIPT '
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if (len(reports) != 1 or reports[0]['passed'] != ['passed', 'passed']
                    or reports[0]['failed'] != ['failed', 'failed']
                    or reports[0]['logs_verified'] != 6):
                raise RuntimeError('Missing successful actual check/Resources receipt')
            (output / 'mcp-check.json').write_text(json.dumps(reports[0], indent=2) + '\n')
        if number == 6:
            marker = 'M1_FORMAT_RECEIPT '
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if (len(reports) != 1 or reports[0]['status'] != 'passed'
                    or reports[0]['cases'] != 7 or reports[0]['logs_verified'] != 7):
                raise RuntimeError('Missing successful actual formatting receipt')
            (output / 'mcp-format.json').write_text(json.dumps(reports[0], indent=2) + '\n')
        if number in (7, 8):
            marker, filename, cases = (
                ('M1_CLIPPY_CONTAINMENT_RECEIPT ', 'clippy-containment.json', 2)
                if number == 7 else ('M1_CLIPPY_RECEIPT ', 'mcp-clippy.json', 6)
            )
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if (len(reports) != 1 or reports[0]['status'] != 'passed'
                    or reports[0]['cases'] != cases or not reports[0]['cleanup']):
                raise RuntimeError('Missing successful actual Clippy receipt')
            (output / filename).write_text(json.dumps(reports[0], indent=2) + '\n')
        if number in (9, 10, 11, 12):
            marker, filename = {
                9: ('M1_TEST_CONTAINMENT_RECEIPT ', 'test-containment.json'),
                10: ('M1_TEST_RECEIPT ', 'mcp-test.json'),
                11: ('M1_TEST_CANCELLATION_RECEIPT ', 'mcp-test-cancellation.json'),
                12: ('M1_TEST_FORGERY_RECEIPT ', 'test-forgery.json'),
            }[number]
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            if len(reports) != 1 or not reports[0]['cleanup']:
                raise RuntimeError('Missing successful actual test receipt')
            report = reports[0]
            if number == 9 and (len(report['observations']) != 4
                               or any(not item['cleanup'] for item in report['observations'][1:])):
                raise RuntimeError('Missing actual libtest containment scenarios')
            if number == 10 and (report['cases'] != 9 or report['logs_sha256_verified'] != 9):
                raise RuntimeError('Missing actual MCP test/Resources cases')
            if number == 11 and report['active_test_binaries_observed'] != 2:
                raise RuntimeError('Missing active test cancellation/EOF observations')
            if number == 12 and (not report['forged_success_forwarded']
                                 or report['parser_complete']
                                 or report['execution']['exit_code'] != 101):
                raise RuntimeError('Missing real forged-phase rejection')
            (output / filename).write_text(json.dumps(report, indent=2) + '\n')
        if number in (13, 14, 15):
            marker = 'M1_AUDIT_RECEIPT '
            reports = [json.loads(line.partition(marker)[2])
                       for line in content.splitlines() if marker in line]
            expected_cases = {13: 4, 14: 6, 15: 5}[number]
            if (len(reports) != 1 or reports[0]['cases'] != expected_cases
                    or not reports[0]['cleanup']):
                raise RuntimeError('Missing actual MCP audit receipt')
            filename = {13: 'mcp-audit-rsa.json', 14: 'mcp-audit-freshness.json',
                        15: 'mcp-audit-denials.json'}[number]
            (output / filename).write_text(json.dumps(reports[0], indent=2) + '\n')
        if number == 16:
            reports = [json.loads(line.split('M1_EXPLAIN_RECEIPT ', 1)[1])
                       for line in content.splitlines() if 'M1_EXPLAIN_RECEIPT ' in line]
            if len(reports) != 1 or reports[0]['cases'] != 10 or not reports[0]['cleanup']:
                raise RuntimeError('Missing actual MCP compiler explanation receipt')
            (output / 'mcp-explain.json').write_text(json.dumps(reports[0], indent=2) + '\n')
        if number in (17, 18, 19):
            reports = [json.loads(line.split('M1_QUALITY_RECEIPT ', 1)[1])
                       for line in content.splitlines() if 'M1_QUALITY_RECEIPT ' in line]
            cases = {17: 2, 18: 3, 19: 3}[number]
            if len(reports) != 1 or reports[0]['cases'] != cases or not reports[0]['cleanup']:
                raise RuntimeError('Missing actual MCP quality gate receipt')
            if number == 19 and reports[0]['active_quality_test_binaries_observed'] != 2:
                raise RuntimeError('Missing active quality test cancellation/EOF evidence')
            filename = {17: 'mcp-quality-fast.json', 18: 'mcp-quality-standard.json',
                        19: 'mcp-quality-cancellation.json'}[number]
            (output / filename).write_text(json.dumps(reports[0], indent=2) + '\n')
    print(f'PASS actual Rust containment, MCP inspection/check/format/clippy/test/audit/explain/quality/Resources and joined cleanup: {output}')


if __name__ == '__main__':
    if not __debug__:
        raise RuntimeError('Optimized Python mode is rejected')
    main()
