#!/usr/bin/env python3
"""Summarize actual histories and pairwise refinement errors without retuning."""
import json
import gzip
import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QA = ROOT / "docs/qa"
EVIDENCE = QA


def evidence_file(name):
    raw = EVIDENCE / name
    if raw.exists():
        return json.loads(raw.read_text())
    with gzip.open(str(raw) + ".gz", "rt") as stream:
        return json.load(stream)


def summary(run):
    negatives = run["negative_stage_evaluations"]
    samples = run["samples_20hz"]
    fields = ("completed_s", "failure", "minimum_disk_clearance_m",
              "minimum_static_blade_swept_clearance_m", "maximum_gear_compression_m",
              "near_static_braked_seconds_below_0_1mps", "last_braked_10s_peak_speed_mps",
              "last_braked_10s_traveled_distance_m", "stationary_parking_pass",
              "positive_flow_bit_exact_step_count", "observed_evaluations")
    result = {key: run[key] for key in fields}
    result.update(hz=run["hz"], negative_stage_evaluations=len(negatives),
                  final_speed_mps=run["final"]["speed_mps"],
                  final_shaft_rad_s=run["final"]["shaft_rad_s"],
                  final_pitch_rad=run["final"]["pitch_rad"])
    if negatives:
        result.update(minimum_negative_j=min(n["flow"]["signed_j"] for n in negatives),
                      maximum_adverse_ratio=max(n["flow"]["signed_adverse_ratio"] for n in negatives),
                      maximum_transverse_ratio=max(n["flow"]["transverse_ratio"] for n in negatives),
                      negative_stage_time_range_s=[min(n["time_s"] for n in negatives),
                                                   max(n["time_s"] for n in negatives)])
    result["milestones"] = [s for s in samples if s["time_s"] in (0, 4, 4.1, 4.2, 5, 10, 15, 20, 21, 25, 30)]
    return result


def difference(a, b):
    left, right = a["samples_20hz"], b["samples_20hz"]
    assert len(left) == len(right)
    assert all(x["time_s"] == y["time_s"] for x, y in zip(left, right))
    fields = {
        "speed_mps": ("state", "speed_mps"),
        "pitch_rad": ("state", "pitch_rad"),
        "shaft_rad_s": ("state", "shaft_rad_s"),
        "blade_pitch_rad": ("state", "blade_pitch_rad"),
        "altitude_m": ("state", "altitude_m"),
        "signed_j": ("flow", "signed_j"),
        "relative_rpm": ("flow", "relative_rpm"),
        "thrust_n": ("flow", "thrust_n"),
        "disk_clearance_m": ("disk_clearance_m",),
    }

    def value(item, path):
        for key in path:
            item = item[key]
        return item

    result = {"rates_hz": [a["hz"], b["hz"]]}
    for name, path in fields.items():
        result["maximum_history_difference_" + name] = max(
            abs(value(x, path) - value(y, path)) for x, y in zip(left, right))
    for key in ("gear_clearances_m", "contact_force_body_n"):
        result["maximum_history_component_difference_" + key] = max(
            abs(x[key][axis] - y[key][axis]) for x, y in zip(left, right) for axis in range(3))
    result["maximum_history_component_difference_body_air_velocity_mps"] = max(
        abs(x["flow"]["body_air_velocity_mps"][axis] - y["flow"]["body_air_velocity_mps"][axis])
        for x, y in zip(left, right) for axis in range(3))
    result["maximum_history_component_difference_position_ecef_m"] = max(
        abs(x["state"]["position_ecef_m"][axis] - y["state"]["position_ecef_m"][axis])
        for x, y in zip(left, right) for axis in range(3))
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-dir", type=Path, default=QA,
                        help="raw or compressed full histories; default is local regenerated docs/qa")
    args = parser.parse_args()
    EVIDENCE = args.evidence_dir
    evidence = evidence_file("cedar-near-static-critical.json")
    runs = evidence["critical_runs"]
    result = dict(schema=1, critical_runs=[summary(r) for r in runs])
    if len(runs) == 4 and all(r["failure"] is None for r in runs):
        result["pairwise_refinement"] = [difference(a, b) for a, b in zip(runs, runs[1:])]
        result["comparison_to_960hz"] = [difference(r, runs[-1]) for r in runs[:-1]]
    matrix = evidence_file("cedar-near-static-matrix.json")
    result["matrix"] = [dict(case=c["case"], **{key: c["result"][key] for key in (
        "completed_s", "failure", "minimum_static_blade_swept_clearance_m",
        "maximum_gear_compression_m", "positive_flow_bit_exact_step_count")})
        for c in matrix["cases"]]
    destination = QA / "cedar-near-static-summary.json"
    destination.write_text(json.dumps(result, indent=2, allow_nan=False) + "\n")
    print(destination.relative_to(ROOT))
