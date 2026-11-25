"""Unit tests for canary.py. No QEMU, no network: python3 -m unittest discover -s spikes/k1 -v"""

import io
import unittest
from contextlib import redirect_stdout
from unittest import mock

import canary
from canary import FAIL, KNOWN, PASS, RETRY, Verdict, verdict


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

    def test_only_known_race_fails_is_retried(self):
        v = verdict(1, report(failed=[KNOWN]))
        self.assertEqual(v.status, RETRY)
        self.assertEqual(v.failed, [KNOWN])

    def test_retry_run_of_known_case(self):
        self.assertEqual(verdict(0, report(n=1), expected=1).status, PASS)
        self.assertEqual(verdict(1, report(n=1, failed=[KNOWN]), expected=1).status, RETRY)

    def test_other_case_fails(self):
        v = verdict(1, report(failed=["test_case_5"]))
        self.assertEqual(v.status, FAIL)
        self.assertIn("other than the known race", v.reason)

    def test_known_and_other_case_fail(self):
        self.assertEqual(verdict(1, report(failed=[KNOWN, "test_case_5"])).status, FAIL)

    def test_suite_level_error_without_failed_case(self):
        v = verdict(2, report(n=0, suite_error="collection failed"))
        self.assertEqual(v.status, FAIL)
        self.assertIn("error", v.reason)

    def test_suite_error_with_all_cases_passed(self):
        v = verdict(0, report(suite_error="teardown failed"))
        self.assertEqual(v.status, FAIL)

    def test_case_error_on_known_race_is_not_retried(self):
        v = verdict(1, report(case_errors=[KNOWN]))
        self.assertEqual(v.status, FAIL)
        self.assertIn("never retried", v.reason)

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
    def main_with(self, verdicts):
        with mock.patch.object(canary, "run", side_effect=verdicts) as run, \
             mock.patch("sys.argv", ["canary.py", "test"]), \
             mock.patch.dict("os.environ", {"GITHUB_STEP_SUMMARY": ""}), \
             redirect_stdout(io.StringIO()) as out:
            code = canary.main()
        return code, run.call_count, out.getvalue()

    def test_known_race_fails_three_times(self):
        r = Verdict(RETRY, "known", failed=[KNOWN])
        code, calls, out = self.main_with([r, r, r])
        self.assertEqual((code, calls), (1, 3))
        self.assertIn("failed 3 times in a row", out)

    def test_known_race_passes_on_second_retry(self):
        r = Verdict(RETRY, "known", failed=[KNOWN])
        code, calls, out = self.main_with([r, r, Verdict(PASS, "ok", passed=1)])
        self.assertEqual((code, calls), (0, 3))
        self.assertIn("passed on retry 2 of 2", out)

    def test_retry_that_errors_fails_at_once(self):
        r = Verdict(RETRY, "known", failed=[KNOWN])
        code, calls, _ = self.main_with([r, Verdict(FAIL, "report missing")])
        self.assertEqual((code, calls), (1, 2))

    def test_first_pass_fail_is_not_retried(self):
        code, calls, _ = self.main_with([Verdict(FAIL, "skipped")])
        self.assertEqual((code, calls), (1, 1))


if __name__ == "__main__":
    unittest.main()
