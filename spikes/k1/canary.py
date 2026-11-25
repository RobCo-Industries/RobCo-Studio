"""Run RobCo-Core's canary once. No retry.

Usage: python canary.py <system> [extra pytest args...]

The one pass is the result. It goes to the job summary. The job fails
on any failed case, an error at suite or case level, a skipped case, a
case count other than EXPECTED, a pytest exit code other than 0, or a
report that is missing, empty or not XML.

verdict() makes the decision and runs nothing, so test_canary.py can
test it without QEMU.
"""

import os
import subprocess
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field

SUITE = "robco-core/tests/canary"
# Cases in tests/canary/test_qemu_assumptions.py at the RobCo-Core
# commit pinned as ROBCO_CORE_SHA in .github/workflows/spike-k1.yml
# (c6b6482). Change both together.
EXPECTED = 78

PASS, FAIL = "pass", "fail"


@dataclass
class Verdict:
    status: str
    reason: str
    passed: int = 0
    failed: list = field(default_factory=list)
    skipped: list = field(default_factory=list)


def verdict(code, xml_text, expected=EXPECTED):
    """Decide from pytest's exit code and the JUnit XML text.

    PASS: every expected case ran and passed. FAIL: anything else.
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
        v.reason = f"{len(errors)} error(s): " + ", ".join(errors)
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
    else:
        v.reason = f"{len(v.failed)} case(s) failed"
    return v


def run(xml, args):
    if os.path.exists(xml):
        os.remove(xml)
    cmd = [sys.executable, "-m", "pytest", *args, "-v", "-rA", f"--junitxml={xml}", SUITE]
    code = subprocess.run(cmd).returncode
    try:
        with open(xml, encoding="utf-8") as f:
            text = f.read()
    except OSError:
        return Verdict(FAIL, f"JUnit report {xml} is missing (pytest exit code {code})")
    return verdict(code, text)


def summary(text):
    print(text)
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a") as f:
            f.write(text + "\n")


def main():
    system, args = sys.argv[1], sys.argv[2:]
    v = run("canary.xml", args)
    summary(f"### Canary, {system}\n\n"
            f"{v.passed} of {EXPECTED} passed, {len(v.failed)} failed, "
            f"{len(v.skipped)} skipped.\n")
    for name in v.failed:
        summary(f"- failed: `{name}`")
    for name, why in v.skipped:
        summary(f"- skipped: `{name}`: {why}")
    summary(f"\nVerdict: {v.status}, {v.reason}.")
    return 0 if v.status == PASS else 1


if __name__ == "__main__":
    sys.exit(main())
