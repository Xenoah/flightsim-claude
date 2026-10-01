//! Independently compare a local geoid grid with GeographicLib/NGA harmonic
//! reference data. Neither the model nor the reference dataset is bundled.
//!
//! Download links and column definitions:
//! <https://geographiclib.sourceforge.io/C++/doc/geoid.html#testgeoid>
//!
//! ```text
//! cargo run -p flightsim-tilegen --example check_geoid --
//!   egm2008-5.pgm egm2008 GeoidHeights.dat 0.5
//! ```
use flightsim_core::Geodetic;
use flightsim_tilegen::{geoid::GeoidGrid, vertical_datum::GeoidModel};
use std::io::BufRead;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: check_geoid GRID.pgm egm2008|egm96 GeoidHeights.dat MAX_ERROR_METRES".into(),
        );
    }
    let (model, column) = match args[2].as_str() {
        "egm2008" => (GeoidModel::Egm2008, 4),
        "egm96" => (GeoidModel::Egm96, 3),
        _ => return Err("model must be egm2008 or egm96".into()),
    };
    let tolerance: f64 = args[4].parse()?;
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("tolerance must be positive and finite".into());
    }
    let grid = GeoidGrid::open(std::path::Path::new(&args[1]), model)?;
    let reader = std::io::BufReader::new(std::fs::File::open(&args[3])?);
    let (mut count, mut max_error, mut sum_squared) = (0_u32, 0.0_f64, 0.0_f64);
    let mut worst = String::new();
    for line in reader.lines() {
        let line = line?;
        let values: Vec<f64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        if values.len() != 5
            || !values.iter().all(|value| value.is_finite())
            || !(-90.0..=90.0).contains(&values[0])
        {
            return Err(format!("invalid reference row {}", count + 1).into());
        }
        let actual = grid
            .undulation(Geodetic::from_degrees(values[0], values[1], 0.0))
            .ok_or("invalid reference coordinate")?
            .get();
        let error = (actual - values[column]).abs();
        if error > max_error {
            max_error = error;
            worst = format!(
                "lat={} lon={} expected={} actual={actual}",
                values[0], values[1], values[column]
            );
        }
        sum_squared += error * error;
        count = count.checked_add(1).ok_or("too many reference rows")?;
    }
    if count == 0 {
        return Err("reference dataset is empty".into());
    }
    println!(
        "Grid: {} {:?}; interpolation: bilinear",
        grid.description(),
        grid.dimensions()
    );
    println!(
        "Samples: {count}; maximum error: {max_error:.9} m; RMS error: {:.9} m",
        (sum_squared / f64::from(count)).sqrt()
    );
    println!("Worst point: {worst}");
    if max_error > tolerance {
        return Err(format!("maximum error exceeds {tolerance} m tolerance").into());
    }
    Ok(())
}
