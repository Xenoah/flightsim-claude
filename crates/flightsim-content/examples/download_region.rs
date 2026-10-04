//! Explicit command-line consumer of the opt-in download foundation.
use flightsim_content::download::{CacheMode, DownloadSource, stage_github_with_progress};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 || !matches!(args[4].as_str(), "online" | "offline") {
        return Err("usage: download_region <GITHUB_ZIP_URL> <SHA256> <CACHE_DIR> <STORE_DIR> <online|offline>".into());
    }
    let source = DownloadSource::github(&args[0], &args[1])?;
    let mode = if args[4] == "offline" {
        CacheMode::Offline
    } else {
        CacheMode::PreferCache
    };
    let mut phase = String::new();
    let downloaded = stage_github_with_progress(
        &source,
        Path::new(&args[2]),
        Path::new(&args[3]),
        mode,
        |progress| {
            // Log phase changes only; applications can display the byte/file counters.
            let current = format!("{progress:?}")
                .split([' ', '{', '('])
                .next()
                .unwrap_or("")
                .to_string();
            if current != phase {
                eprintln!("{current}");
                phase = current;
            }
            true
        },
    )?;
    let installed = downloaded.staged.commit()?;
    println!(
        "Installed {}@{} from {} (archive SHA256 {}, cache hit {}). No flight activated.",
        installed.identity().id,
        installed.identity().version,
        downloaded.source.url(),
        downloaded.source.archive_sha256(),
        downloaded.cache_hit
    );
    Ok(())
}
