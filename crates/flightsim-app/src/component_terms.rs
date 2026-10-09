//! Fail-closed, local-only Windows component-supplement choice.
//!
//! Embedded documents are the display and receipt authority. The receipt is a
//! local convenience, not identity, a signature, or proof of another person's
//! assent. There is no environment-variable or ordinary screenshot bypass.

pub(super) const TERMS_VERSION: &str = "2026-10-09-2";
pub(super) const TERMS_SHA256: &str =
    "052656e63c7f2df77a7cfffb7d78fcbb64d8525f9d0ca69d39e5475e5bce9d20";
const ENGLISH: &str =
    include_str!("../../../docs/release/components/MICROSOFT-COMPONENT-TERMS.txt");
const JAPANESE: &str =
    include_str!("../../../docs/release/components/MICROSOFT-COMPONENT-TERMS.ja.txt");
const NOTICE: &str =
    include_str!("../../../docs/release/components/MICROSOFT-COMPONENT-NOTICE.txt");
const PROJECT_MIT: &str = include_str!("../../../LICENSE-MIT");
const PROJECT_APACHE: &str = include_str!("../../../LICENSE-APACHE");
#[cfg(any(windows, test))]
const DIALOG_SCRIPT: &str = include_str!("component_terms_dialog.ps1");

pub(super) const SMOKE_MAX_MAIN_UPDATES: u32 = 10_800;
pub(super) const SMOKE_MAX_SECONDS: u64 = 180;
pub(super) const SMOKE_MARKER: &str = "component-terms: internal-release-smoke diagnostic-only; no assent read or written; interactive input disabled; max_main_updates=10800; max_seconds=180";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Admission {
    Continue,
    Exit,
}

/// Only Windows contains the Microsoft component route. Elsewhere the explicit
/// review command prints the same complete documents without claiming assent.
pub(super) fn enforce(review_only: bool) -> Result<Admission, String> {
    #[cfg(windows)]
    {
        let path = windows::receipt_path()?;
        consent_flow(
            review_only,
            || read_receipt(&path),
            windows::show_dialog,
            |bytes| write_receipt(&path, bytes),
        )
    }
    #[cfg(not(windows))]
    {
        if review_only {
            println!(
                "{ENGLISH}\n{JAPANESE}\n{NOTICE}\n{PROJECT_MIT}\n{PROJECT_APACHE}\n\
                 Component terms version: {TERMS_VERSION}\nEnglish SHA-256: {TERMS_SHA256}\n\
                 Non-Windows text view only; no agreement recorded.\n\
                 Redistributors of the Windows binary must use --component-terms on Windows."
            );
            Ok(Admission::Exit)
        } else {
            Ok(Admission::Continue)
        }
    }
}

/// A closed diagnostic command, not a modifier for arbitrary simulator input.
/// Output is data passed through the ordinary parser, never shell text.
pub(super) fn smoke_arguments(arguments: &[String]) -> Result<Option<Vec<String>>, String> {
    if !arguments
        .iter()
        .any(|arg| arg == "--internal-release-smoke")
    {
        return Ok(None);
    }
    if arguments.len() != 3 || arguments[0] != "--internal-release-smoke" {
        return Err("internal release smoke requires exactly SCENE OUTPUT.png; no other options are allowed".into());
    }
    let (aircraft, view) = match arguments[1].as_str() {
        "light-single-cockpit" => ("light-single", "cockpit"),
        "swift-sport-chase" => ("swift-sport", "chase"),
        "light-single-exterior" => ("light-single", "chase"),
        _ => return Err("unknown internal release smoke scene".into()),
    };
    let output = &arguments[2];
    if output.starts_with('-')
        || output.contains(['\0', '\r', '\n'])
        || std::path::Path::new(output).extension() != Some(std::ffi::OsStr::new("png"))
    {
        return Err("internal release smoke output must be a .png path, not an option".into());
    }
    Ok(Some(
        [
            "--screenshot",
            output,
            "--screenshot-delay",
            "5",
            "--exit-after-screenshot",
            "--aircraft",
            aircraft,
            "--view",
            view,
            "--traffic",
            "synthetic",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    ))
}

/// Independent of progress, screenshot readiness, pauses, or user input. This
/// never extends or replaces the release runner's external 180-second deadline.
pub(super) fn arm_smoke_watchdog() -> Result<(), String> {
    std::thread::Builder::new()
        .name("internal-release-smoke-deadline".into())
        .spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(SMOKE_MAX_SECONDS));
            eprintln!("internal release smoke exceeded its fixed 180-second bound");
            std::process::exit(2);
        })
        .map(|_| ())
        .map_err(|error| format!("cannot enforce internal release smoke deadline: {error}"))
}

