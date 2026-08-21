//! # flightsim-assetgen
//!
//! Meshy から機体の 3D モデルを取ってくるオフライン CLI。
//!
//! ## 位置づけ
//!
//! `flightsim-tilegen` と同じくオフライン専用のツール。**実行時には動かない。**
//! 生成したモデルは `assets/` へ置き、アプリが glTF として読む。
//!
//! ## API キー
//!
//! リポジトリ直下の `.env` から読む。**引数では受け取らない。**
//! コマンドライン引数はプロセス一覧やシェル履歴に残る。
//!
//! ```text
//! # .env（.gitignore 対象なのでコミットされない）
//! MESHY_API_KEY=msy_...
//! ```
//!
//! 同名の環境変数があればそちらが優先される。一時的に別の鍵で試したいときに、
//! ファイルを書き換えずに済むようにするため。
//!
//! キーが無くても起動でき、**どこを探したか**と**何をすればよいか**を言って終わる。
//!
//! ## 使い方
//!
//! ```bash
//! # 形だけ（速い、素材なし）
//! cargo run -p flightsim-assetgen -- --prompt "a small white propeller aircraft" \
//!     --output assets/aircraft/light_single.glb
//!
//! # テクスチャまで（preview の続き）
//! cargo run -p flightsim-assetgen -- --refine <preview-task-id> \
//!     --output assets/aircraft/light_single.glb
//! ```

mod env_file;
mod meshy;

use env_file::{EnvFile, find_env_file};
use meshy::{GenerationRequest, TaskStatus, redact};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// キーを入れる環境変数。
const KEY_VARIABLE: &str = "MESHY_API_KEY";

/// 1 回の HTTP 要求の上限。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// 生成が終わるまで待つ上限。
///
/// **上限なしで待たない。** 外部 API は落ちるし、返らないこともある。
const POLL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// 状態を見にいく間隔。
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// 連続で失敗を許す回数。
///
/// 一時的な 5xx で諦めないが、無限には粘らない。
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

// 0 だと 1 回の失敗で諦めることになる。実行時ではなくコンパイル時に落とす。
const _: () = assert!(MAX_CONSECUTIVE_FAILURES > 0);

#[derive(Debug)]
struct Cli {
    prompt: Option<String>,
    refine: Option<String>,
    output: PathBuf,
    /// 生成を投げるだけで、完了を待たずに終わる。
    no_wait: bool,
}

fn main() -> ExitCode {
    let cli = match parse_arguments() {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!();
            eprintln!("{}", usage());
            return ExitCode::FAILURE;
        }
    };

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    [
        "usage: flightsim-assetgen --output <FILE> (--prompt <TEXT> | --refine <TASK_ID>)",
        "",
        "  --prompt <TEXT>    generate a new model from a description",
        "  --refine <ID>      add textures to a previous preview task",
        "  --output <FILE>    where to write the .glb",
        "  --no-wait          submit and print the task id, do not wait",
        "",
        "The API key comes from a `.env` file at the repository root, or from an",
        "environment variable of the same name (which takes precedence):",
        "",
        &format!("    {KEY_VARIABLE}=msy_..."),
        "",
        "It is never accepted as an argument, because command lines show up in",
        "process listings and shell history.",
    ]
    .join("\n")
}

fn parse_arguments() -> Result<Cli, String> {
    let mut prompt = None;
    let mut refine = None;
    let mut output = None;
    let mut no_wait = false;

    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        match flag.as_str() {
            "--prompt" => prompt = arguments.next(),
            "--refine" => refine = arguments.next(),
            "--output" => output = arguments.next().map(PathBuf::from),
            "--no-wait" => no_wait = true,
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }

    let output = output.ok_or("--output is required")?;
    match (&prompt, &refine) {
        (None, None) => return Err("one of --prompt or --refine is required".to_owned()),
        (Some(_), Some(_)) => {
            return Err("--prompt and --refine cannot be combined".to_owned());
        }
        _ => {}
    }

    Ok(Cli {
        prompt,
        refine,
        output,
        no_wait,
    })
}

