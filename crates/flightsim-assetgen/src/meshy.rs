//! Meshy API の要求・応答。
//!
//! # ネットワークから切り離してある
//!
//! ここには HTTP が出てこない。要求の組み立てと応答の解釈だけを純関数にして、
//! **API キーもネットワークも無しでテストできる**ようにしている。
//! 外部 API は落ちる・遅い・仕様が変わるので、そこに依存したテストは信用できない。
//!
//! # キーを絶対に漏らさない
//!
//! エラーメッセージ・ログ・デバッグ出力のどこにも API キーを載せない。
//! [`redact`] を通してから出すこと。**一度ログに出たキーは、そのログが
//! 残る限り漏れ続ける。**

use std::fmt;

/// Meshy の API 基点。
pub const API_BASE: &str = "https://api.meshy.ai/openapi/v2/text-to-3d";

/// 生成の段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 形だけ。速いが素材が無い。
    Preview,
    /// テクスチャ付き。preview の結果を入力に取る。
    Refine,
}

impl Stage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Refine => "refine",
        }
    }
}

/// 生成の依頼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRequest {
    pub stage: Stage,
    pub prompt: String,
    /// `refine` のときに必要な、preview 段階のタスク ID。
    pub preview_task_id: Option<String>,
    pub art_style: String,
}

impl GenerationRequest {
    /// 形状だけを作る依頼。
    #[must_use]
    pub fn preview(prompt: impl Into<String>) -> Self {
        Self {
            stage: Stage::Preview,
            prompt: prompt.into(),
            preview_task_id: None,
            art_style: "realistic".to_owned(),
        }
    }

    /// テクスチャを付ける依頼。
    #[must_use]
    pub fn refine(preview_task_id: impl Into<String>) -> Self {
        Self {
            stage: Stage::Refine,
            prompt: String::new(),
            preview_task_id: Some(preview_task_id.into()),
            art_style: "realistic".to_owned(),
        }
    }

    /// 送信する JSON。
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut body = serde_json::json!({ "mode": self.stage.as_str() });
        match self.stage {
            Stage::Preview => {
                body["prompt"] = self.prompt.clone().into();
                body["art_style"] = self.art_style.clone().into();
                // 生成物をそのまま使いたいので、ポリゴンを整えてもらう。
                body["should_remesh"] = true.into();
            }
            Stage::Refine => {
                // refine は preview の ID だけを取る。prompt は無視される。
                body["preview_task_id"] = self.preview_task_id.clone().unwrap_or_default().into();
            }
        }
        body
    }
}

/// タスクの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Succeeded,
    Failed,
    Canceled,
    /// API が知らない状態を返した。
    ///
    /// **未知を「成功」にも「失敗」にも倒さない。** 仕様が変わったときに
    /// 黙って壊れるより、待ち続けて時間切れになるほうが原因を追える。
    Unknown,
}

impl TaskStatus {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        match text.trim().to_uppercase().as_str() {
            "PENDING" => Self::Pending,
            "IN_PROGRESS" => Self::InProgress,
            "SUCCEEDED" => Self::Succeeded,
            "FAILED" => Self::Failed,
            "CANCELED" | "CANCELLED" => Self::Canceled,
            _ => Self::Unknown,
        }
    }

    /// もう待っても変わらないか。
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

/// タスクの現在の状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskState {
    pub id: String,
    pub status: TaskStatus,
    /// 0〜100。
    pub progress: u32,
    /// 完了時のダウンロード先（glb）。
    pub glb_url: Option<String>,
    /// 失敗した場合の理由。
    pub failure: Option<String>,
}

/// 応答の解釈に失敗した。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// JSON として読めない。
    NotJson(String),
    /// 期待した項目が無い。
    MissingField {
        field: &'static str,
        /// 何が返ってきたか。**キーは含めないこと。**
        body: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(body) => write!(
                formatter,
                "the response was not JSON: {}",
                truncate(body, 200)
            ),
            Self::MissingField { field, body } => write!(
                formatter,
                "the response has no `{field}`: {}",
                truncate(body, 200)
            ),
        }
    }
}

impl std::error::Error for ParseError {}

/// 長い応答本文を切り詰める。全部出すとログが読めなくなる。
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let head: String = text.chars().take(limit).collect();
    format!("{head}… ({} chars total)", text.chars().count())
}

/// タスク作成の応答から ID を取り出す。
///
/// # Errors
///
/// JSON でない、または `result` が無い場合。
pub fn parse_task_id(body: &str) -> Result<String, ParseError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ParseError::NotJson(body.to_owned()))?;

    // 素直な形は `{"result": "<id>"}`。念のため `id` も見る。
    value
        .get("result")
        .or_else(|| value.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| ParseError::MissingField {
            field: "result",
            body: body.to_owned(),
        })
}

/// タスク状態の応答を解釈する。
///
/// # Errors
///
/// JSON でない場合。項目が欠けていても、分かる範囲で状態を返す
/// （途中経過の応答は項目が揃っていないことがある）。
pub fn parse_task_state(body: &str) -> Result<TaskState, ParseError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ParseError::NotJson(body.to_owned()))?;

    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .map_or(TaskStatus::Unknown, TaskStatus::parse);

    Ok(TaskState {
        id: value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        status,
        progress: value
            .get("progress")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            .min(100)
            .try_into()
            .unwrap_or(0),
        glb_url: value
            .get("model_urls")
            .and_then(|urls| urls.get("glb"))
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        failure: value
            .get("task_error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
            .filter(|message| !message.is_empty())
            .map(str::to_owned),
    })
}

