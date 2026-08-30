# ARCHITECTURE

地球規模フライトシミュレータの構造仕様。**この文書が実装の正**であり、コードと乖離した場合はどちらかを直す（放置しない）。

関連: [docs/adr/](docs/adr/) に個別の意思決定ログ、[docs/ROADMAP.md](docs/ROADMAP.md) にマイルストーン。

---

## 1. 設計の中心にある制約

この規模のプロジェクトで最初に破綻するのは「機能不足」ではなく **座標精度・更新ループ・依存の汚染** の3つ。全体構造はこの3つを守るために決まっている。

| 制約 | 破綻の症状 | 本設計での対策 |
|---|---|---|
| 地球規模の座標精度 | `f32` で ECEF を扱うと地表で約 0.5m 量子化し、機体が振動する | 世界座標は **`f64` ECEF 固定**。描画直前に **floating origin** で `f32` ローカル座標へ落とす（[ADR-0002](docs/adr/0002-coordinate-system.md)） |
| 時間ステップの安定性 | 可変 dt で剛体積分すると失速・接地時に発散する | FDM は **固定 dt の内部サブステップ + RK4**。描画フレームレートから完全に分離（[ADR-0004](docs/adr/0004-simulation-loop.md)） |
| 依存の汚染 | 物理コードがレンダラを参照し始めるとテストもCIも不可能になる | **`core`/`fdm`/`world`/`sim`/`tilegen` はエンジン非依存の純 Rust**。Bevy は `render`/`input`/`ui`/`app` のみ（[ADR-0001](docs/adr/0001-engine-selection.md)） |

3番目が今回の技術選定の実利そのもの。`cargo test -p flightsim-fdm` が GUI もアセットもなしに数秒で回るからこそ、QA エージェントが回帰網を維持できる。**この境界を壊す PR はレビューで落とす。**

---

## 2. クレート構成と依存の向き

依存は**下から上への一方向のみ**。逆流と横断は禁止。

```
                    ┌─────────────────┐
                    │ flightsim-app   │  統合バイナリ・シーン構築・状態遷移
                    └────────┬────────┘
          ┌──────────────┬───┴───┬──────────────┐
          ▼              ▼       ▼              ▼
    ┌──────────┐  ┌──────────┐ ┌──────┐  ┌──────────┐
    │  render  │  │  input   │ │  ui  │  │ net(後) │   ← Bevy 依存層
    └────┬─────┘  └────┬─────┘ └──┬───┘  └────┬─────┘
         └─────────────┴──────┬───┴───────────┘
                              ▼
              ┌───────────────┴───────────────┐
              ▼                               ▼
        ┌──────────┐                    ┌──────────┐
        │  world   │                    │   fdm    │      ← 純 Rust（Bevy 非依存）
        └────┬─────┘                    └────┬─────┘
             └──────────────┬────────────────┘
                            ▼
                     ┌─────────────┐
                     │    core     │  座標系・単位・時刻。他に一切依存しない
                     └─────────────┘
```

| クレート | 責務 | Bevy 依存 | 担当エージェント |
|---|---|:---:|---|
| `flightsim-core` | WGS84 測地系、ECEF/ENU/NED 変換、単位型、シミュレーション時刻 | ✗ | architect |
| `flightsim-fdm` | 6DoF 剛体、ISA 大気、空力係数、失速、風、着陸装置、積分器 | ✗ | simulation |
| `flightsim-world` | タイル分割、DEM、LOD 選択、ストリーミング、地形高度クエリ | ✗ | world |
| `flightsim-render` | floating origin の適用、地形メッシュの GPU 投入、LOD 描画 | ✓ | rendering |
| `flightsim-input` | 入力マッピング、視点切替、カメラ制御 | ✓ | input-camera |
| `flightsim-ui` | HUD、計器、メニュー、チュートリアル導線 | ✓ | ux |
| `flightsim-sim` | **地形と FDM の結線。** 接地平面の生成、固定ステップ駆動、ヘッドレス実行 | ✗ | architect |
| `flightsim-app` | 全体統合、実行バイナリ | ✓ | orchestrator |
| `flightsim-tilegen` | **オフライン CLI。** GeoTIFF → 実行時タイル `.fsdem` の焼き込み | ✗ | world |
| `flightsim-assetgen` | **オフライン CLI。** Meshy から機体 3D モデルを取得 | ✗ | rendering |

`flightsim-tilegen` は実行時のグラフに乗らない。`world` の上に位置し、
GeoTIFF デコーダ（`tiff`）を抱えるのはこのツールだけ。**実行時クレートが
tilegen に依存してはならない**（デコーダが実行時に載ってしまう。ADR-0003）。