/// API キーを読む。
///
/// 環境変数を先に見て、無ければ `.env` を探す。**環境変数が勝つ**のは、
/// 一時的に別の鍵で試したいときにファイルを書き換えずに済むため。
///
/// 無ければ**何をすればよいかを言って**失敗する。「キーがありません」だけでは
/// どこに置けばいいのか分からない。
fn api_key() -> Result<String, String> {
    if let Some(key) = std::env::var(KEY_VARIABLE)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        eprintln!("key source: {KEY_VARIABLE} environment variable");
        return Ok(key);
    }

    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (found, searched) = find_env_file(&start);

    let Some(path) = found else {
        return Err(missing_key_message(&searched, None));
    };

    let parsed = EnvFile::read(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;

    // 解釈できなかった行は黙って捨てない。「書いたのに効かない」の原因になる。
    if !parsed.malformed.is_empty() {
        eprintln!(
            "warning: {} has unparseable line(s): {}",
            path.display(),
            parsed
                .malformed
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        eprintln!("         expected `NAME=value` on each line");
    }

    match parsed.file.get(KEY_VARIABLE) {
        Some(key) if !key.trim().is_empty() => {
            eprintln!("key source: {}", path.display());
            Ok(key.trim().to_owned())
        }
        // ファイルはあるが鍵が無い／空。**何が入っていたかを名前だけ出す。**
        _ => Err(missing_key_message(&searched, Some((&path, &parsed.file)))),
    }
}

/// 鍵が見つからなかったときの案内。
///
/// **探した場所と、ファイルに何が入っていたか（名前だけ）を出す。**
/// それが無いと、置き場所が違うのか綴りが違うのか切り分けられない。
fn missing_key_message(searched: &[PathBuf], found: Option<(&PathBuf, &EnvFile)>) -> String {
    let mut message = format!("{KEY_VARIABLE} is not set.\n\n");

    match found {
        Some((path, file)) => {
            message.push_str(&format!(
                "{} exists but has no {KEY_VARIABLE}.\n",
                path.display()
            ));
            if file.is_empty() {
                message.push_str("It contains no assignments at all.\n");
            } else {
                message.push_str(&format!(
                    "It defines: {}\n",
                    file.keys().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        None => {
            message.push_str("No .env file was found. Looked in:\n");
            for path in searched.iter().take(6) {
                message.push_str(&format!("    {}\n", path.display()));
            }
        }
    }

    message.push_str(&format!(
        "\nPut the key in a `.env` file at the repository root:\n\
         \n\
         \x20   {KEY_VARIABLE}=msy_...\n\
         \n\
         `.env` is in .gitignore, so it will not be committed.\n\
         An environment variable of the same name takes precedence if set.\n\
         Setting it only in the current shell will not reach a freshly spawned process."
    ));
    message
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // 応答が返らないまま止まらないよう、必ず上限を置く。
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into()
}

fn run(cli: &Cli) -> Result<(), String> {
    let key = api_key()?;
    eprintln!("using {KEY_VARIABLE} = {}", redact(&key));

    let agent = agent();
    let request = match (&cli.prompt, &cli.refine) {
        (Some(prompt), _) => {
            eprintln!("submitting a preview task: {prompt}");
            GenerationRequest::preview(prompt)
        }
        (_, Some(task)) => {
            eprintln!("submitting a refine task for {task}");
            GenerationRequest::refine(task)
        }
        _ => unreachable!("parse_arguments rejects this"),
    };

    let task_id = submit(&agent, &key, &request)?;
    println!("{task_id}");
    eprintln!("task {task_id} created");

    if cli.no_wait {
        eprintln!("not waiting (--no-wait). Poll it later with --refine or the web UI.");
        return Ok(());
    }

    let url = wait_for_model(&agent, &key, &task_id)?;
    download(&agent, &url, &cli.output)?;

    eprintln!();
    eprintln!("wrote {}", cli.output.display());
    eprintln!(
        "Try it with:\n\
         \x20   cargo run -p flightsim-app --release -- --model {} --view chase",
        cli.output
            .strip_prefix("assets")
            .unwrap_or(&cli.output)
            .display()
    );
    eprintln!(
        "If the aircraft looks sideways or upside down, adjust --model-forward / --model-up."
    );
    Ok(())
}

/// タスクを作る。
fn submit(agent: &ureq::Agent, key: &str, request: &GenerationRequest) -> Result<String, String> {
    let mut response = agent
        .post(meshy::API_BASE)
        .header("Authorization", &format!("Bearer {key}"))
        .send_json(request.to_json())
        .map_err(|error| describe(&error))?;

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read the response: {error}"))?;

    meshy::parse_task_id(&body).map_err(|error| error.to_string())
}

/// 完成するまで待ち、ダウンロード先を返す。
fn wait_for_model(agent: &ureq::Agent, key: &str, task_id: &str) -> Result<String, String> {
    let started = Instant::now();
    let mut failures = 0_u32;
    let mut last_progress = u32::MAX;

    loop {
        if started.elapsed() > POLL_TIMEOUT {
            return Err(format!(
                "gave up after {:.0} minutes. The task may still finish; check it later with \
                 task id {task_id}",
                POLL_TIMEOUT.as_secs_f64() / 60.0
            ));
        }

        match poll_once(agent, key, task_id) {
            Ok(state) => {
                failures = 0;
                if state.progress != last_progress {
                    last_progress = state.progress;
                    eprintln!("  {:?} {}%", state.status, state.progress);
                }
                match state.status {
                    TaskStatus::Succeeded => {
                        return state.glb_url.ok_or_else(|| {
                            "the task succeeded but returned no glb url".to_owned()
                        });
                    }
                    TaskStatus::Failed | TaskStatus::Canceled => {
                        return Err(format!(
                            "the task {:?}: {}",
                            state.status,
                            state
                                .failure
                                .unwrap_or_else(|| "no reason given".to_owned())
                        ));
                    }
                    // 将来 API が別の終端状態を足したときに、待ち続けて
                    // 時間切れになるのを防ぐ安全網。
                    other if other.is_terminal() => {
                        return Err(format!(
                            "the task ended in {other:?} without producing a model"
                        ));
                    }
                    // 未知かつ非終端なら待ち続ける。成功にも失敗にも倒さない。
                    _ => {}
                }
            }
            Err(error) => {
                failures += 1;
                eprintln!("  poll failed ({failures}/{MAX_CONSECUTIVE_FAILURES}): {error}");
                if failures >= MAX_CONSECUTIVE_FAILURES {
                    return Err(format!(
                        "gave up after {failures} consecutive failures. Last error: {error}"
                    ));
                }
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

fn poll_once(agent: &ureq::Agent, key: &str, task_id: &str) -> Result<meshy::TaskState, String> {
    let mut response = agent
        .get(&format!("{}/{task_id}", meshy::API_BASE))
        .header("Authorization", &format!("Bearer {key}"))
        .call()
        .map_err(|error| describe(&error))?;

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read the response: {error}"))?;

    meshy::parse_task_state(&body).map_err(|error| error.to_string())
}

/// glb をファイルへ落とす。
fn download(agent: &ureq::Agent, url: &str, output: &PathBuf) -> Result<(), String> {
    eprintln!("downloading the model...");
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| format!("could not download the model: {}", describe(&error)))?;

    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read the model: {error}"))?;

    if bytes.is_empty() {
        return Err("the downloaded model was empty".to_owned());
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    std::fs::write(output, &bytes)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;

    eprintln!("  {} bytes", bytes.len());
    Ok(())
}

/// HTTP エラーを読める形にする。
///
/// **API キーを含めないこと。** ureq のエラーは URL を含むが、キーは
/// ヘッダにあるので出てこない。ここで自分で足さないよう注意する。
fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(401 | 403) => format!(
            "the API rejected the key ({error}). Check that {KEY_VARIABLE} holds a valid key."
        ),
        ureq::Error::StatusCode(429) => {
            "the API rate-limited the request. Wait a little and try again.".to_owned()
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_says_where_the_key_goes() {
        // 「キーがありません」だけでは、どこに置けばいいのか分からない。
        let text = usage();
        assert!(text.contains(KEY_VARIABLE), "{text}");
        assert!(text.contains(".env"), "{text}");
        assert!(
            text.contains("never accepted as an argument"),
            "the reason should be stated: {text}"
        );
    }

    #[test]
    fn a_missing_env_file_says_where_it_looked() {
        // 置き場所が違うのか綴りが違うのか、切り分けられる情報を出す。
        let searched = vec![PathBuf::from("C:/a/b/.env"), PathBuf::from("C:/a/.env")];
        let message = missing_key_message(&searched, None);
        assert!(message.contains("C:/a/b/.env"), "{message}");
        assert!(message.contains(".gitignore"), "{message}");
        assert!(message.contains("freshly spawned"), "{message}");
    }

    #[test]
    fn an_env_file_without_the_key_lists_what_it_does_have() {
        // 綴り違いに気付けるようにする。**値は出さない。**
        let parsed = EnvFile::parse(
            "MESHY_KEY=msy_secret_value
OTHER=1",
        );
        let path = PathBuf::from("C:/repo/.env");
        let message = missing_key_message(&[], Some((&path, &parsed.file)));

        assert!(message.contains("MESHY_KEY"), "{message}");
        assert!(message.contains("OTHER"), "{message}");
        assert!(
            !message.contains("secret_value"),
            "the message leaked a value: {message}"
        );
    }

    #[test]
    fn an_empty_env_file_says_so() {
        let parsed = EnvFile::parse(
            "# nothing here
",
        );
        let path = PathBuf::from("C:/repo/.env");
        let message = missing_key_message(&[], Some((&path, &parsed.file)));
        assert!(message.contains("no assignments"), "{message}");
    }

    #[test]
    fn an_authentication_failure_points_at_the_key() {
        let message = describe(&ureq::Error::StatusCode(401));
        assert!(message.contains(KEY_VARIABLE), "{message}");
        // エラー文にキーそのものを混ぜていないこと。
        assert!(!message.contains("msy_"), "{message}");
    }

    #[test]
    fn rate_limiting_says_to_wait() {
        let message = describe(&ureq::Error::StatusCode(429));
        assert!(message.to_lowercase().contains("wait"), "{message}");
    }

    #[test]
    fn the_timeouts_are_bounded() {
        // 上限なしで待つと、外部 API が返らないときに永久に止まる。
        assert!(REQUEST_TIMEOUT.as_secs() > 0 && REQUEST_TIMEOUT.as_secs() <= 300);
        assert!(POLL_TIMEOUT.as_secs() > POLL_INTERVAL.as_secs());
        assert!(POLL_TIMEOUT.as_secs() <= 60 * 60);
    }
}
