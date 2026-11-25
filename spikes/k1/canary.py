"""Run RobCo-Core's canary once, and retry only one known racy case.

Usage: python canary.py <system> [extra pytest args...]

The first pass is the result that counts. It goes to the job summary
with the names of the failed cases. If the only failed case is the known
race, that case alone is rerun up to two more times. Any other failure
fails at once.
"""

import os
import subprocess
import sys
import xml.etree.ElementTree as ET

KNOWN = "test_v23_spi0_byte_rate[raspi3b]"
SUITE = "robco-core/tests/canary"
RETRIES = 2


def run(xml, args, select=None):
    cmd = [sys.executable, "-m", "pytest", *args, "-v", "-rA", f"--junitxml={xml}"]
    cmd += [select] if select else [SUITE]
    subprocess.run(cmd)
    cases = ET.parse(xml).getroot().iter("testcase")
    passed, failed = 0, []
    for c in cases:
        if c.find("skipped") is not None:
            continue
        if c.find("failure") is not None or c.find("error") is not None:
            failed.append(c.get("name"))
        else:
            passed += 1
    return passed, failed


def summary(text):
    print(text)
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a") as f:
            f.write(text + "\n")


def main():
    system, args = sys.argv[1], sys.argv[2:]
    passed, failed = run("canary.xml", args)
    total = passed + len(failed)
    summary(f"### Canary first pass, {system}\n\n"
            f"{passed} of {total} passed, {len(failed)} failed.\n")
    for name in failed:
        summary(f"- failed: `{name}`")
    if not failed:
        return 0
    if failed != [KNOWN]:
        summary("\nA case other than the known race failed: no retry.")
        return 1
    node = f"{SUITE}/test_qemu_assumptions.py::{KNOWN}"
    for i in range(1, RETRIES + 1):
        _, again = run(f"canary-retry{i}.xml", args, node)
        if not again:
            summary(f"\nKnown race `{KNOWN}` retried: passed on retry {i} of {RETRIES}.")
            return 0
    summary(f"\nKnown race `{KNOWN}` failed 3 times in a row.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
