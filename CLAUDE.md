# flightsim-claude

地球規模のフライトシミュレータ（Rust + Bevy / Windows）。

**新しく入った担当者は [docs/HANDOFF.md](docs/HANDOFF.md) から読むこと。** 現状・次のタスク・すでに踏んだ地雷がまとまっています。

**作業前に [ARCHITECTURE.md](ARCHITECTURE.md) と [docs/adr/](docs/adr/) を読むこと。** 設計判断には理由があり、それを知らずに書いた変更はレビューで差し戻されます。

## コマンド

```bash
# 純 Rust 側。数秒で終わる。物理や地形を触ったらまずこれ。
cargo test -p flightsim-core -p flightsim-fdm -p flightsim-world -p flightsim-sim -p flightsim-tilegen
# 描画層。Bevy を含むので重い。並列度を落とすこと（下記）。
cargo test -j 2 -p flightsim-render -p flightsim-input -p flightsim-ui -p flightsim-app
cargo clippy --workspace --all-targets -- -D warnings   # lint
cargo fmt --all                                         # 整形
bash scripts/check-architecture.sh                      # 依存規約の検査
cargo run -p flightsim-fdm --example aero_trace         # 空力の内訳を時系列表示
cargo run -p flightsim-tilegen -- --help                # 地形タイルの焼き込み
cargo run -p flightsim-assetgen -- --help               # 機体モデルの取得（要 .env の MESHY_API_KEY）
cargo run -p flightsim-sim --bin flightsim-headless -- --help   # 実地形の上を飛ばす
cargo bench --workspace                                 # 性能測定（criterion）
cargo run -p flightsim-app --release                     # 起動（同梱の機体モデルが出る）
cargo run -p flightsim-app --release -- --tiles data/tiles --start 35.55,139.78  # 実地形の上で
```

`core` / `fdm` / `world` / `sim` / `tilegen` は Bevy 非依存なので、GUI もアセットもなしに数秒でテストが回ります。

**`cargo test --workspace` は避けること。** Bevy を含む全クレートのテストバイナリを
同時にビルドするとメモリを使い切り、`failed to mmap ... The paging file is too small`
（os error 1455）で落ちる。**コードの問題に見えるが環境の問題。** 上の 2 グループに
分けて回す（CI もこの分け方にしてある）。

性能について書くときは `cargo bench` の数字を添えること。**測定なしに「速い」「重い」と書かない。**
実測値は [docs/HANDOFF.md](docs/HANDOFF.md) にまとめてある。

## 絶対に破ってはいけない規約

CI で機械的に検査されるもの、および レビューで自動失格になるものです。

1. **`flightsim-core` / `flightsim-fdm` / `flightsim-world` / `flightsim-sim` / `flightsim-tilegen` に `bevy` を依存させない。** これらは純 Rust。Bevy は `render` / `input` / `ui` / `app` のみ（版は [ADR-0007](docs/adr/0007-bevy-version.md) で 0.18.1 に固定。**版上げは独立した PR で**）
2. **依存は一方向。** `core` ← `fdm`/`world` ← `sim` ← `render`/`input`/`ui` ← `app`。逆流も横断（`fdm` → `world`）も禁止。地形と FDM の結線は `sim` にのみ置く（[ADR-0006](docs/adr/0006-simulation-integration-layer.md)）。`tilegen` はオフライン専用
3. **世界座標は `f64` ECEF。** `f32` を位置の正として持たない（地表で約 76cm の量子化が起き、機体が振動する。[ADR-0002](docs/adr/0002-coordinate-system.md)）
4. **座標変換は `flightsim-core` にのみ書く。** 他クレートで `sin`/`cos` を使った測地変換を書かない
5. **公開 API の物理量は単位付き newtype。** 裸の `f64` を渡さない。ft/m・kt/(m/s)・deg/rad の取り違えは型で潰す
6. **FDM は決定論的。** 壁時計時間・乱数・グローバル可変状態を参照しない（[ADR-0004](docs/adr/0004-simulation-loop.md)）

## 単位

内部は SI が正（m, kg, s, rad, K, Pa, N）。ノット・フィート・度への変換は **UI・入力・外部データ読込の境界でのみ** 行い、必ず `flightsim-core::units` の変換を使う。

マジックナンバー（`* 1.94384` など）を各所に書かないこと。係数が散ると片方だけ直されて表示がずれます。

## テストの方針

- **外部の既知値と突き合わせる。** 「実装がこう返すから正しい」は検証になりません。ISA 標準値、測量基準点、公表されている単位換算値を使う
- **境界と特異点を必ず試す。** 経度 ±180°、緯度 ±90°、対気速度 0、失速角前後
- **NaN / Inf が出ないことを検査する。** 数値シミュレーションでは NaN が全状態に伝播し、原因特定が極めて困難になります
- カバレッジの数字を目標にしない

## エージェント編成

`.claude/agents/` に定義。担当外のクレートを触らないこと。

| エージェント | 担当 |
|---|---|
| `architect` | モジュール境界・データ設計・ADR、`flightsim-sim`（地形と FDM の結線・ヘッドレス実行） |
| `simulation` | `flightsim-fdm`（物理・空力・大気） |
| `world` | `flightsim-world`（地形・タイル・LOD）、`flightsim-tilegen`（焼き込み） |
| `rendering` | `flightsim-render`（大気散乱・描画・floating origin） |
| `input-camera` | `flightsim-input`（入力・視点） |
| `ux` | `flightsim-ui`（HUD・計器・チュートリアル） |
| `netcode` | `flightsim-net`（同期・ライブ交通・天候。M2 以降） |
| `qa` | テスト・CI・ベンチ |
| `reviewer` | レビュー専任（統合前に必ず通す） |

## 報告の姿勢

落ちているテストは「落ちている」と報告すること。確認したことと推測を区別すること。「たぶん動く」を「動く」と書かないこと。
