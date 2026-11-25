"""Unit tests for canary.py. No QEMU, no network: python3 -m unittest discover -s tools/qemu -v"""

import io
import unittest
from contextlib import redirect_stdout
from unittest import mock

import canary
from canary import FAIL, PASS, Verdict, verdict

KNOWN = "test_v23_spi0_byte_rate[raspi3b]"


def report(n=canary.EXPECTED, failed=(), skipped=(), case_errors=(), suite_error=None):
    """A JUnit report with n cases, the way pytest writes it."""
    cases = []
    names = [KNOWN] + [f"test_case_{i}" for i in range(1, n)] if n else []
    for name in names[:n]:
        body = ""
        if name in failed:
            body = '<failure message="KeyError: R15">trace</failure>'
        elif name in case_errors:
            body = '<error message="fixture failed">trace</error>'
        elif name in skipped:
            body = '<skipped type="pytest.skip" message="no arm-none-eabi-objcopy"/>'
        cases.append(f'<testcase classname="t" name="{name}">{body}</testcase>')
    err = f'<error message="{suite_error}"/>' if suite_error else ""
    return (f'<?xml version="1.0"?><testsuites><testsuite name="pytest">'
            f'{err}{"".join(cases)}</testsuite></testsuites>')


class VerdictTest(unittest.TestCase):
    def test_all_pass(self):
        v = verdict(0, report())
        self.assertEqual(v.status, PASS)
        self.assertEqual(v.passed, 78)

    def test_one_case_fails(self):
        v = verdict(1, report(failed=[KNOWN]))
        self.assertEqual(v.status, FAIL)
        self.assertEqual(v.failed, [KNOWN])
        self.assertIn("1 case(s) failed", v.reason)

    def test_two_cases_fail(self):
        v = verdict(1, report(failed=[KNOWN, "test_case_5"]))
        self.assertEqual(v.status, FAIL)
        self.assertIn("2 case(s) failed", v.reason)

    def test_suite_level_error_without_failed_case(self):
        v = verdict(2, report(n=0, suite_error="collection failed"))
        self.assertEqual(v.status, FAIL)
        self.assertIn("error", v.reason)

    def test_suite_error_with_all_cases_passed(self):
        v = verdict(0, report(suite_error="teardown failed"))
        self.assertEqual(v.status, FAIL)

    def test_case_error(self):
        v = verdict(1, report(case_errors=[KNOWN]))
        self.assertEqual(v.status, FAIL)
        self.assertIn("1 error(s)", v.reason)

    def test_nonzero_exit_with_clean_report(self):
        v = verdict(3, report())
        self.assertEqual(v.status, FAIL)
        self.assertIn("exit code 3", v.reason)
        self.assertEqual(verdict(1, report()).status, FAIL)

    def test_one_case_skipped(self):
        v = verdict(0, report(skipped=["test_case_7"]))
        self.assertEqual(v.status, FAIL)
        self.assertEqual(v.skipped, [("test_case_7", "no arm-none-eabi-objcopy")])
        self.assertIn("skipped", v.reason)

    def test_fewer_cases_than_expected(self):
        v = verdict(0, report(n=77))
        self.assertEqual(v.status, FAIL)
        self.assertIn("77 case(s) ran, expected 78", v.reason)

    def test_empty_report(self):
        for text in ("", "   \n"):
            v = verdict(0, text)
            self.assertEqual(v.status, FAIL)
            self.assertIn("empty", v.reason)

    def test_report_not_xml(self):
        v = verdict(0, "ERROR: file or directory not found")
        self.assertEqual(v.status, FAIL)
        self.assertIn("not XML", v.reason)


class MainTest(unittest.TestCase):
    def main_with(self, v):
        with mock.patch.object(canary, "run", return_value=v) as run, \
             mock.patch("sys.argv", ["canary.py", "test"]), \
             mock.patch.dict("os.environ", {"GITHUB_STEP_SUMMARY": ""}), \
             redirect_stdout(io.StringIO()) as out:
            code = canary.main()
        return code, run.call_count, out.getvalue()

    def test_pass_runs_once(self):
        code, calls, _ = self.main_with(Verdict(PASS, "ok", passed=canary.EXPECTED))
        self.assertEqual((code, calls), (0, 1))

    def test_failed_case_fails_at_once(self):
        v = Verdict(FAIL, "1 case(s) failed", failed=[KNOWN])
        code, calls, out = self.main_with(v)
        self.assertEqual((code, calls), (1, 1))
        self.assertIn(f"failed: `{KNOWN}`", out)


if __name__ == "__main__":
    unittest.main()
