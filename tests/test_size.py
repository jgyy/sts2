"""No script grows past 1000 lines: a file that long is several files. And no table lists a key twice."""

import ast
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

    def test_no_table_lists_a_key_twice(self):
        # The later entry of a dict wins without a word: six card ratings reset from the
        # run history never applied, because the old ones stood again further down the table.
        twice = {}
        for path in (ROOT / "sts2bot").glob("*.py"):
            for node in ast.walk(ast.parse(path.read_text())):
                if isinstance(node, ast.Dict):
                    keys = [k.value for k in node.keys if isinstance(k, ast.Constant)]
                    again = sorted({str(k) for k in keys if keys.count(k) > 1})
                    if again:
                        twice[f"{path.name}:{node.lineno}"] = again
        self.assertEqual(twice, {})


if __name__ == "__main__":
    unittest.main()
