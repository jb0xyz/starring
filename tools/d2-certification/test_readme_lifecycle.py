import pathlib
import unittest


README_PATH = pathlib.Path(__file__).with_name("README.md")


def lifecycle_example():
    text = README_PATH.read_text(encoding="utf-8")
    heading = text.index("Then run the substrate lifecycle with the immutable manifest:")
    start = text.index("```text", heading) + len("```text")
    end = text.index("```", start)
    return text[start:end]


class ReadmeLifecycleTests(unittest.TestCase):
    def test_prior_absence_is_captured_before_prepare(self):
        example = lifecycle_example()
        dry_run = example.index("isolated_orchestrator.py\" dry-run")
        preflight = example.index("d2_preflight_evidence.py")
        prepare = example.index("isolated_orchestrator.py\" prepare")
        self.assertLess(dry_run, preflight)
        self.assertLess(preflight, prepare)

    def test_step_11_example_does_not_restart_twice(self):
        example = lifecycle_example()
        self.assertEqual(example.count("certify-live-runtime-restart"), 2)
        self.assertNotIn("isolated_orchestrator.py\" restart-drained-runtime", example)


if __name__ == "__main__":
    unittest.main()
