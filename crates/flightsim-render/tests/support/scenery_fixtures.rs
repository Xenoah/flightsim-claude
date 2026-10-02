//! Deterministic synthetic geography shared by CPU regression and benchmark code.
#![allow(dead_code)]

use flightsim_core::{Geodetic, LocalFrame, Meters, Ned};
use flightsim_world::scenery::{
    BuildingHeightSource, LandCoverClass, RoadClass, SceneryBuilding, SceneryFeatureRef,
    SceneryLandCover, SceneryRoad,
};

pub fn anchor() -> Geodetic {
    Geodetic::from_degrees(47.13, 9.53, 0.0)
}

pub fn point(north: f64, east: f64) -> Geodetic {
    let p = LocalFrame::new(anchor())
        .ned_to_ecef_position(Ned::new(north, east, 0.0))
        .to_geodetic();
    Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
}

fn rectangle(north: f64, east: f64, width: f64, depth: f64) -> Vec<Geodetic> {
    vec![
        point(north, east),
        point(north, east + width),
        point(north + depth, east + width),
        point(north + depth, east),
    ]
}

pub fn building(id: u32) -> SceneryBuilding {
    SceneryBuilding {
        source_id: i64::from(id) + 1,
        height: Meters(15.0),
        height_source: BuildingHeightSource::Default,
        footprint: rectangle(
            f64::from(id / 8) * 35.0,
            f64::from(id % 8) * 40.0,
            24.0,
            18.0,
        ),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}

pub fn road(id: u32) -> SceneryRoad {
    SceneryRoad {
        source_id: i64::from(id) + 10_000,
        class: RoadClass::Residential,
        width: Meters(6.0),
        width_inferred: false,
        points: vec![
            point(0.0, f64::from(id) * 12.0),
            point(600.0, f64::from(id) * 12.0),
        ],
    }
}

pub fn forest(id: u32, size: f64) -> SceneryLandCover {
    SceneryLandCover {
        source_id: i64::from(id) + 20_000,
        class: LandCoverClass::Forest,
        boundary: rectangle(
            f64::from(id / 4) * 330.0,
            f64::from(id % 4) * 330.0,
            size,
            size,
        ),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}

#[derive(Debug)]
pub struct Fixture {
    pub buildings: Vec<SceneryBuilding>,
    pub roads: Vec<SceneryRoad>,
    pub land: Vec<SceneryLandCover>,
}

impl Fixture {
    pub fn buildings() -> Self {
        Self {
            buildings: (0..64).map(building).collect(),
            roads: vec![],
            land: vec![],
        }
    }
    pub fn roads() -> Self {
        Self {
            buildings: vec![],
            roads: (0..64).map(road).collect(),
            land: vec![],
        }
    }
    pub fn forest() -> Self {
        Self {
            buildings: vec![],
            roads: vec![],
            land: (0..16).map(|id| forest(id, 300.0)).collect(),
        }
    }
    pub fn rejected_forest() -> Self {
        Self {
            buildings: vec![],
            roads: vec![],
            land: (0..3).map(|id| forest(id, 900.0)).collect(),
        }
    }
    pub fn features(&self) -> Vec<SceneryFeatureRef<'_>> {
        self.buildings
            .iter()
            .map(SceneryFeatureRef::Building)
            .chain(self.roads.iter().map(SceneryFeatureRef::Road))
            .chain(self.land.iter().map(SceneryFeatureRef::LandCover))
            .collect()
    }
}