**禁止事項（レビュー自動失格）**
- `core` / `fdm` / `world` / `sim` / `tilegen` の `Cargo.toml` に `bevy` を追加すること
- `fdm` から `world` を参照すること（地形高度は `sim` が引数で渡す）
- `core` / `fdm` / `world` から `sim` / `tilegen` を参照すること
- Bevy 層（`app` / `render`）で地形と FDM の結線を再実装すること（`sim` を呼ぶ）
- 単位付きでない生の `f32` / `f64` を公開 API の引数にすること（[§4](#4-単位と型)）

---

## 3. 座標系

詳細は [ADR-0002](docs/adr/0002-coordinate-system.md)。要点のみ。

| 系 | 型 | 用途 |
|---|---|---|
| **Geodetic** | `Geodetic { lat, lon, alt }` (f64, rad/m) | 入出力・地形タイル索引・空港位置 |
| **ECEF** | `Ecef(DVec3)` (f64, m) | **世界の正準座標。**物理積分はここで行う |
| **NED** | ローカル接平面 (f64, m) | 風・姿勢角・航法計器 |
| **Body** | 機体固定 (f64, m) | 空力・推力・慣性テンソル |
| **Render** | `Vec3` (f32, m) | floating origin 適用後。描画専用 |

変換の入口は `flightsim-core` に集約し、**各クレートが独自に三角関数で変換を書くことを禁ずる**（丸め規約が分岐して原因不明のズレになるため）。

---

## 4. 単位と型

SI を内部の正とする（m, kg, s, rad, K, Pa, N）。ノット・フィート・度は**境界（UI/入力/データ読込）でのみ**変換する。

公開 API は newtype で単位を型に持たせる:

```rust
pub struct Meters(pub f64);
pub struct Knots(pub f64);
pub struct Radians(pub f64);
```

理由: この種のシミュレータで最も多く、かつ最も見つけにくいバグが単位取り違え（ft/m、kt/(m/s)、deg/rad）だから。型で潰す。

---

## 5. 更新ループ

詳細は [ADR-0004](docs/adr/0004-simulation-loop.md)。

```
描画フレーム (可変 dt, 60-144Hz)
  │
  ├─ 入力サンプリング
  │
  ├─ FDM アキュムレータ
  │    while acc >= FIXED_DT {          // FIXED_DT = 1/120 s
  │        fdm.step(FIXED_DT)           //   └ 内部で RK4、必要に応じ更に分割
  │        acc -= FIXED_DT
  │    }
  │
  ├─ 状態補間 (alpha = acc / FIXED_DT)  // 描画のスムージング
  │
  ├─ ワールドストリーミング (予算制: 1フレームあたりの読込上限を固定)
  │
  └─ 描画
```

**不変条件**
- FDM は壁時計時間を一切参照しない。`step(dt)` の `dt` は常に定数。
- ストリーミングは1フレームの処理量に上限を持つ（フレームスパイク防止）。
- 補間は描画のみに影響し、物理状態を書き戻さない。

---

## 6. ワールドデータ

ソースは全てオープンデータ（[ADR-0003](docs/adr/0003-terrain-data.md)）。

| 種別 | ソース | ライセンス |
|---|---|---|
| 標高 | Copernicus DEM GLO-30 (全球 30m) | 無償・再配布可 |
| 空港・滑走路・建物 | OpenStreetMap | ODbL |
| 地表画像 | Sentinel-2 / Natural Earth | 無償 |

タイル分割は **地理座標系クアッドツリー**（level 0 = 経度方向2タイル × 緯度方向1タイル）。Cesium の geographic tiling scheme と同一にして、既存タイルセットとの互換を保つ。

LOD は幾何誤差ベースの screen-space error で選択する（距離ベースではなく）。理由は山岳と平野で必要ポリゴン数が桁違いに違うため。

実行時タイル形式 `.fsdem` は `u16` 量子化 + タイル毎スケールの自前バイナリ（[ADR-0005](docs/adr/0005-runtime-tile-format.md)）。焼き込みは `flightsim-tilegen` が行う。

OSM の滑走路と誘導路は、利用者が用意した地域 PBF から `flightsim-airportgen` が
`aeroway=runway` / `aeroway=taxiway` の中心線を固定長 `.fsairports` へ焼く。
実行時は PBF デコーダに依存せず、開始地点から ECEF 距離が最小の滑走路を選び、
その周辺の誘導路を描画する。元 PBF と派生 DB は同梱しない
（[ADR-0008](docs/adr/0008-osm-airport-data.md)）。

Copernicus DEM GLO-30 の鉛直基準は EGM2008 だが、現在の tilegen は WGS84 楕円体高へ
変換していない。地形・接地・滑走路は同じ数値を共有するので局所的には整合するが、
絶対高度の系統誤差は [Issue #22](../../issues/22) で追跡する。

```text
Copernicus DEM (GeoTIFF)  ──[flightsim-tilegen / オフライン]──>  tiles/{level}/{x}/{y}.fsdem
                                                                          │
                                                              [flightsim-world / 実行時]

OpenStreetMap (.osm.pbf) ──[flightsim-airportgen / オフライン]──> region.fsairports
                                                                          │
                                                              [flightsim-world / 実行時]
```

---

## 7. 現状のスコープ

**この節はマイルストーンごとに更新する。** 実装済みでないものを「ある」と書かないこと。

### 実装済み

| クレート | 内容 |
|---|---|
| `flightsim-core` | 単位型、WGS84 測地系、ECEF/NED/ENU 変換、floating origin、固定ステップ、描画座標フレーム |
| `flightsim-fdm` | ISA 標準大気、WGS84 正規重力、6DoF、失速、プロペラ推力、3 点式着陸装置、接地摩擦・ブレーキ、RK4、定常風と決定論的乱流 |
| `flightsim-world` | 地理座標系クアッドツリー、DEM、SSE-LOD、予算制ストリーミング、LRU、`.fsdem`、スカート付き地形メッシュ、合成滑走路、`.fsairports` v1/v2 の検証と最寄り滑走路選択 |
| `flightsim-tilegen` | GeoTIFF の地理参照解釈・地形タイル焼き込み、OSM PBF の滑走路・誘導路 DB 焼き込み CLI |
| `flightsim-assetgen` | `.env` から鍵を安全に読み、Meshy から glTF / glb を取得するオフライン CLI |
| `flightsim-sim` | 地形と FDM の結線、固定ステップ、滑走路中心線を追うフライトディレクタ、場周飛行、進入初期化、軌跡・着陸・飛行記録 |
| `flightsim-render` | 地形・滑走路・誘導路・滑走路灯メッシュの GPU 投入、LOD 描画、floating origin、大気散乱、時刻・太陽、決定論的な雲層と雲中視程、glTF の軸・倍率補正 |
| `flightsim-input` | キーボード・ゲームパッドの軸合成、舵のレート制御、視点切替、追従カメラ |
| `flightsim-ui` | HUD、操作説明、チュートリアル、飛行記録、着陸の 5 段階評価、計器盤、利用中データの帰属表示 |
| `flightsim-app` | 上記の統合、合成飛行場または OSM の最寄り滑走路と周辺誘導路、風・乱流・時刻・雲層・着陸練習・スクリーンショットの CLI |

雲場は固定 seed の周期的な 2D value/fBm noise を緯度・経度と `TimeOfDay` から
サンプルするため、同じ設定なら同じ結果になる。雲底・雲頂は alpha mask 付きの
PBR 平面で表し、カメラが層内に入ったときだけ distance fog で視程を制限する。
`ClearColor` は変更しない。`--cloud-cover`、`--cloud-base`、`--cloud-top`、
`--cloud-visibility` で設定し、既定は雲量 0 の快晴である。

ワークスペースの全テストを CI の Windows / Linux で実行する。さらに
`clippy -D warnings`、`fmt --check`、依存規約検査、
`cargo doc -D warnings` に加え、Linux の Mesa/lavapipe で同梱 glTF を読み、
スクリーンショットを 1 枚描画する起動スモークを行う。リリース時は Windows zip を
新規ディレクトリに展開し、D3D12 のフォールバックアダプタで同じ検査を行う。

### 未実装

- コックピット内装の 3D モデル（計器盤・計器照明・滑走路灯は実装済み）
- OSM の空港建物・apron、地表画像、METAR、高品質なボリューム雲。
  OSM 対応は滑走路・誘導路の中心線まで
- 難易度設定、HOTAS と軸の再割り当て、推力線オフセット
- 追加機体、リプレイ、ライブ交通、オンライン共有ワールド

### 実装済みだが検証を残すもの

- ゲームパッドは純関数とキーボード共存をテスト済みだが、物理デバイスでの符号・感度は未確認
- 乱流は強度上限・連続性・決定論を検証済みだが、操縦感は未調整
- 実 Copernicus DEM での夜間・高高度表示は目視未検証
- CI の描画スモークは CPU Vulkan、Windows 配布スモークは D3D12 フォールバックを使う。
  実 GPU、ベンダードライバ、性能は保証しない

詳細は [docs/ROADMAP.md](docs/ROADMAP.md)。
