"""Reference input regressions for the zero-offset browser scorer."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "sterile_browser_score", Path(__file__).with_name("score-sterile-browser-recording.py")
)
SCORE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SCORE)


class ReferenceTests(unittest.TestCase):
    def write_reference(self, root, actions, csv):
        for suffix in ("funscript", "json"):
            (root / f"353079142771.{suffix}").write_text(
                json.dumps({"actions": actions})
            )
        (root / "353079142771.csv").write_text(csv)

    def test_reference_validation_preserves_valid_points_and_refuses_malformed_input(self):
        valid = [{"at": 0, "pos": 0}, {"at": 100, "pos": 100}]
        cases = (
            (valid, "0,0,9\n100,100\n", "exactly two columns"),
            ([valid[0], {"at": 100, "pos": 101}], "0,0\n100,101\n", "positions from 0 to 100"),
            ([{"at": False, "pos": 0}, valid[1]], "0,0\n100,100\n", "nonnegative integer times"),
            ([valid[0], {"at": 15000, "pos": 100}], "0,0\n15000,100\n", "two actions within the clip"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.write_reference(root, valid, "0,0\n100,100\n")
            self.assertEqual(SCORE.read_reference(root), valid)
            for actions, csv, error in cases:
                with self.subTest(error=error):
                    self.write_reference(root, actions, csv)
                    with self.assertRaisesRegex(ValueError, error):
                        SCORE.read_reference(root)
