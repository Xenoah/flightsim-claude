"""Independent identity/polar references and explicit numerical gate analysis.

Usage: python3 analyze.py PROFILE FULL_OUTPUT [SUPPLEMENTARY_OUTPUT]
Requires NumPy for local linear eigenvalues; no simulator code is imported.
The existing independent Python wire encoder is reused, not a Rust digest.
"""
import importlib.util
import gzip
import json
import math
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[3]


def read_json(path):
    """Accept the losslessly compressed retained trace representation too."""
    if path.exists():
        return json.loads(path.read_text())
    return json.loads(gzip.decompress(path.with_suffix(path.suffix + ".gz").read_bytes()))


profile_path, output = map(Path, sys.argv[1:3])
profile = json.loads(profile_path.read_text())
results = json.loads((output / "summary.json").read_text())
supplement = (json.loads((Path(sys.argv[3]) / "supplementary.json").read_text())
              if len(sys.argv) > 3 else {})
spec = importlib.util.spec_from_file_location("identity_reference", ROOT / "docs/qa/jet_identity_reference.py")
identity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(identity)
expected = identity.canonical(profile)
identity_pass = expected == (output / "canonical.bin").read_bytes()
assert identity_pass
assert f"{identity.digest(expected):016x}" == results["identity"]


def modes(pair):
    a, b = (np.array(row["matrix"], dtype=float) for row in pair)
    ea, eb = np.linalg.eigvals(a), np.linalg.eigvals(b)
    # Nearest complex eigenvalue comparison allows conjugate-pair order changes.
    delta = max(min(abs(x-y) for y in eb) for x in ea)
    return dict(eigenvalues_per_s=[[float(e.real), float(e.imag)] for e in ea],
                max_real_per_s=float(max(ea.real)),
                fastest_abs_per_s=float(max(abs(ea))),
                half_step_matrix_max_difference=float(np.max(abs(a-b))),
                half_step_eigenvalue_max_nearest_difference=float(delta),
                no_growth_at_or_above_005_per_s=bool(max(ea.real) < .05),
                qualification="local linearization only; zero translation modes and slow modes retained")


mode_rows = []
trim_rows = []
for row in results["trim_grid"]:
    if not row["trimmed"]:
        trim_rows.append(row)
        continue
    residual = row["residual"]
    held = row["held600"]
    complete = held["steps"] == 72000 and held["terminal"] is None
    trim_rows.append(dict(altitude_m=row["altitude_m"], mach=row["mach"],
        acceleration_residual_max=max(abs(x) for x in residual[:3]),
        angular_residual_max=max(abs(x) for x in residual[3:]),
        trim_pass=max(abs(x) for x in residual) <= 1e-6,
        throttle=row["controls"][3], elevator=row["controls"][0],
        cruise_margin=row["cruise_margin"], held60_pass=row["held60_pass"],
        held600_pass=complete and held["max_altitude_deviation_m"] < 250
            and held["max_speed_deviation_mps"] < 15 and held["max_abs_bank_deg"] < 45,
        held600_altitude_deviation_m=held["max_altitude_deviation_m"],
        held600_speed_deviation_mps=held["max_speed_deviation_mps"]))
    mode_rows.append(dict(altitude_m=row["altitude_m"], mach=row["mach"],
                          at_knot_central_difference=True, **modes(row["jacobian"])))

side_modes = []
for row in supplement.get("one_sided_knot_modes", []):
    if "jacobians" in row:
        side_modes.append(dict(altitude_m=row["altitude_m"], knot=row["knot"],
            side=row["side"], actual_mach=row["actual_mach"], **modes(row["jacobians"])))
    else:
        side_modes.append(row)


def polar(coeff, alpha, flaps):
    # Exponential ratio, independent of Rust's stable logistic product.
    left = math.exp(-coeff["stall_blend_rate"] * (alpha - coeff["stall_angle_rad"]))
    right = math.exp(coeff["stall_blend_rate"] * (alpha + coeff["stall_angle_rad"]))
    sigma = (1 + left + right) / ((1 + left) * (1 + right))
    cl = coeff["lift_zero"] + coeff["lift_alpha"] * alpha + coeff["lift_flaps"] * flaps
    air = profile["dynamics"]["airframe"]
    aspect = air["wing_span_m"] ** 2 / air["wing_area_m2"]
    lift = (1-sigma)*cl + sigma*2*math.copysign(1, alpha)*math.sin(alpha)**2*math.cos(alpha)
    drag = ((1-sigma)*(coeff["drag_min"]+cl*cl/(math.pi*coeff["oswald_efficiency"]*aspect)
             + coeff["drag_flaps"]*flaps) + sigma*2*math.sin(alpha)**2)
    return lift, drag


