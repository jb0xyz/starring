import pathlib
import unittest


README_PATH = pathlib.Path(__file__).with_name("README.md")


def lifecycle_example():
    text = README_PATH.read_text(encoding="utf-8")
    heading = text.index("Then run the substrate lifecycle with the immutable manifest:")
    start = text.index("```text", heading) + len("```text")
    end = text.index("```", start)
    return text[start:end]


def browser_certification_section():
    text = README_PATH.read_text(encoding="utf-8")
    start = text.index("## Certified browser steps 5-7")
    end = text.index("When an update must drain", start)
    return text[start:end]


def candidate_preparation_section():
    text = README_PATH.read_text(encoding="utf-8")
    start = text.index("Example preparation shape:")
    end = text.index("Then run the substrate lifecycle", start)
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

    def test_certified_browser_flow_crosses_step_six_completion(self):
        section = browser_certification_section()
        markers = (
            "--checkpoint before",
            "const authoring = await product.beginCertificationAuthoring",
            "JSON.stringify(authoring.authoring_evidence)",
            "JSON.stringify(authoring.preview_ready_evidence)",
            "--checkpoint after",
            "--browser-evidence /absolute/step-05-browser-authoring.json",
            '"$BUNDLE/starring-d2-sealed-provisioner" inspect',
            "--checkpoint authoring",
            "--step 5",
            "--step 6",
            "d2_run.py\" status",
            "product.createCertificationDecisionCommand",
            "previewCompletionChallengeSha256:",
            "<preview_completion_challenge_sha256 from status>",
            "product.decisionCommandSha256",
            "product.completeCertificationDecision",
            "JSON.stringify(decision.product_decision_evidence)",
            "--step 7",
        )
        positions = [section.index(marker) for marker in markers]
        self.assertEqual(positions, sorted(positions))
        for source in (
            "--source /absolute/step-05-browser-authoring.json",
            '--source "$ORCH/worker-authoring/evidence.json"',
            "--source /absolute/step-06-browser-preview-ready.json",
            "--source /absolute/step-06-db-authoring.json",
            "--source /absolute/step-07-browser-product-decision.json",
        ):
            self.assertIn(source, section)
        self.assertIn(
            'isolated_orchestrator.py" worker-authoring-evidence \\\n'
            '  --manifest "$MANIFEST" \\\n'
            "  --checkpoint before",
            section,
        )
        self.assertIn(
            'isolated_orchestrator.py" worker-authoring-evidence \\\n'
            '  --manifest "$MANIFEST" \\\n'
            "  --checkpoint after \\\n"
            "  --browser-evidence /absolute/step-05-browser-authoring.json",
            section,
        )
        self.assertIn(
            '"$BUNDLE/starring-d2-sealed-provisioner" inspect \\\n'
            '  --manifest "$MANIFEST" \\\n'
            "  --checkpoint authoring",
            section,
        )
        self.assertIn(
            'previewCompletionChallengeSha256:\n'
            '    "<preview_completion_challenge_sha256 from status>",',
            section,
        )
        self.assertIn(
            "const decision = await product.completeCertificationDecision({\n"
            "  command: decisionCommand,\n"
            "  decisionCommandSha256,\n"
            "})",
            section,
        )
        self.assertIn("mode-`0600` JSON files", section)
        self.assertNotIn("confirmPreview:", section)
        self.assertNotIn("JSON.stringify(authoring)", section)
        self.assertNotIn("JSON.stringify(decision)", section)

    def test_run_one_shot_is_not_the_initial_certification_flow(self):
        section = browser_certification_section()
        self.assertIn("non-certification convenience", section)
        self.assertIn("certified steps 5-7", section)
        self.assertIn("step 14 replacement", section)
        self.assertNotIn("runOneShotProductFlow(", section)
        self.assertNotIn(
            "productEvidence = await product.runOneShotProductFlow", section
        )

    def test_d3_state_path_and_run_directory_are_distinct(self):
        section = candidate_preparation_section()
        self.assertIn("D3_STATE=/absolute/d3/output-root/run-id/state.json", section)
        self.assertIn('D3_RUN="$(dirname "$D3_STATE")"', section)
        self.assertIn('BUNDLE="$D3_RUN/candidate-bundle"', section)
        self.assertIn(
            'D2_TOOLCHAIN="$D3_RUN/worktree/tools/d2-certification"', section
        )
        self.assertIn(
            'CANDIDATE_COMMIT="$(jq -er \'.merge_commit\' "$D3_STATE")"', section
        )
        self.assertNotIn('BUNDLE="$D3_STATE/candidate-bundle"', section)

    def test_browser_evidence_does_not_require_devtools_copy(self):
        text = README_PATH.read_text(encoding="utf-8")
        self.assertNotIn("copy(JSON.stringify", text)
        section = browser_certification_section()
        self.assertIn("does not depend on", section)
        self.assertGreaterEqual(section.count("window.prompt("), 3)


if __name__ == "__main__":
    unittest.main()
