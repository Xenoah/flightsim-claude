#[path = "support/cedar_near_static.rs"]
mod cedar;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = cedar::critical();
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "docs/qa/cedar-near-static-critical.json".to_owned());
    std::fs::write(&path, serde_json::to_string_pretty(&report)? + "\n")?;
    println!("{path}");
    if report["critical_runs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| !r["failure"].is_null())
    {
        return Err(
            "Unchanged critical braking trajectory rejected; broad qualification stopped".into(),
        );
    }
    if std::env::args().any(|a| a == "--matrix") {
        let matrix = cedar::matrix();
        let path = "docs/qa/cedar-near-static-matrix.json";
        std::fs::write(path, serde_json::to_string_pretty(&matrix)? + "\n")?;
        println!("{path}");
        if matrix["cases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| !c["result"]["failure"].is_null())
        {
            return Err("Cedar flight/contact matrix contains a rejection".into());
        }
    }
    Ok(())
}