peak_rows = []
for row in results["stall_peaks"]:
    a = next(k["aero"] for k in profile["dynamics"]["aero"]["knots"] if k["mach"] == row["mach"])
    # Dense independent scan establishes bracket; golden section refines peak.
    xs = np.linspace(0., a["stall_angle_rad"], 2001)
    vals = [polar(a, x, row["flaps"])[0] for x in xs]
    i = int(np.argmax(vals))
    lo, hi = xs[max(0, i-1)], xs[min(len(xs)-1, i+1)]
    for _ in range(55):
        u = lo + (hi-lo)*.3819660112501051
        v = lo + (hi-lo)*.6180339887498949
        if polar(a, u, row["flaps"])[0] < polar(a, v, row["flaps"])[0]:
            lo = u
        else:
            hi = v
    peak = (lo+hi)*.5
    peak_rows.append(dict(**row, independent_peak_rad=float(peak),
        absolute_angle_error_rad=abs(row["peak_rad"]-peak),
        warning_precedes_peak=bool(row["proposed_warning_rad"] < peak),
        status="read-only candidate warning threshold; no native warning added"))

max_polar_error = 0.
for row in supplement.get("polar_samples", []):
    a = next(k["aero"] for k in profile["dynamics"]["aero"]["knots"] if k["mach"] == row["mach"])
    lift, drag = polar(a, row["alpha_rad"], row["flaps"])
    max_polar_error = max(max_polar_error, abs(lift-row["lift"]), abs(drag-row["drag"]))
    assert row["drag"] >= 0

stall_trace = read_json(output / "poststall-explicit-recovery.json")["history_20hz"]
warning_low = min(row["independent_peak_rad"]*.85 for row in peak_rows if row["flaps"] == 0.)
recovered = next((row["t"] for row in stall_trace if abs(row["alpha"]) < warning_low), None)
last10 = [row for row in stall_trace if row["t"] >= 20.]
stall = results["poststall_recovery"]
approach = results["approach"]["run"]
contact = approach["first_contact"]
takeoff = results["takeoff"]
go = results["go_around"]
numeric_maneuvers = dict(
    poststall=dict(first_below_conservative_warning_s=recovered,
        minimum_altitude_m=stall["min_altitude_m"],
        frozen_transient_gate_pass=recovered is not None and recovered <= 15
            and 1500-stall["min_altitude_m"] < 500 and stall["terminal"] is None,
        last10s_all_below_warning=all(abs(row["alpha"]) < warning_low for row in last10),
        final_speed_mps=stall["final_speed_mps"],
        operational_recovery="NOT ESTABLISHED: later large attitude excursion/low-speed flight"),
    takeoff=dict(frozen_altitude_gate_pass=takeoff["final_altitude_m"] > 100
        and takeoff["terminal"] is None,
        operational_takeoff="NOT ESTABLISHED: open-loop script departs ordinary attitude/alpha box"),
    approach=dict(contact_gate_pass=contact is not None and contact["time_s"] <= 40
        and .5 < contact["sink_mps"] < 4,
        brake_trigger="CG altitude<1.2m; pre-contact script, not actual-contact latch",
        parking_pass=approach["parking_pass"]),
    go_around=dict(frozen_altitude_gate_pass=go["final_altitude_m"] > 100
        and go["final_climb_mps"] > 0 and go["terminal"] is None,
        operational_go_around="NOT ESTABLISHED: final speed about16m/s is not sustained controlled climb"))

report = dict(identity_bytes_equal=identity_pass, identity=results["identity"],
    trim_grid=trim_rows, central_local_modes=mode_rows, one_sided_local_modes=side_modes,
    near_corner_modes=(modes(supplement["domain_and_corners"]["corner_jacobians"])
        if supplement else None),
    independent_stall_peaks=peak_rows,
    negative_and_positive_polar_sample_count=len(supplement.get("polar_samples", [])),
    max_polar_coefficient_error=max_polar_error,
    numerical_maneuver_classification=numeric_maneuvers,
    preset_admission=False,
    blocked=["strict stationary parking fails all idle cases", "ordinary takeoff/recovery/go-around not established",
        "complete exact corner/control and weather grid not qualified", "no asset/native/manual/Windows qualification"],
    scope="authored experimental dry-jet numerical prototype; no measured aircraft validation")
(output / "analysis.json").write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")
print(json.dumps(dict(identity=report["identity"], trim_cases=len(trim_rows),
    central_mode_cases=len(mode_rows), one_sided_cases=len(side_modes),
    preset_admission=False, maneuver_classification=numeric_maneuvers), indent=2))
