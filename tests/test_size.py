"""No script grows past 1000 lines: a file that long is several files. No table lists a key twice.
And no directory holds more than 10 files: past that, the rest go in a new directory."""

import ast
import os
import unittest
from pathlib import Path

MAX_LINES, MAX_FILES = 1000, 10  # (MAX_FILES: a directory's files, subdirectories not counted)
SKIP = {"__pycache__", ".git", ".build", ".pytest_cache", ".ruff_cache", ".venv"}  # generated or not ours
ROOT = Path(__file__).resolve().parent.parent


class FileSize(unittest.TestCase):
    def test_no_script_is_longer_than_the_cap(self):
        scripts = [p for d in ("sts2bot", "sim", "tests", "scripts") for p in (ROOT / d).rglob("*") if p.suffix in (".py", ".sh")]
        self.assertTrue(scripts)
        too_long = {str(p.relative_to(ROOT)): n for p in scripts if (n := len(p.read_text().splitlines())) > MAX_LINES}
        self.assertEqual(too_long, {}, f"split these (cap {MAX_LINES} lines)")

    def test_no_table_lists_a_key_twice(self):
        # A dict's later entry wins without a word: six card ratings reset from the run history once never applied.
        twice = {}
        for path in (ROOT / "sts2bot").rglob("*.py"):
            for node in ast.walk(ast.parse(path.read_text())):
                keys = [k.value for k in node.keys if isinstance(k, ast.Constant)] if isinstance(node, ast.Dict) else []
                if again := sorted({str(k) for k in keys if keys.count(k) > 1}):
                    twice[f"{path.relative_to(ROOT)}:{node.lineno}"] = again
        self.assertEqual(twice, {})

    def test_no_directory_holds_more_than_ten_files(self):
        crowded = {}
        for d, dirs, files in os.walk(ROOT):
            dirs[:] = [x for x in dirs if x not in SKIP]
            if len(files) > MAX_FILES:
                crowded[str(Path(d).relative_to(ROOT))] = len(files)
        self.assertEqual(crowded, {}, f"move the rest into a new directory (cap {MAX_FILES} files)")
