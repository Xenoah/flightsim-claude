// Independent implementation of the documented 64-cube log2 stimulus.
// Output is top-row-first little-endian RGBA32F, with alpha fixed to one.
use std::{fs::File, io::{BufWriter, Write}};
fn main() -> std::io::Result<()> {
    let path = std::env::args().nth(1).expect("output required");
    let mut out = BufWriter::new(File::create(path)?);
    for blue in 0..64 { for green in 0..64 { for red in 0..64 {
        for code in [red, green, blue] {
            let grid = (code as f32) * (1.0f32 / 64.0f32);
            let exposure = grid * (12.0f32 - (-11.0f32)) + (-11.0f32);
            let radiance = 2.0f32.powf(exposure) * 0.18f32;
            out.write_all(&radiance.to_le_bytes())?;
        }
        out.write_all(&1.0f32.to_le_bytes())?;
    } } }
    out.flush()
}
