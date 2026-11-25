"""Run RobCo-Core's canary once, and retry only one known racy case.

Usage: python canary.py <system> [extra pytest args...]

The first pass is the result that counts. It goes to the job summary.
If the only failed case is the known race, that case alone is rerun up
to two more times. Anything else fails the job: another failed case, an
error at suite or case level, a skipped case, a case count other than
EXPECTED, a non-zero pytest exit code without a failed case, or a
report that is missing, empty or not XML.

verdict() makes the decision and runs nothing, so test_canary.py can
test it without QEMU.
"""

import os
import subprocess
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field

KNOWN = "test_v23_spi0_byte_rate[raspi3b]"
SUITE = "robco-core/tests/canary"
RETRIES = 2
# Cases in tests/canary/test_qemu_assumptions.py at the RobCo-Core
# commit pinned as ROBCO_CORE_SHA in .github/workflows/spike-k1.yml
# (c8220b2). Change both together.
EXPECTED = 78

PASS, RETRY, FAIL = "pass", "retry", "fail"


@dataclass
class Verdict:
    status: str
    reason: str
    passed: int = 0
    failed: list = field(default_factory=list)
    skipped: list = field(default_factory=list)


def verdict(code, xml_text, expected=EXPECTED):
    """Decide from pytest's exit code and the JUnit XML text.

    PASS: every expected case ran and passed. RETRY: the only problem is
    the known race failing. FAIL: anything else.
    """
    if not xml_text or not xml_text.strip():
        return Verdict(FAIL, f"JUnit report is empty (pytest exit code {code})")
    try:
        root = ET.fromstring(xml_text)
    except ET.ParseError as e:
        return Verdict(FAIL, f"JUnit report is not XML: {e} (pytest exit code {code})")
    suites = [root] if root.tag == "testsuite" else list(root.iter("testsuite"))
    if root.tag not in ("testsuite", "testsuites"):
        return Verdict(FAIL, f"JUnit report has root <{root.tag}>")

    v = Verdict(FAIL, "")
    errors = []
    for s in suites:
        for e in s.findall("error"):
            errors.append(f"suite {s.get('name')}: {e.get('message', '')}")
        for c in s.iter("testcase"):
            name = c.get("name")
            if c.find("error") is not None:
                errors.append(name)
            elif c.find("failure") is not None:
                v.failed.append(name)
            elif c.find("skipped") is not None:
                sk = c.find("skipped")
                v.skipped.append((name, sk.get("message", "")))
            else:
                v.passed += 1
    ran = v.passed + len(v.failed)

    if errors:
        v.reason = f"{len(errors)} error(s), never retried: " + ", ".join(errors)
    elif code not in (0, 1):
        v.reason = f"pytest exit code {code}"
    elif v.skipped:
        v.reason = f"{len(v.skipped)} case(s) skipped: the runner is set up wrong"
    elif ran != expected:
        v.reason = f"{ran} case(s) ran, expected {expected}"
    elif (code == 1) != bool(v.failed):
        v.reason = f"pytest exit code {code} with {len(v.failed)} failed case(s)"
    elif not v.failed:
        v.status, v.reason = PASS, f"{v.passed} of {expected} passed"
    elif v.failed == [KNOWN]:
        v.status, v.reason = RETRY, f"only the known race `{KNOWN}` failed"
    else:
        v.reason = "a case other than the known race failed: no retry"
    return v


def run(xml, args, select):
    if os.path.exists(xml):
        os.remove(xml)
    cmd = [sys.executable, "-m", "pytest", *args, "-v", "-rA", f"--junitxml={xml}", select]
    code = subprocess.run(cmd).returncode
    try:
        with open(xml, encoding="utf-8") as f:
            text = f.read()
    except OSError:
        return Verdict(FAIL, f"JUnit report {xml} is missing (pytest exit code {code})")
    return verdict(code, text, EXPECTED if select == SUITE else 1)


def summary(text):
    print(text)
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a") as f:
            f.write(text + "\n")


def main():
    system, args = sys.argv[1], sys.argv[2:]
    v = run("canary.xml", args, SUITE)
    summary(f"### Canary first pass, {system}\n\n"
            f"{v.passed} of {EXPECTED} passed, {len(v.failed)} failed, "
            f"{len(v.skipped)} skipped.\n")
    for name in v.failed:
        summary(f"- failed: `{name}`")
    for name, why in v.skipped:
        summary(f"- skipped: `{name}`: {why}")
    summary(f"\nVerdict: {v.status}, {v.reason}.")
    if v.status != RETRY:
        return 0 if v.status == PASS else 1
    node = f"{SUITE}/test_qemu_assumptions.py::{KNOWN}"
    for i in range(1, RETRIES + 1):
        again = run(f"canary-retry{i}.xml", args, node)
        if again.status == PASS:
            summary(f"\nKnown race `{KNOWN}` retried: passed on retry {i} of {RETRIES}.")
            return 0
        if again.status == FAIL:
            summary(f"\nRetry {i} failed outside the known race: {again.reason}.")
            return 1
    summary(f"\nKnown race `{KNOWN}` failed 3 times in a row.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
