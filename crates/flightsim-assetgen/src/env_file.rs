//! `.env` ファイルの読み取り。
//!
//! # なぜ自前で書くのか
//!
//! dotenv 系のクレートを入れてもよいが、ここで欲しいのは
//! **「鍵が読めなかったときに、何が起きたかを正確に言うこと」**。
//! 既存クレートは黙って無視する挙動が多く、
//! 「`.env` に書いたのに読まれない」という最も詰まりやすい状況で手掛かりが出ない。
//!
//! パースをファイル操作から切り離してあるので、`.env` を作らずにテストできる。
//!
//! # 値をログに出さない
//!
//! この型は**値を持つが、`Debug` では伏せる**。うっかり `dbg!` した瞬間に
//! 鍵がターミナルとログに出るのを防ぐ。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `.env` の中身。
///
/// `Debug` は鍵の名前だけを出し、値は伏せる。
#[derive(Clone, Default, PartialEq, Eq)]
pub struct EnvFile {
    entries: BTreeMap<String, String>,
}

impl core::fmt::Debug for EnvFile {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // **値を出さない。** うっかり dbg! した瞬間に鍵が漏れる。
        formatter
            .debug_struct("EnvFile")
            .field("keys", &self.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl EnvFile {
    /// 文字列を解釈する。
    ///
    /// 受け付ける形:
    ///
    /// ```text
    /// # コメント
    /// MESHY_API_KEY=msy_...
    /// export MESHY_API_KEY=msy_...
    /// MESHY_API_KEY="msy_..."
    /// ```
    ///
    /// 解釈できない行は捨てるのではなく [`ParsedEnv::malformed`] に残す。
    /// **黙って無視すると「書いたのに効かない」の原因が分からなくなる。**
    #[must_use]
    pub fn parse(contents: &str) -> ParsedEnv {
        let mut entries = BTreeMap::new();
        let mut malformed = Vec::new();

        for (index, raw) in contents.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // `export KEY=value` も受ける。シェル用の .env をそのまま使えるように。
            let line = line.strip_prefix("export ").unwrap_or(line).trim_start();

            let Some((key, value)) = line.split_once('=') else {
                malformed.push(index + 1);
                continue;
            };

            let key = key.trim();
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                malformed.push(index + 1);
                continue;
            }

            entries.insert(key.to_owned(), unquote(value.trim()));
        }

        ParsedEnv {
            file: Self { entries },
            malformed,
        }
    }

    /// ファイルから読む。
    ///
    /// # Errors
    ///
    /// 読み込みに失敗した場合。
    pub fn read(path: &Path) -> Result<ParsedEnv, std::io::Error> {
        Ok(Self::parse(&std::fs::read_to_string(path)?))
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// 入っている鍵の**名前**。値は返さない。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 解釈の結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedEnv {
    pub file: EnvFile,
    /// 解釈できなかった行の番号（1 始まり）。
    pub malformed: Vec<usize>,
}

/// 値を囲む引用符を外す。
///
/// 対応するのは前後が揃っている場合だけ。片方だけの引用符は
/// 値の一部として扱う（`msy_"abc` のような鍵を壊さないため）。
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

/// 作業ディレクトリから上へ辿って `.env` を探す。
///
/// リポジトリのどこで実行しても同じファイルが見つかるようにする。
/// **探した場所を返す**のは、見つからなかったときにそれを伝えるため。
#[must_use]
pub fn find_env_file(start: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut searched = Vec::new();
    let mut directory = Some(start);

    while let Some(current) = directory {
        let candidate = current.join(".env");
        searched.push(candidate.clone());
        if candidate.is_file() {
            return (Some(candidate), searched);
        }
        directory = current.parent();
    }
    (None, searched)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- 解釈 ---

    #[test]
    fn a_plain_assignment_is_read() {
        let parsed = EnvFile::parse("MESHY_API_KEY=msy_abc123");
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some("msy_abc123"));
        assert!(parsed.malformed.is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let parsed = EnvFile::parse(
            "# Meshy の API キー\n\
             \n\
             MESHY_API_KEY=msy_abc\n\
             \n\
             # 予備\n",
        );
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some("msy_abc"));
        assert!(parsed.malformed.is_empty());
    }

