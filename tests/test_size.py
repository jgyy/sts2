"""No script grows past 1000 lines: a file that long is several files."""

import unittest
from pathlib import Path

MAX_LINES = 1000
ROOT = Path(__file__).resolve().parent.parent


class FileSize(unittest.TestCase):
    def test_no_script_is_longer_than_the_cap(self):
        scripts = [p for d in ("sts2bot", "tests", "scripts") for p in (ROOT / d).rglob("*") if p.suffix in (".py", ".sh")]
        self.assertTrue(scripts)
        too_long = {str(p.relative_to(ROOT)): n for p in scripts if (n := len(p.read_text().splitlines())) > MAX_LINES}
        self.assertEqual(too_long, {}, f"split these (cap {MAX_LINES} lines)")


if __name__ == "__main__":
    unittest.main()
