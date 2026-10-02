"""Known-answer tests for the independent CPU mesh-clearance diagnostic."""
import importlib.util
from pathlib import Path
import unittest
import numpy as np

SCRIPT = Path(__file__).resolve().parents[1] / 'diagnose_airport_clearance.py'
spec = importlib.util.spec_from_file_location('airport_clearance', SCRIPT)
qa = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qa)


class ClearanceTests(unittest.TestCase):
    def test_wgs84_equator_and_pole_have_known_axes(self):
        np.testing.assert_allclose(qa.ecef(0, 0, 0), [6378137, 0, 0], atol=1e-9)
        np.testing.assert_allclose(qa.ecef(90, 0, 0), [0, 0, 6356752.314245179], atol=1e-8)

    def test_horizontal_plane_hit_and_outside_triangle(self):
        a, b, c = np.array([[0, 0, 12], [1, 0, 12], [0, 1, 12]], dtype=float)
        self.assertEqual(qa.ray_triangle_height(np.array([.25, .25, 0]), np.array([0, 0, 1]), a, b, c), 12)
        self.assertTrue(np.isnan(qa.ray_triangle_height(np.array([2, 2, 0]), np.array([0, 0, 1]), a, b, c)))

    def test_saddle_triangle_is_fifty_metres_above_bilinear_centre(self):
        # NW=SE=0, NE=SW=100. Bilinear center=50; actual NE-SW edge=100.
        nw, ne, sw, se = np.array([[0, 0, 0], [1, 0, 100], [0, 1, 100], [1, 1, 0]], dtype=float)
        ray = np.array([.5, .5, 0])
        for vertices in ((nw, sw, ne), (ne, sw, se)):
            rendered = qa.ray_triangle_height(ray, np.array([0, 0, 1]), *vertices)
            self.assertAlmostEqual(rendered - 50, 50)

    def test_sampling_exact_surface_at_airport_vertices_still_misses_crease(self):
        # A 10 m by 10 m airport face spans [5,5] to [15,15] in a 20 m saddle cell.
        # Its endpoints are exactly on terrain (50 m); terrain is 100 m at
        # their midpoint. Therefore changing the elevation callback alone
        # still buries an interior point by 50 m. Split at the terrain edge.
        nw, ne, sw, se = np.array([[0, 0, 0], [20, 0, 100], [0, 20, 100], [20, 20, 0]], dtype=float)
        before = qa.ray_triangle_height(np.array([5, 5, 0]), np.array([0, 0, 1]), nw, sw, ne)
        after = qa.ray_triangle_height(np.array([15, 15, 0]), np.array([0, 0, 1]), ne, sw, se)
        edge = qa.ray_triangle_height(np.array([10, 10, 0]), np.array([0, 0, 1]), nw, sw, ne)
        self.assertAlmostEqual(edge - (before + after) / 2, 50)
        # Clipping adds the intersection to both faces; any interpolation
        # within each underlying plane now coincides at every point.
        quarter = qa.ray_triangle_height(np.array([7.5, 7.5, 0]), np.array([0, 0, 1]), nw, sw, ne)
        self.assertAlmostEqual(quarter, (before + edge) / 2)

    def test_flat_curved_earth_mesh_is_below_geodetic_surface(self):
        class FlatAtlas:
            @staticmethod
            def sample(latitude, longitude):
                return np.zeros(np.broadcast_arrays(latitude, longitude)[0].shape)
        lat, lon = np.array([35.55]), np.array([139.78])
        coarse = qa.mesh_height(FlatAtlas(), lat, lon, 6)
        fine = qa.mesh_height(FlatAtlas(), lat, lon, 13)
        self.assertLess(coarse[0], -.1)
        self.assertLess(abs(fine[0]), .002)
        self.assertGreater(fine[0], coarse[0])

    def test_existing_synthetic_runway_geometry_and_light_count(self):
        atlas = qa.Atlas(qa.ROOT / 'crates/flightsim-world/data/global-terrain.fsgt')
        probes, count = qa.synthetic_runway_probes(atlas)
        self.assertEqual(count, 106)
        self.assertEqual(len(probes['pavement_triangle_centroids'][0]), 16100)
        self.assertEqual(len(probes['light_top_triangle_centroids'][0]), 212)
        for p, l, height in probes.values():
            self.assertTrue(np.isfinite(height).all())
            self.assertTrue(((p > 35.54) & (p < 35.58)).all())
            self.assertTrue(((l > 139.77) & (l < 139.81)).all())

    def test_bundled_atlas_is_periodic_and_polar(self):
        atlas = qa.Atlas(qa.ROOT / 'crates/flightsim-world/data/global-terrain.fsgt')
        np.testing.assert_allclose(atlas.sample([0, 30, -30], [180, 181, -181]),
                                   atlas.sample([0, 30, -30], [-180, -179, 179]), atol=1e-9)
        for latitude in [-90, 90]:
            values = atlas.sample(latitude, np.linspace(-180, 180, 13))
            np.testing.assert_allclose(values, values[0], atol=1e-9)


if __name__ == '__main__':
    unittest.main()
