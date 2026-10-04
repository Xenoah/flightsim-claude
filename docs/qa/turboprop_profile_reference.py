"""Independent numerical-fixture power-margin samples, not a full-map certificate.

This checks the original authored profile with Python, without running or
importing production FDM code. It is not aircraft/preset flight qualification.
"""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / 'docs/examples/aircraft-profiles-v3/numerical-turboprop.json'


def sample_grid():
    p = json.loads(PROFILE.read_text())['dynamics']['propeller']
    axes = p['advance_ratio'], p['blade_pitch_rad']
    minimum, location, count = math.inf, None, 0
    for i in range(len(axes[0]) - 1):
        for k in range(len(axes[1]) - 1):
            for u in range(17):
                for v in range(17):
                    a, b = u / 16, v / 16
                    j = axes[0][i] * (1-a) + axes[0][i+1] * a
                    beta = axes[1][k] * (1-b) + axes[1][k+1] * b

                    def interpolate(field):
                        c, n = p['cells'], len(axes[1])
                        low = c[i*n+k][field]*(1-b) + c[i*n+k+1][field]*b
                        high = c[(i+1)*n+k][field]*(1-b) + c[(i+1)*n+k+1][field]*b
                        return low*(1-a) + high*a

                    ct, cp = interpolate('ct'), interpolate('cp')
                    assert ct > 0 and cp > 0  # This fixture uses only positive thrust.
                    ideal = ct * (j + math.sqrt(j*j + 8*ct/math.pi)) / 2
                    margin = cp - ideal
                    assert math.isfinite(margin) and margin > 0
                    count += 1
                    if margin < minimum:
                        minimum = margin
                        location = dict(advance_ratio=j, blade_pitch_rad=beta,
                                        ct=ct, cp=cp, ideal_cp=ideal)
    assert count == 1156
    assert abs(minimum - 0.03921045903391185) < 1e-15
    return dict(queries=count, minimum_sampled_cp_margin=minimum, location=location,
                scope='numerical fixture only; samples do not prove the continuous stronger bound')


if __name__ == '__main__':
    print(json.dumps(sample_grid(), indent=2))
