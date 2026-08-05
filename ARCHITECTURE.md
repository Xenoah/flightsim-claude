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
| 依存の汚染 | 物理コードがレンダラを参照し始めるとテストもCIも不可能になる | **`core`/`fdm`/`world` はエンジン非依存の純 Rust**。Bevy は `render`/`app` のみ（[ADR-0001](docs/adr/0001-engine-selection.md)） |

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
| `flightsim-render` | 大気散乱、雲、地形メッシュ生成、LOD 描画 | ✓ | rendering |
| `flightsim-input` | 入力マッピング、視点切替、カメラ制御 | ✓ | input-camera |
| `flightsim-ui` | HUD、計器、メニュー、チュートリアル導線 | ✓ | ux |
| `flightsim-app` | 全体統合、実行バイナリ | ✓ | orchestrator |
| `flightsim-tilegen` | **オフライン CLI。** GeoTIFF → 実行時タイル `.fsdem` の焼き込み | ✗ | world |

`flightsim-tilegen` は実行時のグラフに乗らない。`world` の上に位置し、
GeoTIFF デコーダ（`tiff`）を抱えるのはこのツールだけ。**実行時クレートが
tilegen に依存してはならない**（デコーダが実行時に載ってしまう。ADR-0003）。

**禁止事項（レビュー自動失格）**
- `core` / `fdm` / `world` / `tilegen` の `Cargo.toml` に `bevy` を追加すること
- `fdm` から `world` を参照すること（地形高度は呼び出し側が引数で渡す）
- `core` / `fdm` / `world` から `tilegen` を参照すること
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

```text
Copernicus DEM (GeoTIFF)  ──[flightsim-tilegen / オフライン]──>  tiles/{level}/{x}/{y}.fsdem
                                                                          │
                                                              [flightsim-world / 実行時]
```

---

## 7. 現状のスコープ

**この節はマイルストーンごとに更新する。** 実装済みでないものを「ある」と書かないこと。

### 実装済み（テストで検証済み）

| クレート | 内容 | テスト数 |
|---|---|---:|
| `flightsim-core` | 単位型、WGS84 測地系、ECEF/NED/ENU 変換、floating origin、固定ステップ | 50 |
| `flightsim-fdm` | ISA 標準大気、WGS84 正規重力、6DoF 剛体、空力係数、失速、3 点式着陸装置、接地摩擦・ブレーキ、RK4 + 剛性対応サブステップ | 103 |
| `flightsim-world` | 地理座標系クアッドツリー、DEM サンプリング、SSE-LOD、ストリーミング、LRU キャッシュ、実行時タイル形式の読み書き | 86 |
| `flightsim-tilegen` | GeoTIFF の地理参照解釈、面積平均リサンプリング、タイル列挙、焼き込み CLI | 55 |

CI で `cargo test` / `clippy -D warnings` / `fmt --check` / 依存規約検査 / `cargo doc -D warnings` を回している。

### 未実装

- **描画が一切ない。** M1 はヘッドレスの物理・地形基盤まで
- **FDM とワールドの結線。** 焼いたタイルの標高を FDM へ渡すヘッドレス統合ランナーが無い
- 推力線オフセット、乱流
- 地形メッシュ生成（亀裂対策のスカート込み）
- OSM（空港・建物）と地表画像の取り込み。tilegen が扱うのは標高のみ
- 天候、ライブ交通、オンライン共有ワールド、複数機体、コックピット操作

詳細は [docs/ROADMAP.md](docs/ROADMAP.md)。
