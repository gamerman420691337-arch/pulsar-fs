"""Regression controls for misleading execution status and lost requirements."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import execution_dashboard as dashboard


class DashboardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads((dashboard.ROOT / dashboard.DATA).read_text(encoding="utf-8"))

    def changed(self):
        return copy.deepcopy(self.data)

    def test_reconciled_ledger_and_both_generated_views(self):
        dashboard.validate(self.data)
        views = dashboard.render(self.data)
        self.assertEqual(len(dashboard.archived_gates(dashboard.ROOT, self.data["archive"])), 25)
        self.assertGreaterEqual(len(self.data["gates"]), 25)
        self.assertIn("Current working tree: " + self.data["working_tree"]["build_state"].upper(), views[dashboard.VIEWS[0]])
        self.assertIn("Last verified baseline", views[dashboard.VIEWS[0]])
        self.assertIn(self.data["working_tree"]["active_work"], views[dashboard.VIEWS[1]])

    def test_original_gate_cannot_disappear_or_change_criterion(self):
        data = self.changed()
        data["gates"].pop()
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)
        data = self.changed()
        data["gates"][0]["criterion"] = "Reduced acceptance requirement"
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_failed_build_cannot_be_relabelled_verified(self):
        data = self.changed()
        data["working_tree"]["build_state"] = "verified"
        next(e for e in data["evidence"] if e["id"] == data["working_tree"]["evidence"])["outcome"] = "fail"
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_cross_milestone_obligations_remain_required(self):
        data = self.changed()
        data["cross_milestone_obligations"] = ""
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_checkpoint_numbers_require_matching_typed_evidence(self):
        data = self.changed()
        data["last_verified"]["metric_refs"]["engine_passed"]["pointer"] = ["qualified"]
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_changed_source_invalidates_current_build_claim(self):
        data = self.changed()
        path = data["authority"][0]["path"]
        data["working_tree"]["source_stamp"] = [{"path": path, "sha256": "0" * 64}]
        data["working_tree"]["build_state"] = "failed"
        record = copy.deepcopy(data["evidence"][0])
        record.update(id="TEST-BUILD-FAIL", outcome="fail")
        data["evidence"].append(record)
        data["working_tree"]["evidence"] = record["id"]
        with self.assertRaisesRegex(dashboard.InvalidLedger, "stale"):
            dashboard.validate(data)
        data["working_tree"]["build_state"] = "unverified"
        dashboard.validate(data)

    def test_failed_evidence_cannot_support_scoped_completion(self):
        data = self.changed()
        evidence_id = data["completed_work"][0]["evidence"][0]
        next(e for e in data["evidence"] if e["id"] == evidence_id)["outcome"] = "fail"
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_dependency_cycle_and_premature_ready_are_rejected(self):
        data = self.changed()
        first, second = data["queue"][:2]
        first["depends_on"] = [second["id"]]
        second["depends_on"] = [first["id"]]
        for task in data["queue"]:
            task["state"] = "planned"
        with self.assertRaisesRegex(dashboard.InvalidLedger, "cycle"):
            dashboard.validate(data)
        data = self.changed()
        data["queue"][0]["state"] = "in_progress"
        data["queue"][1]["depends_on"] = [data["queue"][0]["id"]]
        data["queue"][1]["state"] = "ready"
        with self.assertRaisesRegex(dashboard.InvalidLedger, "dependencies"):
            dashboard.validate(data)

    def test_missing_evidence_and_unproven_roadmap_completion_are_rejected(self):
        data = self.changed()
        data["queue"][0]["evidence"] = ["UNKNOWN"]
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)
        data = self.changed()
        data["full_roadmap"] = "complete"
        data["gates"][0]["acceptance"] = "open"
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_closed_gate_requires_explicit_criterion_evidence(self):
        data = self.changed()
        data["gates"][0]["acceptance"] = "closed"
        data["gates"][0].pop("closure", None)
        with self.assertRaises(dashboard.InvalidLedger):
            dashboard.validate(data)

    def test_open_acceptance_gate_cannot_lose_all_queued_work(self):
        data = self.changed()
        next(gate for gate in data["gates"] if gate["id"] == "M8-COMPARISON")["acceptance"] = "open"
        for task in data["queue"]:
            task["parents"] = [gate for gate in task["parents"] if gate != "M8-COMPARISON"]
        with self.assertRaisesRegex(dashboard.InvalidLedger, "no queued"):
            dashboard.validate(data)

    def test_stale_view_and_corrupted_evidence_fail_with_owned_fixture(self):
        base = Path("/mnt/4tb/tmp")
        with tempfile.TemporaryDirectory(prefix="pulsar-dashboard-", dir=base if base.is_dir() else None) as name:
            root = Path(name)
            view = root / "view.md"
            view.write_text("expected\n")
            dashboard.check_views({"view.md": "expected\n"}, root)
            view.write_text("stale\n")
            with self.assertRaisesRegex(dashboard.InvalidLedger, "stale"):
                dashboard.check_views({"view.md": "expected\n"}, root)
            evidence = root / "neutral.json"
            evidence.write_text('{"passed": true}\n')
            record = {"id": "FIXTURE", "path": str(evidence),
                      "sha256": hashlib.sha256(evidence.read_bytes()).hexdigest(), "kind": "json",
                      "assertions": [{"pointer": ["passed"], "equals": True}]}
            self.assertEqual(dashboard.audit({"evidence": [record]}), ["FIXTURE"])
            evidence.write_text('{"passed": false}\n')
            with self.assertRaisesRegex(dashboard.InvalidLedger, "hash differs"):
                dashboard.audit({"evidence": [record]})
        self.assertFalse(root.exists(), "owned dashboard test scratch must be removed")


if __name__ == "__main__":
    unittest.main()