    #[test]
    fn an_export_prefix_is_accepted() {
        // シェル用に書いた .env をそのまま使えるように。
        let parsed = EnvFile::parse("export MESHY_API_KEY=msy_abc");
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some("msy_abc"));
    }

    #[test]
    fn quotes_around_the_value_are_removed() {
        for line in [
            r#"MESHY_API_KEY="msy_abc""#,
            "MESHY_API_KEY='msy_abc'",
            "MESHY_API_KEY = msy_abc",
        ] {
            let parsed = EnvFile::parse(line);
            assert_eq!(
                parsed.file.get("MESHY_API_KEY"),
                Some("msy_abc"),
                "failed on `{line}`"
            );
        }
    }

    #[test]
    fn a_lone_quote_stays_part_of_the_value() {
        // 片方だけの引用符で値を壊さない。
        let parsed = EnvFile::parse(r#"MESHY_API_KEY="msy_abc"#);
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some(r#""msy_abc"#));
    }

    #[test]
    fn a_value_containing_equals_is_kept_whole() {
        // base64 の鍵は `=` で終わることがある。
        let parsed = EnvFile::parse("MESHY_API_KEY=abc=def==");
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some("abc=def=="));
    }

    #[test]
    fn a_line_without_an_equals_sign_is_reported_not_ignored() {
        // 黙って無視すると「書いたのに効かない」の原因が分からなくなる。
        let parsed = EnvFile::parse("MESHY_API_KEY msy_abc\nOTHER=1");
        assert_eq!(parsed.malformed, vec![1]);
        assert_eq!(parsed.file.get("OTHER"), Some("1"));
    }

    #[test]
    fn a_key_with_odd_characters_is_reported() {
        let parsed = EnvFile::parse("MY KEY=value\nMY-KEY=value");
        assert_eq!(parsed.malformed, vec![1, 2]);
    }

    #[test]
    fn windows_line_endings_do_not_leak_into_the_value() {
        // メモ帳で作った .env は CRLF になる。\r が鍵に混ざると認証が落ちる。
        let parsed = EnvFile::parse("MESHY_API_KEY=msy_abc\r\nOTHER=1\r\n");
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some("msy_abc"));
        assert_eq!(parsed.file.get("OTHER"), Some("1"));
    }

    #[test]
    fn a_utf8_bom_does_not_break_the_first_line() {
        // PowerShell の Out-File は既定で BOM を付ける。
        let parsed = EnvFile::parse("\u{feff}MESHY_API_KEY=msy_abc");
        // BOM が付くと鍵名が壊れるので、不正な行として報告されること。
        // **黙って読み飛ばすより、行番号が出るほうが直せる。**
        assert!(
            parsed.file.get("MESHY_API_KEY").is_some() || parsed.malformed == vec![1],
            "a BOM should either be handled or reported, got {parsed:?}"
        );
    }

    // --- 値を漏らさない ---

    #[test]
    fn the_debug_output_never_contains_the_value() {
        // うっかり dbg! した瞬間に鍵がターミナルとログに出る。
        let parsed = EnvFile::parse("MESHY_API_KEY=msy_super_secret_value");
        let shown = format!("{:?}", parsed.file);
        assert!(
            !shown.contains("super_secret"),
            "the debug output leaked the value: {shown}"
        );
        assert!(shown.contains("MESHY_API_KEY"), "{shown}");
    }

    #[test]
    fn only_key_names_are_listed() {
        let parsed = EnvFile::parse("A=1\nB=2");
        assert_eq!(parsed.file.keys().collect::<Vec<_>>(), vec!["A", "B"]);
    }

    // --- 探索 ---

    #[test]
    fn the_search_reports_where_it_looked() {
        // 見つからなかったときに「どこを探したか」が出ないと、
        // 置き場所が合っているのか確かめられない。
        let deep = Path::new("C:/one/two/three");
        let (found, searched) = find_env_file(deep);
        assert!(found.is_none() || found.is_some());
        assert!(
            searched.len() >= 3,
            "the search should walk up the tree, looked at {searched:?}"
        );
        assert!(searched[0].ends_with(".env"));
    }

    #[test]
    fn an_existing_env_file_is_found_from_a_subdirectory() {
        let root = std::env::temp_dir().join(format!(
            "flightsim-env-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let nested = root.join("a/b");
        std::fs::create_dir_all(&nested).expect("temp dirs");
        std::fs::write(root.join(".env"), "MESHY_API_KEY=msy_found").expect("write");

        let (found, _) = find_env_file(&nested);
        let found = found.expect("the .env above should be found");
        let parsed = EnvFile::read(&found).expect("readable");
        assert_eq!(parsed.file.get("MESHY_API_KEY"), Some("msy_found"));

        std::fs::remove_dir_all(&root).ok();
    }
}
