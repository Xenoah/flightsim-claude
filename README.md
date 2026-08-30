# flightsim-claude

地球規模のフライトシミュレータ。Rust + Bevy、Windows 対象。

**M2（離陸→場周→着陸のゲームループ）を達成し、M3 を実装中です。** 実地形・機体・
滑走路・時刻・風・乱流が描画され、着陸練習と 5 段階評価まで遊べます。検証済みの範囲と
未検証事項は [ARCHITECTURE.md §7](ARCHITECTURE.md#7-現状のスコープ) を参照してください。

---

## 何ができるか（今）

```bash
# 純 Rust 側。数秒
cargo test -p flightsim-core -p flightsim-fdm -p flightsim-world \
    -p flightsim-sim -p flightsim-tilegen -p flightsim-assetgen
# 描画層。Bevy を含むので重い
cargo test -j 2 -p flightsim-render -p flightsim-input -p flightsim-ui -p flightsim-app
```

**`cargo test --workspace` は避けてください。** Bevy を含む全クレートのテストバイナリを
同時にビルドするとメモリを使い切り、`failed to mmap ... The paging file is too small`
（os error 1455）で落ちます。**コードの問題に見えますが環境の問題です。**

- **WGS84 測地系と `f64` ECEF 世界座標** — 地球全体で振動しない位置表現。描画用の
  floating origin 付き
- **6DoF 飛行力学モデル** — ISA 標準大気、緯度依存の正規重力、失速を含む空力モデル、
  RK4 固定ステップ積分、3 点式着陸装置、接地摩擦とブレーキ。決定論的で、離着陸と
  10 分の飛行を数秒でヘッドレス検証できる
- **地形タイル基盤** — 地理座標系クアッドツリー（極まで表現可能）、DEM のバイリニア
  サンプリング、幾何誤差ベースの LOD 選択、フレーム予算つきストリーミング
- **実地形の焼き込み** — Copernicus DEM の GeoTIFF から実行時タイルを生成する
  オフライン CLI。日付変更線と極、投影座標系の誤読、nodata を扱う
- **実地形の上をヘッドレスで飛べる** — 焼いたタイルから接地平面（標高と勾配）を作って
  FDM へ渡し、離陸 → 上昇 → 巡航 → 旋回 → 進入 → 接地までを軌跡 CSV に出力する
- **機体 3D モデルを読める** — glTF / glb を機体軸へ合わせる補正層つき。モデルごとに
  違う「前」「上」の軸と大きさを引数で吸収するので、差し替えても描画コードを触らない。
  **テクスチャ付きの軽単発機を同梱**しているので、引数なしで起動すれば機体が出る
- **機体モデルの取得** — Meshy の API から取ってくるオフライン CLI。API キーは `.env`
  から読み、**引数では受け取らない**（コマンドラインはプロセス一覧とシェル履歴に残る）
- **合成飛行場とゲームループ** — 引数なしで滑走路の中心線上から始まり、離陸して
  戻って降りると**着陸が 5 段階で評価される**（沈下率・滑走路上か・バンク）。
  滑走路は舗装・中心線・ピアノキーつき。回帰テスト用フライトディレクタは6 km手前から
  中心線を連続捕捉し、左右6 m/sの直角横風でも滑走路内へ接地する
- **ゲームパッド対応** — 左スティックで操縦桿、トリガーでスロットル（保持式）。
  デッドゾーンと感度カーブつき。**キーボードと軸ごとに共存**する（繋いだら
  キーボードが死ぬ、はない）
- **風と乱流** — `--wind 270/10`（方位/ノット）と `--turbulence moderate`。
  空力が相対速度で効くので横風着陸が成立する。向かい風で離陸滑走が実際に短くなる。
  **乱流は乱数ではなく時刻と位置の決定論的な関数**（リプレイと同期のため）
- **着陸練習** — `--approach 1.5` で滑走路の手前 1.5 海里・3 度の進入角から始まる。
  毎回場周を一周しなくてよい
- **チュートリアル導線** — 「今なにをすべきか」を画面中央上に 1〜2 行で出す。
  一度着陸したら二度と出ず、`H` でいつでも消せる。**上級者の邪魔をしない**
- **時刻と太陽位置** — `--time 05:30`（地方平均太陽時）と `--time-rate 60`。
  天文計算（Meeus / NOAA と同じ低精度式）で日の出・南中・日没が正しい位置に来る。
  夏至の東京の南中高度 77.75°、分点の日の出は真東、極の白夜と極夜まで一致する

| ゲームパッド | 機能 |
|---|---|
| 左スティック | エルロン / エレベータ（手前で機首上げ） |
| 右スティック X | ラダー |
| RT / LT | スロットル増 / 減（離しても保持） |
| 十字キー 下 / 上 | フラップ 出す / 収める |
| A（下ボタン） | ブレーキ |
| Y（上ボタン） | 視点切替 |

```bash
cargo run -p flightsim-fdm --example aero_trace   # 空力の内訳を時系列で表示

cargo run -p flightsim-tilegen --     --input Copernicus_DSM_COG_10_N35_00_E139_00_DEM.tif     --output data/tiles --min-level 8 --max-level 12

cargo run -p flightsim-sim --bin flightsim-headless --     --tiles data/tiles --start 35.553,139.781 --output flight.csv

cargo bench --workspace                           # 性能測定（criterion）
```

### Windows で起動する

`v0.6.0-alpha.5` 以降の [Releases](https://github.com/Xenoah/flightsim-claude/releases) には
`flightsim-claude-v<version>-windows-x86_64.zip` が付きます。展開後、`assets/` を
`flightsim-app.exe` と同じフォルダに置いたまま実行してください。これは開発途中の
prerelease です。

## 未実装・未検証

- コックピット内装と計器の照明、OSM 空港データ、METAR・視程、雲、難易度設定
- HOTAS と軸の再割り当て、追加機体、リプレイ、ライブ交通、オンライン共有ワールド
- ゲームパッドは変換ロジックとキーボード共存を自動テスト済みですが、実機での符号・
  感度確認は未実施
- フライトディレクタの中心線誘導は回帰テスト用で、ILS・航法データ・autolandではありません
- 乱流は数値・決定論を検証済みですが、操縦感の調整は未実施。実 DEM の夜間・高高度も
  目視未検証です
- CI は Mesa の CPU Vulkan で 1 枚描画し、Windows 配布 zip もクリーンな
  展開先から D3D12 ソフトウェアアダプタで起動します。アプリ結線と同梱アセットは
  検査しますが、実 GPU・ベンダードライバ・FPS は保証しません

[docs/ROADMAP.md](docs/ROADMAP.md) に段階と、後回しにした理由を書いています。

---

## 設計

| 文書 | 内容 |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | クレート構成、依存の向き、座標系、更新ループ |
| [ADR-0001](docs/adr/0001-engine-selection.md) | なぜ Rust + Bevy か |
| [ADR-0002](docs/adr/0002-coordinate-system.md) | なぜ `f64` ECEF + floating origin か |
| [ADR-0003](docs/adr/0003-terrain-data.md) | なぜオープンデータと自前パイプラインか |
| [ADR-0004](docs/adr/0004-simulation-loop.md) | なぜ固定ステップ RK4 か |
| [ADR-0005](docs/adr/0005-runtime-tile-format.md) | なぜ自前の `u16` 量子化タイル形式か |
| [ADR-0006](docs/adr/0006-simulation-integration-layer.md) | なぜ結線を `flightsim-sim` に置くか |
| [ADR-0007](docs/adr/0007-bevy-version.md) | なぜ Bevy 0.18.1 か |

設計の要は 1 点に集約されます。

> **`flightsim-core` / `flightsim-fdm` / `flightsim-world` / `flightsim-sim` / `flightsim-tilegen` は Bevy に依存しない。**

物理と地形が純 Rust であるおかげで、`cargo test` が GUI もアセットもなしに数秒で回ります。
これは慣習ではなく [CI で検査される規約](scripts/check-architecture.sh) です。

```text
                    ┌─────────────────┐
                    │ flightsim-app   │
                    └────────┬────────┘
          ┌──────────────┬───┴───┬──────────────┐
          ▼              ▼       ▼              ▼
       render         input     ui            net        ← Bevy 依存層
          └─────────────┴───────┬┴──────────────┘
                                ▼
                   ┌────────────┴────────────┐
                   ▼                         ▼
                 world                      fdm          ← 純 Rust
                   └────────────┬────────────┘
                                ▼
                              core
```

---

## 開発体制

複数のエージェントが 1 モジュールずつ担当します。定義は
[.claude/agents/](.claude/agents/)、共通規約は [CLAUDE.md](CLAUDE.md)。

| エージェント | 担当 |
|---|---|
| `architect` | モジュール境界・データ設計・ADR |
| `simulation` | 物理・空力・大気 |
| `world` | 地形・タイル・LOD |
| `rendering` | 大気散乱・描画・floating origin |
| `input-camera` | 入力・視点 |
| `ux` | HUD・計器・チュートリアル |
| `netcode` | 同期・ライブ交通・天候 |
| `qa` | テスト・CI・ベンチ |
| `reviewer` | レビュー専任 |

---

## データの帰属表示

利用するデータのライセンスと帰属表示は [ATTRIBUTION.md](ATTRIBUTION.md)。
OpenStreetMap と ESA WorldCover は帰属表示が**法的に必須**です。

## ライセンス

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)
