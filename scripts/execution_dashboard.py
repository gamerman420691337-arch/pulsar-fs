#!/usr/bin/env python3
"""Validate and render Pulsar's execution ledger. No build or device dispatch."""
import argparse
from datetime import date
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DATA = "docs/PULSAR_EXECUTION.json"
VIEWS = ("docs/PULSAR_MILESTONES.md", "docs/PULSAR_CURRENT_STATUS.md")
SHA = re.compile(r"[0-9a-f]{64}\Z")


class InvalidLedger(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise InvalidLedger(message)


def repo_path(root, value):
    require(isinstance(value, str) and not Path(value).is_absolute(), "repository path required")
    path = (root / value).resolve()
    require(path.is_relative_to(root.resolve()), "repository path escapes checkout")
    return path


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def indexed(rows, group):
    require(isinstance(rows, list), f"{group} records must be a list")
    result = {}
    for row in rows:
        require(isinstance(row, dict), f"{group} record must be an object")
        key = row.get("id")
        require(isinstance(key, str) and re.fullmatch(r"[A-Z][A-Z0-9-]+", key), f"invalid {group} ID")
        require(key not in result, f"duplicate {group} ID: {key}")
        result[key] = row
    return result


def archived_gates(root, archive):
    text = repo_path(root, archive["gate_ledger"]).read_text(encoding="utf-8")
    marker = "# Pulsar milestone gate ledger\n"
    require(marker in text, "original gate-ledger snapshot absent")
    original = text[text.index(marker):]
    require(hashlib.sha256(original.encode()).hexdigest() == archive["original_gate_ledger_sha256"],
            "original gate-ledger snapshot changed")
    result = {}
    for line in original.splitlines():
        if line.startswith("| M"):
            columns = [value.strip() for value in line.strip("|").split("|")]
            if re.fullmatch(r"M[1-8]-[A-Z]+", columns[0]):
                result[columns[0]] = columns[1]
    require(len(result) == 25, "expected all 25 original acceptance gates")
    return result


def checkpoint_metrics(data):
    evidence = indexed(data["evidence"], "evidence")
    result = dict(data["last_verified"])
    expected = {"engine_passed", "engine_ignored", "clients_passed", "clients_ignored", "protocol_passed"}
    require(set(result["metric_refs"]) == expected, "checkpoint metric references differ")
    for name, reference in result["metric_refs"].items():
        require(reference["evidence"] in result["evidence"], "metric refers outside its checkpoint")
        assertions = evidence[reference["evidence"]]["assertions"]
        values = [item["equals"] for item in assertions if item["pointer"] == reference["pointer"]]
        require(len(values) == 1 and type(values[0]) is int and values[0] >= 0,
                f"checkpoint metric lacks an exact integer evidence assertion: {name}")
        result[name] = values[0]
    return result


def validate(data, root=ROOT):
    require(isinstance(data, dict), "execution ledger must be an object")
    require(data.get("schema_version") == 1, "unsupported execution-ledger schema")
    require(re.fullmatch(r"\d{4}-\d{2}-\d{2}", data.get("reconciled_on", "")), "reconciliation date required")
    date.fromisoformat(data["reconciled_on"])
    require(data["runner_state"] in {"active", "paused", "blocked", "unknown"}, "invalid runner observation")
    milestones = indexed(data["milestones"], "milestone")
    require(set(milestones) == {f"M{n}" for n in range(1, 9)}, "M1–M8 scope must remain present")
    gates = indexed(data["gates"], "gate")
    original = archived_gates(root, data["archive"])
    archive_text = repo_path(root, data["archive"]["gate_ledger"]).read_text(encoding="utf-8")
    start = archive_text.index("## Cross-milestone qualification obligations\n") + len("## Cross-milestone qualification obligations\n")
    end = archive_text.index("\n\nHistorical scoped Stash evidence:", start)
    require(data["cross_milestone_obligations"] == archive_text[start:end].strip(),
            "cross-milestone acceptance obligations changed")
    require(original.keys() <= gates.keys(), "an original milestone gate was removed")
    for key, criterion in original.items():
        require(gates[key]["criterion"] == criterion, f"original acceptance criterion changed: {key}")
    evidence = indexed(data["evidence"], "evidence")
    checkpoint_metrics(data)
    for item in evidence.values():
        require(SHA.fullmatch(item.get("sha256", "")), f"missing evidence digest: {item['id']}")
        require(item["kind"] in {"json", "cargo_jsonl"}, "unsupported evidence kind")
        require(item["outcome"] in {"pass", "partial", "fail", "unknown"}, "invalid evidence outcome")
        require(item["freshness"] == "pinned_checkpoint" and item["scope"], "evidence scope/freshness required")
        require(item["assertions"], "evidence assertions required")
    for gate in gates.values():
        require(gate["milestone"] in milestones and gate["id"].startswith(gate["milestone"] + "-"), "gate milestone mismatch")
        require(gate["implementation"] in {"partial", "in_progress", "implemented", "unknown", "not_implemented"}, "invalid implementation state")
        require(gate["verification"] in {"failed_current", "checkpoint_partial", "not_requalified", "awaiting_external", "verified"}, "invalid verification state")
        require(gate["acceptance"] in {"open", "closed"}, "invalid gate acceptance state")
        require(gate["next_proof"], "each gate needs its remaining proof")
        require(set(gate["evidence"]) <= evidence.keys(), "unknown gate evidence")
        for name in gate["source_refs"]:
            require(repo_path(root, name).is_file(), f"gate source/reference absent: {name}")
        if gate["acceptance"] == "closed":
            closure = gate.get("closure", {})
            require(closure.get("scope") and closure.get("evidence"), "gate closure needs explicit criterion-specific evidence")
            require(set(closure["evidence"]) <= evidence.keys(), "unknown closure evidence")
            require(all(evidence[key]["outcome"] in {"pass", "partial"} for key in closure["evidence"]),
                    "failed evidence cannot close a gate")
            require(gate["verification"] == "verified", "unverified gate cannot close")
    tasks = indexed(data["queue"], "task")
    states = {"in_progress", "ready", "planned", "open", "awaiting_external", "verified"}
    for task in tasks.values():
        require(task["state"] in states and task["owner"] and task["exit"], "task state/owner/exit proof required")
        require(task["parents"] and set(task["parents"]) <= gates.keys(), "task must map to existing gates")
        require(set(task["evidence"]) <= evidence.keys(), "unknown task evidence")
        require(set(task["depends_on"]) <= tasks.keys() and task["id"] not in task["depends_on"], "invalid task dependency")
        if task["state"] == "ready":
            require(all(tasks[key]["state"] == "verified" for key in task["depends_on"]), "ready task has unverified dependencies")
        if task["state"] == "verified":
            require(task["evidence"] and all(evidence[key]["outcome"] in {"pass", "partial"} for key in task["evidence"]), "verified task lacks scoped successful evidence")
    active, visited = set(), set()

    def visit(key):
        require(key not in active, "execution queue contains a dependency cycle")
        if key in visited:
            return
        active.add(key)
        for parent in tasks[key]["depends_on"]:
            visit(parent)
        active.remove(key)
        visited.add(key)

    for key in tasks:
        visit(key)
    queued_gates = {key for task in tasks.values() for key in task["parents"]}
    require({gate["id"] for gate in gates.values() if gate["acceptance"] == "open"} <= queued_gates,
            "an open gate has no queued next work")
    for item in indexed(data["completed_work"], "completion").values():
        require(item["parents"] and set(item["parents"]) <= gates.keys(), "completion lacks parent gates")
        require(item["scope"] and item["evidence"], "scoped completion needs scope and evidence")
        require(set(item["evidence"]) <= evidence.keys(), "unknown completion evidence")
        require(all(evidence[key]["outcome"] in {"pass", "partial"} for key in item["evidence"]), "failed evidence cannot support a completion")
    tree = data["working_tree"]
    require(tree["active_work"] in tasks, "active task missing")
    require(tree["build_state"] in {"failed", "verified", "unverified"}, "invalid current build state")
    require(tree["evidence"] in evidence, "current build evidence missing")
    if tree["build_state"] != "unverified":
        expected = "fail" if tree["build_state"] == "failed" else "pass"
        require(evidence[tree["evidence"]]["outcome"] == expected, "build state contradicts its evidence")
        require(tree["source_stamp"], "current build claim requires source identities")
        for source in tree["source_stamp"]:
            path = repo_path(root, source["path"])
            require(path.is_file() and digest(path) == source["sha256"],
                    f"current build observation is stale: {source['path']}; mark build unverified before editing, then attach fresh evidence")
    for entry in data["authority"]:
        require(repo_path(root, entry["path"]).is_file(), "authority document absent")
    require(data["full_roadmap"] in {"incomplete", "complete"}, "invalid roadmap state")
    if data["full_roadmap"] == "complete":
        require(all(gate["acceptance"] == "closed" for gate in gates.values()) and data.get("release_acceptance"),
                "full roadmap completion needs every gate and genuine release acceptance")


def cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def label(value):
    return value.replace("_", " ")


def render(data):
    evidence = {item["id"]: item for item in data["evidence"]}

    def refs(ids):
        return ", ".join(f"[{key}]({evidence[key]['path']})" for key in ids) or "Not requalified"

    generated = "> GENERATED from [PULSAR_EXECUTION.json](PULSAR_EXECUTION.json). Edit the JSON; run `python3 scripts/execution_dashboard.py --write` from the repository root.\n"
    tree, last = data["working_tree"], checkpoint_metrics(data)
    task = next(item for item in data["queue"] if item["id"] == tree["active_work"])
    lines = ["# Pulsar execution dashboard", "", generated,
             f"Reconciled **{data['reconciled_on']} ({data['timezone']})**. Full roadmap: **{data['full_roadmap'].upper()}**. Autonomous runner observation: **{data['runner_state']}**.", "",
             "## Now", "", f"**Current working tree: {tree['build_state'].upper()}.** {tree['failure_summary']}", "",
             f"{tree['scope']} Evidence: {refs([tree['evidence']])}.", "",
             f"**First action — {task['id']}: {task['title']}.** Exit required: {task['exit']}", "",
             "The queue drives execution; specifications and owner decisions retain authority. Scoped completions below do not close their parent acceptance gates.", "",
             "## Implemented application surface", "", data["product_surface"], "",
             "## Last verified baseline", "",
             f"**Engine {last['engine_passed']} passed / {last['engine_ignored']} ignored; clients {last['clients_passed']} passed / {last['clients_ignored']} ignored; Protocol {last['protocol_passed']} passed.** {last['scope']}", "",
             refs(last["evidence"]) + ". Ignored native/qualification scenarios are separate. The earlier startup timeout and long Engine-open delay remain causally unresolved.", "",
             "## Done within the recorded software scope", "",
             "| Work | Verified scope and limit | Parent gates | Evidence |", "| --- | --- | --- | --- |"]
    for item in data["completed_work"]:
        lines.append(f"| {cell(item['title'])} | {cell(item['scope'])} | {', '.join(item['parents'])} | {refs(item['evidence'])} |")
    lines += ["", "## Execution queue", "", "Order is priority, with explicit dependencies. External preparation can proceed independently. “In progress” identifies unfinished work, not a confirmed live process.", "",
              "| Order / ID | Work and owner | State / prerequisites | Required exit evidence |", "| --- | --- | --- | --- |"]
    for number, item in enumerate(data["queue"], 1):
        dependencies = ", ".join(item["depends_on"]) or "None"
        lines.append(f"| {number}. **{item['id']}** | {cell(item['title'])}<br>{cell(item['owner'])} | {label(item['state'])}<br>Depends: {dependencies} | {cell(item['exit'])} |")
    closed = sum(gate["acceptance"] == "closed" for gate in data["gates"])
    lines += ["", "## M1–M8 acceptance ledger", "", f"All 25 original criteria are retained verbatim. **{closed} of {len(data['gates'])} acceptance gates closed.** Implementation state, evidence state and acceptance are independent; **not requalified** means this reconciliation did not establish current acceptance.", ""]
    for milestone in data["milestones"]:
        lines += [f"### {milestone['id']}: {milestone['title']}", "", "| Gate / acceptance | Required result | Implementation / verification | Next proof and basis |", "| --- | --- | --- | --- |"]
        for gate in (g for g in data["gates"] if g["milestone"] == milestone["id"]):
            source = ", ".join(f"[{Path(p).name}](../{p})" for p in gate["source_refs"])
            lines.append(f"| **{gate['id']}** / {gate['acceptance']} | {cell(gate['criterion'])} | {label(gate['implementation'])}<br>{label(gate['verification'])} | {cell(gate['next_proof'])}<br>{refs(gate['evidence'])}; source: {source} |")
        lines.append("")
    lines += ["## Cross-milestone acceptance obligations", "", data["cross_milestone_obligations"], "",
              "## External inputs and acceptance", ""]
    lines += [f"- **{item['item']}:** {item['needed']}" for item in data["external_inputs"]]
    lines += ["", "## Evidence registry", "", "Local retained artifacts may be unavailable on another machine. Outcome and scope remain pinned; run `--audit` on the evidence host to verify hashes and assertions. This is not a runtime qualification or an automatic acceptance mechanism.", "",
              "| ID / outcome | Recorded scope | SHA-256 |", "| --- | --- | --- |"]
    for item in data["evidence"]:
        lines.append(f"| [{item['id']}]({item['path']}) / {item['outcome']} | {cell(item['scope'])} | `{item['sha256'][:16]}…` |")
    lines += ["", "## How this drives the remaining work", ""]
    lines += [f"{number}. {rule}" for number, rule in enumerate(data["execution_rules"], 1)]
    lines += ["", "```sh", "python3 scripts/execution_dashboard.py --write", "python3 scripts/execution_dashboard.py --check", "python3 scripts/execution_dashboard.py --audit", "```", "",
              "`--check` validates preserved gates, task dependencies, current-source claims and generated views. CI runs that check. `--audit` additionally checks retained evidence hashes and recorded assertions; it does not rerun builds, benchmarks or hardware tests.", "",
              "## Authority and preserved history", ""]
    lines += [f"- [{Path(item['path']).name}](../{item['path']}): {item['role']}." for item in data["authority"]]
    lines += [f"- [Previous gate ledger]({Path(data['archive']['gate_ledger']).name}) and [previous status chronology]({Path(data['archive']['status_history']).name}): preserved historical records, superseded for current execution state.", "", data["audit_boundary"], ""]
    dashboard = "\n".join(lines)
    status = "\n".join(["# Pulsar current status", "", generated,
        "The [execution dashboard](PULSAR_MILESTONES.md) is the single current work queue and milestone ledger.", "",
        f"Reconciled **{data['reconciled_on']}**. Roadmap **{data['full_roadmap'].upper()}**; autonomous runner observation **{data['runner_state']}**.", "",
        f"**Working tree: {tree['build_state'].upper()}.** {tree['failure_summary']}", "",
        f"**Next: {task['id']} — {task['title']}.** Exit required: {task['exit']}", "",
        f"Last verified checkpoint: Engine **{last['engine_passed']} passed / {last['engine_ignored']} ignored**, clients **{last['clients_passed']} passed / {last['clients_ignored']} ignored**, Protocol **{last['protocol_passed']} passed**. {last['scope']}", "",
        "Native core/browser/restart/resource scenarios have scoped passing evidence. Real rendered acquisition, useful learned quality, installed-Stash compatibility, physical Handy synchronization and full platform/human/release acceptance remain open. The earlier startup failure remains unexplained.", "",
        f"Historical detail is retained in [{Path(data['archive']['status_history']).name}]({Path(data['archive']['status_history']).name}). Update `PULSAR_EXECUTION.json`, regenerate the views, and keep evidence scoped to its original subjects.", ""])
    return dict(zip(VIEWS, (dashboard, status)))


def check_views(views, root=ROOT):
    for name, expected in views.items():
        path = repo_path(root, name)
        require(path.is_file() and path.read_text(encoding="utf-8") == expected,
                f"stale generated view: {name}; run --write")


def audit(data):
    checked = []
    for item in data["evidence"]:
        path = Path(item["path"])
        require(path.is_file(), f"evidence unavailable on this host: {item['id']}")
        require(path.stat().st_size <= 64 * 1024 * 1024, "evidence exceeds audit read bound")
        require(digest(path) == item["sha256"], f"evidence hash differs: {item['id']}")
        if item["kind"] == "cargo_jsonl":
            messages = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.startswith("{")]
            finished = [m for m in messages if m.get("reason") == "build-finished"]
            errors = [m["message"] for m in messages if m.get("reason") == "compiler-message" and m["message"].get("level") == "error"]
            require(len(finished) == 1, "one terminal Cargo result required")
            value = {"success": finished[0]["success"], "error_count": len(errors),
                     "codes": sorted((e.get("code") or {}).get("code", "") for e in errors)}
        else:
            value = json.loads(path.read_text(encoding="utf-8"))
        for assertion in item["assertions"]:
            observed = value
            for key in assertion["pointer"]:
                require(isinstance(observed, dict) and key in observed, f"evidence selector missing: {item['id']}")
                observed = observed[key]
            require(type(observed) is type(assertion["equals"]) and observed == assertion["equals"],
                    f"evidence assertion differs: {item['id']} {assertion['pointer']}")
        checked.append(item["id"])
    return checked


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--audit", action="store_true")
    args = parser.parse_args()
    try:
        data = json.loads((ROOT / DATA).read_text(encoding="utf-8"))
        validate(data)
        views = render(data)
        if args.write:
            for name, value in views.items():
                target = repo_path(ROOT, name)
                temporary = None
                try:
                    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=target.parent,
                                                     prefix=".execution-dashboard-", delete=False) as stream:
                        temporary = Path(stream.name)
                        stream.write(value)
                    temporary.chmod(0o644)
                    temporary.replace(target)
                finally:
                    if temporary is not None:
                        temporary.unlink(missing_ok=True)
        check_views(views)
        result = {"status": "PASS", "mode": "write" if args.write else "audit" if args.audit else "check",
                  "gates": len(data["gates"]), "tasks": len(data["queue"]), "full_roadmap": data["full_roadmap"]}
        if args.audit:
            result["evidence_verified"] = audit(data)
        print(json.dumps(result))
        return 0
    except (InvalidLedger, OSError, ValueError, KeyError, TypeError) as error:
        print(f"execution dashboard: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