/// API キーを伏せる。
///
/// ログやエラーに載せるときは必ずこれを通す。**先頭数文字だけ残す**のは、
/// 「どのキーを使っているか」を確認できるようにするため。
#[must_use]
pub fn redact(key: &str) -> String {
    let visible: String = key.chars().take(4).collect();
    if key.is_empty() {
        "<empty>".to_owned()
    } else {
        format!("{visible}… ({} chars)", key.chars().count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- 要求 ---

    #[test]
    fn a_preview_request_carries_the_prompt() {
        let body = GenerationRequest::preview("a small propeller aircraft").to_json();
        assert_eq!(body["mode"], "preview");
        assert_eq!(body["prompt"], "a small propeller aircraft");
        assert_eq!(body["should_remesh"], true);
    }

    #[test]
    fn a_refine_request_carries_the_preview_id_rather_than_a_prompt() {
        let body = GenerationRequest::refine("task-123").to_json();
        assert_eq!(body["mode"], "refine");
        assert_eq!(body["preview_task_id"], "task-123");
        // refine では prompt は使われない。送っても無視されるが、
        // 送らないほうが「効いていないのに指定した気になる」事故を防げる。
        assert!(body.get("prompt").is_none());
    }

    // --- 応答 ---

    #[test]
    fn the_task_id_is_read_from_the_creation_response() {
        assert_eq!(
            parse_task_id(r#"{"result":"018a2b3c"}"#),
            Ok("018a2b3c".to_owned())
        );
        // `id` を返す形にも耐える。
        assert_eq!(parse_task_id(r#"{"id":"xyz"}"#), Ok("xyz".to_owned()));
    }

    #[test]
    fn a_response_without_an_id_is_an_error_that_shows_what_came_back() {
        // 「解析に失敗しました」だけでは、API が何を返したのか分からない。
        let error = parse_task_id(r#"{"message":"quota exceeded"}"#).expect_err("no id");
        let text = error.to_string();
        assert!(text.contains("quota exceeded"), "{text}");
    }

    #[test]
    fn html_error_pages_do_not_look_like_json() {
        let error =
            parse_task_id("<html><body>502 Bad Gateway</body></html>").expect_err("not json");
        assert!(matches!(error, ParseError::NotJson(_)));
        assert!(error.to_string().contains("502"), "{error}");
    }

    #[test]
    fn a_very_long_body_is_truncated_in_the_error() {
        // 応答をそのまま全部出すとログが読めなくなる。
        let body = "x".repeat(10_000);
        let error = parse_task_id(&body).expect_err("not json");
        let text = error.to_string();
        assert!(
            text.len() < 400,
            "the error message was {} chars",
            text.len()
        );
        assert!(
            text.contains("10000"),
            "the total length should be reported: {text}"
        );
    }

    #[test]
    fn a_finished_task_carries_its_download_url() {
        let state = parse_task_state(
            r#"{"id":"t1","status":"SUCCEEDED","progress":100,
                "model_urls":{"glb":"https://example.invalid/model.glb"}}"#,
        )
        .expect("valid json");

        assert_eq!(state.status, TaskStatus::Succeeded);
        assert_eq!(state.progress, 100);
        assert_eq!(
            state.glb_url.as_deref(),
            Some("https://example.invalid/model.glb")
        );
        assert!(state.failure.is_none());
    }

    #[test]
    fn a_task_in_progress_has_no_url_yet() {
        let state = parse_task_state(r#"{"id":"t1","status":"IN_PROGRESS","progress":42}"#)
            .expect("valid json");
        assert_eq!(state.status, TaskStatus::InProgress);
        assert_eq!(state.progress, 42);
        assert!(state.glb_url.is_none());
    }

    #[test]
    fn a_failed_task_carries_its_reason() {
        let state = parse_task_state(
            r#"{"id":"t1","status":"FAILED","task_error":{"message":"unsafe prompt"}}"#,
        )
        .expect("valid json");
        assert_eq!(state.status, TaskStatus::Failed);
        assert_eq!(state.failure.as_deref(), Some("unsafe prompt"));
    }

    #[test]
    fn an_unknown_status_is_not_treated_as_success_or_failure() {
        // 仕様が変わったときに黙って壊れるより、時間切れになるほうが追える。
        let state = parse_task_state(r#"{"status":"QUEUED_FOR_REVIEW"}"#).expect("valid json");
        assert_eq!(state.status, TaskStatus::Unknown);
        assert!(!state.status.is_terminal());
    }

    #[test]
    fn terminal_states_are_the_ones_worth_stopping_on() {
        assert!(TaskStatus::Succeeded.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Canceled.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::InProgress.is_terminal());
    }

    #[test]
    fn a_partial_response_does_not_break_the_parse() {
        // 途中経過の応答は項目が揃っていないことがある。
        let state = parse_task_state("{}").expect("valid json");
        assert_eq!(state.status, TaskStatus::Unknown);
        assert_eq!(state.progress, 0);
        assert!(state.id.is_empty());
    }

    #[test]
    fn an_out_of_range_progress_is_clamped() {
        let state = parse_task_state(r#"{"progress":9999}"#).expect("valid json");
        assert_eq!(state.progress, 100);
    }

    // --- キーの取り扱い ---

    #[test]
    fn the_key_is_never_shown_in_full() {
        // 一度ログに出たキーは、そのログが残る限り漏れ続ける。
        let key = "msy_abcdefghijklmnopqrstuvwxyz0123456789";
        let shown = redact(key);
        assert!(!shown.contains("abcdefghij"), "{shown}");
        assert!(shown.starts_with("msy_"), "{shown}");
        assert!(shown.contains(&key.len().to_string()), "{shown}");
    }

    #[test]
    fn redacting_an_empty_key_says_so() {
        assert_eq!(redact(""), "<empty>");
    }
}