#[derive(Debug, Default)]
pub(super) struct SmokeBudget {
    updates: u32,
}

impl SmokeBudget {
    pub(super) fn advance(&mut self) -> bool {
        self.updates = self.updates.saturating_add(1);
        self.updates < SMOKE_MAX_MAIN_UPDATES
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Agree,
    Decline,
}

/// The declared SHA is independently checked by packaging. Exact document bytes
/// are also compared, so even a missed hash/version update cannot reuse assent.
#[cfg(any(windows, test))]
fn expected_receipt() -> Vec<u8> {
    let mut receipt = format!(
        "FlightSim component acceptance receipt v1\nversion={TERMS_VERSION}\nenglish_sha256={TERMS_SHA256}\nchoice=explicit-agree-button\n"
    );
    for (name, text) in [
        ("english", ENGLISH),
        ("japanese", JAPANESE),
        ("notice", NOTICE),
        ("project_mit", PROJECT_MIT),
        ("project_apache", PROJECT_APACHE),
    ] {
        receipt.push_str(&format!("{name}_utf8_bytes={}\n", text.len()));
        receipt.push_str(text);
        receipt.push('\n');
    }
    receipt.into_bytes()
}

#[cfg(any(windows, test))]
fn consent_flow(
    review_only: bool,
    read: impl FnOnce() -> Result<Option<Vec<u8>>, String>,
    show: impl FnOnce() -> Result<Choice, String>,
    save: impl FnOnce(&[u8]) -> Result<(), String>,
) -> Result<Admission, String> {
    let expected = expected_receipt();
    // Reopening always shows the full dialog, even with an existing receipt.
    if !review_only && read()?.as_deref() == Some(expected.as_slice()) {
        return Ok(Admission::Continue);
    }
    match show()? {
        Choice::Agree => {
            save(&expected)?;
            Ok(if review_only {
                Admission::Exit
            } else {
                Admission::Continue
            })
        }
        Choice::Decline => {
            Err("component terms declined or closed; FlightSim was not started".into())
        }
    }
}

#[cfg(any(windows, test))]
fn parse_dialog_result(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<Choice, String> {
    let agreed = format!("FLIGHTSIM_COMPONENT_TERMS_AGREE_V1:{TERMS_VERSION}:{TERMS_SHA256}");
    let declined = "FLIGHTSIM_COMPONENT_TERMS_DECLINE_V1";
    let exact_line = |text: &str| {
        stdout == format!("{text}\n").as_bytes() || stdout == format!("{text}\r\n").as_bytes()
    };
    if stderr.is_empty() {
        if code == Some(0) && exact_line(&agreed) {
            return Ok(Choice::Agree);
        }
        if code == Some(1) && exact_line(declined) {
            return Ok(Choice::Decline);
        }
    }
    Err("component terms dialog failed or returned an invalid result; no agreement recorded".into())
}

#[cfg(any(windows, test))]
fn read_receipt(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read component acceptance receipt: {error}")),
    };
    let mut bytes = Vec::new();
    // Oversized/malformed/stale receipts cannot qualify and never cause an
    // unbounded allocation. Other I/O failures stop startup before the dialog.
    file.take(expected_receipt().len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read component acceptance receipt: {error}"))?;
    Ok(Some(bytes))
}

#[cfg(any(windows, test))]
fn write_receipt(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or("component receipt has no parent directory")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create component receipt directory: {error}"))?;
    let temporary = parent.join(format!("component-terms-{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("cannot create component acceptance receipt: {error}"))?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "cannot save component acceptance receipt; startup stopped: {error}"
        ));
    }
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    pub(super) fn receipt_path() -> Result<PathBuf, String> {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("LOCALAPPDATA is unavailable; cannot store local component choice")?;
        Ok(root
            .join("flightsim-claude")
            .join("component-terms")
            .join("accepted.txt"))
    }

    pub(super) fn show_dialog() -> Result<Choice, String> {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("SystemRoot is unavailable; cannot open the component terms dialog")?;
        let powershell = system_root
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        // Fixed embedded program; document data goes only through JSON stdin.
        // No ExecutionPolicy changes, downloads, shell interpolation or script
        // files in a writable directory. Host policy errors fail closed.
        let mut child = Command::new(powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-Command",
                DIALOG_SCRIPT,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot open component terms dialog: {error}"))?;
        let payload = serde_json::json!({
            "version": TERMS_VERSION, "sha256": TERMS_SHA256,
            "english": ENGLISH, "japanese": JAPANESE, "notice": NOTICE,
            "project_licenses": format!("{PROJECT_MIT}\n{PROJECT_APACHE}"),
        })
        .to_string();
        let written = child
            .stdin
            .take()
            .ok_or("component dialog stdin unavailable".to_owned())
            .and_then(|mut stdin| {
                stdin
                    .write_all(payload.as_bytes())
                    .map_err(|error| format!("cannot deliver full component terms: {error}"))
            });
        if let Err(error) = written {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("cannot obtain component terms choice: {error}"))?;
        parse_dialog_result(output.status.code(), &output.stdout, &output.stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn only_exact_successful_explicit_button_protocol_can_agree() {
        let agreed = format!("FLIGHTSIM_COMPONENT_TERMS_AGREE_V1:{TERMS_VERSION}:{TERMS_SHA256}\n");
        assert_eq!(
            parse_dialog_result(Some(0), agreed.as_bytes(), b""),
            Ok(Choice::Agree)
        );
        for code in [None, Some(1), Some(2), Some(-1)] {
            assert!(parse_dialog_result(code, agreed.as_bytes(), b"").is_err());
        }
        for output in [
            b"".as_slice(),
            b"true\n",
            b"yes\n",
            b"AGREE\n",
            b"FLIGHTSIM_COMPONENT_TERMS_AGREE_V1:old:hash\n",
        ] {
            assert!(parse_dialog_result(Some(0), output, b"").is_err());
        }
        assert!(parse_dialog_result(Some(0), agreed.as_bytes(), b"error").is_err());
        assert!(parse_dialog_result(Some(0), format!("{agreed}{agreed}").as_bytes(), b"").is_err());
        assert_eq!(
            parse_dialog_result(Some(1), b"FLIGHTSIM_COMPONENT_TERMS_DECLINE_V1\r\n", b""),
            Ok(Choice::Decline)
        );
    }

    #[test]
    fn absence_or_changed_bytes_require_a_fresh_actual_choice() {
        let expected = expected_receipt();
        let mut changed = expected.clone();
        *changed.last_mut().unwrap() ^= 1;
        for stored in [
            None,
            Some(Vec::new()),
            Some(changed),
            Some(expected[..expected.len() - 1].to_vec()),
        ] {
            let shown = Cell::new(false);
            let result = consent_flow(
                false,
                || Ok(stored),
                || {
                    shown.set(true);
                    Ok(Choice::Decline)
                },
                |_| panic!("decline must not save"),
            );
            assert!(result.is_err());
            assert!(shown.get());
        }
        assert_eq!(
            consent_flow(
                false,
                || Ok(Some(expected)),
                || panic!("matching receipt needs no prompt"),
                |_| panic!("matching receipt needs no write")
            ),
            Ok(Admission::Continue)
        );
    }

    #[test]
    fn reopen_always_shows_the_same_dialog_and_never_starts_simulator() {
        let saves = Cell::new(0);
        assert_eq!(
            consent_flow(
                true,
                || panic!("reopen must not use receipt"),
                || Ok(Choice::Agree),
                |bytes| {
                    assert_eq!(bytes, expected_receipt());
                    saves.set(saves.get() + 1);
                    Ok(())
                }
            ),
            Ok(Admission::Exit)
        );
        assert_eq!(saves.get(), 1);
        assert!(
            consent_flow(
                true,
                || panic!("no read"),
                || Ok(Choice::Decline),
                |_| panic!("no save")
            )
            .is_err()
        );
    }

    #[test]
    fn every_failed_read_dialog_or_save_stops_startup() {
        assert!(
            consent_flow(
                false,
                || Err("read failed".into()),
                || panic!("read failure must stop"),
                |_| panic!("no write")
            )
            .is_err()
        );
        assert!(
            consent_flow(
                false,
                || Ok(None),
                || Err("closed, killed, unavailable, or malformed".into()),
                |_| panic!("no write")
            )
            .is_err()
        );
        assert!(
            consent_flow(
                false,
                || Ok(None),
                || Ok(Choice::Agree),
                |_| Err("save failed".into())
            )
            .is_err()
        );
        assert_eq!(
            consent_flow(
                false,
                || Ok(None),
                || Ok(Choice::Agree),
                |bytes| {
                    assert_eq!(bytes, expected_receipt());
                    Ok(())
                }
            ),
            Ok(Admission::Continue)
        );
    }

    #[test]
    fn receipt_includes_every_complete_displayed_document() {
        let receipt = String::from_utf8(expected_receipt()).unwrap();
        for document in [ENGLISH, JAPANESE, NOTICE, PROJECT_MIT, PROJECT_APACHE] {
            assert!(receipt.contains(document));
        }
        assert!(ENGLISH.contains(&format!("Version {TERMS_VERSION}")));
        assert!(JAPANESE.contains(&format!("Version {TERMS_VERSION}")));
        assert!(NOTICE.contains("Copyright (c) 2026 flightsim-claude contributors"));
    }

    #[test]
    fn receipt_io_is_bounded_and_roundtrips_exact_bytes() {
        let root =
            std::env::temp_dir().join(format!("flightsim-terms-test-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("accepted.txt");
        assert_eq!(read_receipt(&path).unwrap(), None);
        write_receipt(&path, &expected_receipt()).unwrap();
        assert_eq!(read_receipt(&path).unwrap(), Some(expected_receipt()));
        std::fs::write(&path, vec![0; expected_receipt().len() * 2]).unwrap();
        assert_eq!(
            read_receipt(&path).unwrap().unwrap().len(),
            expected_receipt().len() + 1
        );
        assert!(write_receipt(&root.join("absent").join(".."), b"x").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn smoke_has_only_three_closed_noninteractive_scenarios() {
        for (scene, aircraft, view) in [
            ("light-single-cockpit", "light-single", "cockpit"),
            ("swift-sport-chase", "swift-sport", "chase"),
            ("light-single-exterior", "light-single", "chase"),
        ] {
            assert_eq!(
                smoke_arguments(&args(&[
                    "--internal-release-smoke",
                    scene,
                    "output path.png"
                ]))
                .unwrap()
                .unwrap(),
                args(&[
                    "--screenshot",
                    "output path.png",
                    "--screenshot-delay",
                    "5",
                    "--exit-after-screenshot",
                    "--aircraft",
                    aircraft,
                    "--view",
                    view,
                    "--traffic",
                    "synthetic"
                ])
            );
        }
        for invalid in [
            vec!["--internal-release-smoke"],
            vec!["--internal-release-smoke", "other", "x.png"],
            vec![
                "--internal-release-smoke",
                "swift-sport-chase",
                "x.png",
                "--help",
            ],
            vec!["--help", "--internal-release-smoke", "x.png"],
            vec![
                "--internal-release-smoke",
                "swift-sport-chase",
                "--override.png",
            ],
            vec!["--internal-release-smoke", "swift-sport-chase", "x.json"],
        ] {
            assert!(smoke_arguments(&args(&invalid)).is_err());
        }
        assert_eq!(
            smoke_arguments(&args(&["--headless-screenshot", "x.png"])).unwrap(),
            None
        );
        assert_eq!(smoke_arguments(&[]).unwrap(), None);
    }

    #[test]
    fn smoke_budget_is_fixed_and_cannot_reset_or_wrap() {
        let mut budget = SmokeBudget::default();
        for _ in 1..SMOKE_MAX_MAIN_UPDATES {
            assert!(budget.advance());
        }
        assert!(!budget.advance());
        assert!(!budget.advance());
        budget.updates = u32::MAX;
        assert!(!budget.advance());
        assert_eq!(SMOKE_MAX_SECONDS, 180);
        assert!(SMOKE_MARKER.contains("no assent read or written"));
    }

    #[test]
    fn fixed_dialog_has_no_automatic_agreement_or_policy_override() {
        assert!(DIALOG_SCRIPT.contains("$agree.Add_Click({"));
        assert!(DIALOG_SCRIPT.contains("$script:agreed = $false"));
        assert!(DIALOG_SCRIPT.contains("$form.AcceptButton = $null"));
        assert!(DIALOG_SCRIPT.contains("$form.CancelButton = $decline"));
        assert!(!DIALOG_SCRIPT.contains("ExecutionPolicy"));
        assert!(!DIALOG_SCRIPT.contains("Invoke-Expression"));
        assert!(!DIALOG_SCRIPT.contains("Start-Process"));
        assert!(!DIALOG_SCRIPT.contains("Invoke-WebRequest"));
    }
}
