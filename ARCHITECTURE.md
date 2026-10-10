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
| 依存の汚染 | 物理コードがレンダラを参照し始めるとテストもCIも不可能になる | **`core`/`fdm`/`world`/`sim`/`tilegen`/`net` はエンジン非依存の純 Rust**。Bevy は `render`/`input`/`ui`/`audio`/`app` のみ（[ADR-0001](docs/adr/0001-engine-selection.md)） |

3番目が今回の技術選定の実利そのもの。`cargo test -p flightsim-fdm` が GUI もアセットもなしに数秒で回るからこそ、QA エージェントが回帰網を維持できる。**この境界を壊す PR はレビューで落とす。**

---

## 2. クレート構成と依存の向き

依存は**下から上への一方向のみ**。逆流と横断は禁止。

```text
                         flightsim-app
             /          /       |       \          \
          render      input     ui     audio       net
             \          \       |       /          |
                         sim                      core
                       /     \
                    world    fdm
                       \     /
                         core

     tilegen -> world/core (offline only)
     assetgen (offline only)
```

Aircraft package integrity and static GLB resource validation live in pure
`content`; app supplies the existing original-byte physical-profile decoder.
Terrain manifest v1 remains DEM-only. Offline aircraft import publishes data but
never activates a flight, changes replay identity or grants distribution rights.
See [ADR-0027](docs/adr/0027-offline-aircraft-data-packages.md).

`net` は Bevy / FDM / world に依存しない純 Rust の UDP・交通データ層。
ECEF のスナップショットと通信状態を app が描画・UI に結線する。
通信時刻は `Time<Real>`、合成交通の時刻は simulation の固定時間を使い、
通信の遅延や到着順を自機の物理積分へ混ぜない（[ADR-0010](docs/adr/0010-local-traffic-sessions.md)）。

| クレート | 責務 | Bevy 依存 | 担当エージェント |
|---|---|:---:|---|
| `flightsim-core` | WGS84 測地系、ECEF/ENU/NED 変換、単位型、シミュレーション時刻 | ✗ | architect |
| `flightsim-fdm` | 6DoF 剛体、ISA 大気、空力係数、失速、風、着陸装置、積分器 | ✗ | simulation |
| `flightsim-world` | タイル分割、DEM、LOD 選択、ストリーミング、地形高度クエリ | ✗ | world |
| `flightsim-content` | Data-only terrain and aircraft-package validation, immutable installation, hash-checked regional sources and opt-in prepared terrain GitHub acquisition | ✗ | architect/world |
| `flightsim-render` | floating origin の適用、地形メッシュの GPU 投入、LOD 描画 | ✓ | rendering |
| `flightsim-input` | 入力マッピング、視点切替、カメラ制御 | ✓ | input-camera |
| `flightsim-ui` | HUD、計器、メニュー、チュートリアル導線 | ✓ | ux |
| `flightsim-audio` | エンジン音・風切り音・失速警報。**波形はコードで合成する**（[ADR-0009](docs/adr/0009-synthesised-audio.md)） | ✓ | ux |
| `flightsim-sim` | **地形と FDM の結線。** 接地平面の生成、固定ステップ駆動、ヘッドレス実行 | ✗ | architect |
| `flightsim-net` | 決定論的な合成交通、補間、ローカル/LAN セッション | ✗ | netcode |
| `flightsim-app` | 全体統合、実行バイナリ | ✓ | orchestrator |
| `flightsim-tilegen` | **オフライン CLI。** GeoTIFF → 実行時タイル `.fsdem` の焼き込み | ✗ | world |
| `flightsim-assetgen` | **オフライン CLI。** Meshy から機体 3D モデルを取得 | ✗ | rendering |

`flightsim-tilegen` は実行時のグラフに乗らない。`world` の上に位置し、
GeoTIFF デコーダ（`tiff`）を抱えるのはこのツールだけ。**実行時クレートが
tilegen に依存してはならない**（デコーダが実行時に載ってしまう。ADR-0003）。

**禁止事項（レビュー自動失格）**
- `core` / `fdm` / `world` / `sim` / `tilegen` / `net` の `Cargo.toml` に `bevy` を追加すること
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
- Hardware input is sampled per render frame; rate-dependent pilot state updates
  and effective-control recording run only in `Simulation::advance_with_controls`
  immediately before each executed fixed step, with its pre-step state. No-step
  frames and frozen states create no input/recording steps. Crash/divergence ends
  the current frame at once; reports and physical time count only executed steps,
  and the remaining terminal frame budget is discarded (ADR-0004).
- Pilot-owned lateral trims start at zero and hold only explicit J/L, U/O input;
  Shift selects fine adjustment and K resets both. Fixed-step proposals include
  those settings and record the effective surfaces. Zero trim bypasses new
  arithmetic; focus/map/pause release retains settings and new flight/restart
  resets them. No profile schema or automatic torque compensation is added.
- Replay aircraft identity includes the FDM model revision as well as configuration.
  New legacy-aircraft app recordings use complete identity in v3. Old-physics identities remain
  rejected; v1/v2 partial evidence requires explicit supported-baseline opt-in and
  a persistent missing-yaw notice. No format, duration or stored identity is upgraded.
  Modeled-weather v3 restores exact validated parameters and executed simulation
  time before render preparation. Weather CLI and manual replay cloud overrides
  are blocked before startup mutation. Live manual cloud overrides visibly disable recording/F9;
  see [the replay policy](docs/replay-identity.md).
- Opt-in exact profile-v2 dry jets use an app-owned typed flight session and
  [replay v4](docs/replay-v4.md). Live owns one transactional `JetSimulation`
  and recorder; playback owns one `JetReplayPlayer` and reads its simulation.
  No legacy propeller configuration is synthesized. Fixed-step control proposals
  and parking toggles commit only after a successful whole step. Unsupported
  live starts and regional sources fail before replacing the current flight;
  explicit terminal-at-zero v4 evidence remains reproducible. Source support is
  limited to bundled global or matching flat-zero terrain. Recording errors
  close an authentic exportable prefix without freezing live flight. A live
  visual-rate change also closes recording because v4 stores one initial rate.
  New flight/restart starts a new recorder; formats and old physics remain
  unchanged. See [native jet integration and limits](docs/jet-native-app.md).
- The additive running-turboprop foundation owns 16 physical scalars: rigid body,
  turbine response, relative shaft rate and blade pitch. Its separate FDM law
  commits only after every bounded substep and endpoint succeeds; no hidden
  governor history is permitted. [Exact profile v3](docs/aircraft-profile-v3.md)
  and schema-3 identity preserve that model's full immutable configuration.
  Its separate pure host and [replay v5](docs/replay-v5.md) preserve complete
  endpoints, controller/clock/contact history and exact report provenance.
  Seek reconstructs from frame zero within 240 attempts, including terminal
  probes. Explicit external profile-v3 app selection owns a typed live simulation
  or one full-state v5 replay player. The body-only render bridge does not discard
  engine state. No turboprop preset is exposed until authored qualification;
  commercial staging remains Swift-only. Read-only presentation and explicit
  lateral trim are described in [app integration](docs/turboprop-native-app.md).
  Existing replay v1–v4 never substitutes for full-state recording. See
  [ADR-0018](docs/adr/0018-bounded-running-turboprop.md) and
  [ADR-0019](docs/adr/0019-turboprop-full-state-replay-v5.md).
- An explicit pure-FDM `turboprop::near_static` API adds authored law 2 with
  stored negative J rows and fixed adverse/transverse induced-velocity limits.
  It retains the complete law-1 forward model and all 16 physical state scalars.
  It does not widen profile v3, identity schema 3 or replay v5.
  See [ADR-0020](docs/adr/0020-near-static-turboprop-law.md). Independent pure-law
  review is complete; aircraft/preset qualification remains separate from explicit app admission.
- [Profile v4](docs/aircraft-profile-v4.md) explicitly selects near-static law 2
  through a concrete original-token loader. Propeller wrapper schema 2 retains
  the complete schema-1 forward component, two stored negative rows and required
  fixed semantic/domain commitments. Physical identity schema 4 contains the
  full retained forward canonical bytes plus all extension bits. Old profile
  and replay gates stay unchanged. The separate near-static host and
  [replay 6](docs/replay-v6.md) preserve all 16 scalars and complete canonical
  report provenance, with their own closed negative-flow terminal wire tags.
  Unknown identities can be inspected/exported but exact reproduction requires
  schema 4/law 2 and matching configuration/data. Bounded frame-zero seek and
  prospective +dt weather admission retain the accepted v5 mechanics without
  changing that old format. See [ADR-0023](docs/adr/0023-near-static-full-state-replay-v6.md).
  Schema validation, physical construction and pointwise runtime admission are
  distinct. See [ADR-0022](docs/adr/0022-near-static-profile-and-identity.md).
- Explicit external profile-v4 app selection owns a typed near-static live
  simulation/v6 recorder or one complete-state v6 player. Its read-only
  presentation uses the committed clock and separate law-2 support check.
  Candidate source/weather/control/model/audio commits reuse generation-checked
  transactions; failed/canceled starts retain the active flight and trims.
  Profile-v3/law-1/v5 interpretation remains separate. No near-static/Cedar
  preset or commercial staging admission follows. See
  [ADR-0024](docs/adr/0024-near-static-app-sessions.md).
- ストリーミングは1フレームの処理量に上限を持つ（フレームスパイク防止）。
- 補間は描画のみに影響し、物理状態を書き戻さない。
- 乱流の時刻は実行した各固定ステップで進める。描画フレーム末尾の時刻を
  まとめて使わない。同一ビルド・同一入力列で cadence の違いを回帰検査する。
- app のリプレイは frame-zero 状態から開始し、後退時は最大 240 記録フレーム/更新で
  再実行して端数・物理時計・飛行記録を復元する。state-only keyframe への代入を
  完全な巻き戻しと見なさない。描画時計も再生時間へ同期する。
- コックピットカメラは当該フレームの機体描画 Transform 更新後に追従する。
  chase / free の平滑化履歴は origin 変更や restart / rewind 時にリセットする。
  tower は地面で支えた world anchor を保ち、LOD の観測点も実カメラ位置を使う。
- replay reader / writer は frame duration・操作量・keyframe・condition の有限性と
  数値領域を検査する。正しい format-v1 bytes と zero-dt は保持し、不正値の丸め直しをしない。
  app も in-memory record、再現状態・drift、未記録 epoch の解決と visual-time 積を防御する。
  初期状態と step 後の座標は有限なだけでなく、f32 描画座標・LOD の距離計算で有限に
  保てる領域かも確認する。異常時は最後の正常な world Transform を保って停止する。
  再生の停止・seek・fault・終了中は音を停止し、live input が記録条件を上書きしない。

---

## 6. ワールドデータ

### Local regional packages (schema v1)

`flightsim-content` sits above `world`/`core`, with no simulation, Bevy or raw-data
converter dependency. It accepts prepared local ZIP packages containing declared,
size/SHA-256-verified FSDM and inert license/provenance text. It never executes
package code, converts repositories, or activates a region. Strict portable paths,
file/count/inflation limits and runtime-reader validation precede an atomic,
non-overwriting version install. See [package format and lifecycle](docs/content-packages.md).

An opt-in `downloads` feature adds explicit public GitHub prepared ZIP acquisition
with mandatory archive SHA-256, bounded HTTPS/redirect/address policy, a separately
locked verified offline cache and the same strict staging API. The app's separate
opt-in `region-downloads` feature enables map Regions **Installed / Downloads**
views using `--region-catalog FILE.json` and optional `--region-cache DIR` /
`--region-offline`. The local schema-v1 catalog is bounded to 256 KiB and 64 records;
each has canonical ID/version, title, geographic bounds, an allowed prepared GitHub
ZIP URL, mandatory archive hash and inert declared provenance. No real-area catalog
or reviewed terrain source is shipped. Row preview centers the map but cannot
start acquisition; Download/Retry (F) and Cached only (C) are explicit operations.

App serializes download, local import, refresh and inspection on one worker, and
checks the staged manifest's ID/version/title/bounds against the selected catalog
record **before commit**. Refresh clears a changed catalog selection; workers use
the selected snapshot. Cancellation invalidates late UI results, not a completed
cache publication or atomic install. No automatic request, retry, installed
selection or activation occurs. A user must choose Installed and then Start.
Hashes provide integrity, not trust/rights, and declared bounds do not establish
complete tile coverage. Default content/app and existing commercial-candidate
feature sets retain the offline regional-content dependency graph. World/sim/FDM
and `flightsim-net` remain independent. See [catalog and download lifecycle](docs/content-downloads.md)
and [ADR-0015](docs/adr/0015-public-prepared-package-downloads.md).

App may activate one fully inspected immutable package only when creating a new
flight; import/selection/cancel never mutate a current flight. The package TileSource
retains global fallback through the existing world composition and verifies declared
hashes at runtime reads. Supported replay formats cannot represent regional identity and
must be explicitly blocked for package-backed flights. Their bytes are unchanged;
a future format must record and verify package ID, version and exact manifest hash.

### Offline global baseline and regional climate (2026-10-02 branch)

[ADR-0011](docs/adr/0011-offline-global-terrain-climate.md) adds a compact, complete
world atlas beneath all local DEM levels and an offline monthly climate atlas.
`world` owns validated immutable source data; `sim` alone connects regional
temperature to the FDM; `render` applies procedural climate-derived surface cues;
`ui` presents a data-only modal world map; `app` coordinates explicit new flights.
Neither the map preview nor render LOD mutates physical terrain or climate.

The map's measured scroll body stacks its map/sidebar at narrow or short logical
viewports. Credits, region/download controls and wind use their own bounded
scroll surface; only the active modal consumes wheel input. Close resets map
scroll, while child dismissal restores the map's place. Text sizes and the
app-owned Start/Cancel and replay-lock contracts are unchanged. See
[small-window map QA](docs/qa/world-map-small-window-2026-10-04.md).


Map aircraft selection is an app-owned staged new-flight transaction
([ADR-0016](docs/adr/0016-transactional-new-flight-aircraft.md)). It snapshots the
validated target profile, destination/month, weather and regional selection;
scene/dependency/spawn/fit readiness precedes an exclusive complete session,
recorder, controls, aircraft hierarchy, camera/guidance/HUD and synth-source commit.
Generation invalidation prevents canceled or superseded asynchronous work from
activating. UI carries bounded choices only; replay remains aircraft-locked.
Existing CLI defaults, profile/replay schemas and distribution allowlists stay
unchanged. Native acceptance is separate from GPU-free loader/ECS evidence.
Replay v2 records enabled data fingerprints and a fixed climate phase, while v1
preserves legacy terrain/ISA behavior and byte compatibility.

Global relief is roughly 20 km, not globally detailed 30 m terrain. NOAA
1991-2020 monthly reanalysis is climatology, not live weather. Full sources and
limits are in [global terrain](docs/global-terrain.md) and
[global climate](docs/data/global-climate.md). Local `.fsdem` data retain priority;
missing primary reads and generated fallback tiles share the existing frame
budgets and bounded caches. When no regional tile path is configured, an immutable
`EmptyTileSource` explicitly guarantees that primary reads can never succeed.
The renderer and physical sampler can skip those reads while retaining cached
primary precedence; configured directories and mutable sources remain discoverable.
LOD can use a local terrain-height reference so
elevated airports are not incorrectly treated as kilometres above ground.

Global fallback polar shading uses a fixed 505 m physical half-stencil from the
already validated atlas, only when the selector identifies a fallback read.
It changes visual normal attributes before bridge boundary extraction, never
positions, slopes, palette inputs, DEMs, physics or replay identities. Filtering
is complete in the inner 75% of each canonical source cap and fades to the original
normal at its boundary. It intentionally suppresses some retained angular relief;
outside-cap vertices are unchanged, but coarse facets can interpolate pole shading
farther out. Regional/primary DEM normals stay unchanged. The bounded four-sample
policy and its CPU/visual acceptance limits are recorded in
[polar visual normals](docs/qa/polar-visual-normals-2026-10-03.md).

The new branch implementation and its eventual test/runtime evidence are distinct
from the still-unmerged alpha21 release/publication state described below.

Mixed-LOD/source boundaries use render-only bridges between the actual post-f32
edge polylines, including shared T/cross-junction caps and polar fans. DEM height
error alone is not a bound for curved-Earth chord gaps. Topology planning advances in at most 1,024 deterministic work units per update,
without source snapshots or background tasks. Its ordered indexing, sweeps,
corner/polar processing, descriptor queueing and normal scratch cleanup are
incremental. Existing cut assembly, atomic commit and explicit reset cleanup
remain synchronous; this is not a frame-time guarantee. Bridge and surface
preparation share the existing mesh budget; a pending bridge transaction pauses
selection and preserves the old displayed cut until surfaces and bridges can
commit together. Same-ID source replacement preserves the old entity until that
commit. New-flight reset drains pending, visible and retired terrain assets.
Exact ground overlays stage replacement mesh assets across updates under the
same remaining mesh-attempt allowance; old overlay handles and the old cut stay
visible until one atomic handle/visibility commit. Uploads target 65,536 actual
vertices per update, allowing one indivisible first mesh up to the existing
524,288-vertex cap. Generated assets use ownership-only children so original
scene-owner handles remain sufficient for cleanup. Cancellation and retired
meshes are reclaimed synchronously; these are work/residency bounds, not latency
or total-process-memory guarantees ([overlay upload QA](docs/qa/terrain-overlay-upload-budget-2026-10-03.md)).
The selector retains its 8,192-ID bound; one transition may additionally retain
one frame's outgoing surface batch (at most 8,192), and at most two bridge sets
of 4×8,192 meshes each. Compact boundaries and logical geometry bytes are
observable separately from DEM cache usage. Details and verification limits are
in [mixed-LOD stitching QA](docs/qa/terrain-mixed-lod-stitching-2026-10-02.md) and
[incremental planning QA](docs/qa/terrain-seam-planning-2026-10-02.md).

Forward terrain tiles and their bridges opt into perspective-centroid
interpolation for world position, world normal and vertex color. This prevents
MSAA sample coverage on subpixel triangles from extrapolating shading at an
uncovered pixel center. Other materials and prepass/deferred interfaces retain
their original interpolation; geometry, coverage and physical data are unchanged.
See [terrain centroid interpolation](docs/adr/0031-terrain-centroid-interpolation.md).

One-shot screenshots retain their minimum delay and 30-frame floor, then require
CPU world readiness after Update commands and PostUpdate visibility/transform
propagation. The last sampled live/displayed cut must match exact tile IDs with
no pending bridge or overlay transaction. Raw desired-ID convergence admits it;
otherwise an opt-in, positive-budget selector observation must find every current
dependency path examined and no cached mesh preparation or capacity deferral.
Known missing/failed reads may continue their ordinary retries without starving
capture, including 4,094-leaf polar cuts. This preserves valid coarse
primary ancestors, capped global fallback and sparse/empty availability cuts.
Capture-only metadata adds bounded traversal of existing dependency/attempt state
without repeating LOD selection or reading sources, and is disabled once capture
is requested.
The ordinary selector retains only a nullable observer pointer (one machine
word); the original fallback retry map keeps its u64 values. Fallback outcomes
are allocated only in the enabled observer, pruned to the same active tree and
cleared on frame wrap or selection-state replacement. Late observation treats
unavailable earlier outcomes as unknown until their ordinary retry is observed.
An active runway within the existing 15 km airport vicinity must have a visible
propagated surface, including precision gating. Worldwide free-flight starts
may retain a distant synthetic runway; its intentional hiding does not block capture.
Optional omitted/precision-hidden scenery does not block capture. The same
committed surface entities, overlay revision, runway mesh, model entities and
stable render origin must be observed on consecutive frames, allowing a prior
extraction/render-preparation opportunity. This is CPU readiness, not a GPU or
shader completion guarantee. Signature work runs only while a screenshot is
requested and unfinished; simulation, render budgets and capture acceptance
timeouts remain unchanged. Native screenshot sessions additionally bound uncompleted render-frame
batches to two using queue-completion credits around the complete Render
schedule; ordinary launches install no gate. See [ADR-0029](docs/adr/0029-capture-render-backpressure.md).
This is backpressure, not a GPU readiness or deadline guarantee.
Native batch capture additionally defers unfinished view draws while every normal
Main/extraction/Render preparation, upload and cleanup still runs. It preserves
the 30-Main-update floor and original work budgets, then requires a prior admitted
same-scene Render opportunity before Screenshot. SortedCameras is restored before
cleanup and on render unwinding; ordinary/non-batch launches keep their path.
This is not a claim of 30 full-scene draws, GPU completion or native acceptance.
See [ADR-0030](docs/adr/0030-batch-capture-preparation-admission.md).


ソースは全てオープンデータ（[ADR-0003](docs/adr/0003-terrain-data.md)）。

| 種別 | ソース | ライセンス |
|---|---|---|
| 標高 | Copernicus DEM GLO-30 (全球 30m) | 無償・再配布可 |
| 空港・滑走路・建物 | OpenStreetMap | ODbL |
| 地表画像 | Sentinel-2 / Natural Earth | 無償 |

タイル分割は **地理座標系クアッドツリー**（level 0 = 経度方向2タイル × 緯度方向1タイル）。Cesium の geographic tiling scheme と同一にして、既存タイルセットとの互換を保つ。

LOD は幾何誤差ベースの screen-space error で選択する（距離ベースではなく）。理由は山岳と平野で必要ポリゴン数が桁違いに違うため。

Visual draw-distance settings additionally bound the geographic region that may
refine beyond coarse terrain. `world::draw_distance` provides validated Short,
Standard and Long policies; unindexed sources preserve the existing SSE-only cut.
Short caps subdivision outside 10 km after level 6, retaining complete planetary
coverage, while Long requests more local detail under the same leaf, cache,
load, mesh and scenery limits. Scenery query radius, regional terrain-detail
radius and camera far plane are distinct controls. App replaces the actual LOD
selector and cancels stale scenery generations before upload; physical DEM
sampling and replay identity never read this visual policy. See
[draw-distance policy](docs/draw-distance.md) for values, bounds and integration.

描画の SSE 選択は要求精度であり、実タイルの存在を意味しない。欠落・読込失敗時は
予算内で祖先を探し、非表示でメッシュを準備してから、非重複の cut を一括表示する。
全読込試行（欠落・失敗を含む）とメッシュ準備（cache hit を含む）は、それぞれフレーム
予算以内。必要な子メッシュが全部揃うまで親を保持し、DEM cache eviction では既存・
準備済みメッシュを消さない。新規メッシュにも現在の RenderFrame による Transform を
即座に設定する。要求が粗くなったのにそのデータが無い場合は既存の細かい表示を残す。
可視・非表示を合わせた resident mesh は最大 8,192 とし、遠い保持済み子は近い新規描画へ
場所を譲る。現在の選択木全体がこの上限に収まる設定だけを許容し、置換待ちの deadlock を
防ぐ。履歴は現在の選択木内で保持・再試行し、古い非表示準備は破棄する。
これは描画だけの処理で地面 sampler や物理へは書き戻さない。供給元の任意 coverage index
がある場合のみ、局所範囲で最も粗い実タイルへの経路を要求する。未索引の子タイルを探索する
機能ではない（[親 fallback QA](docs/qa/terrain-parent-fallback-2026-10-01.md)）。

Regional DEMs can begin below the SSE cut (for example L10 data under an L9
high-altitude request). `PrimaryCoverage` indexes only the strict ancestor paths
to the coarsest declared primary tiles. Within the current local-detail radius,
render selection follows these paths before resuming SSE, under the same maximum
level, hard radius and leaf limits. Package hints come from validated metadata;
raw-directory hints are optional bounded filename snapshots at render-source
preparation, never per-frame I/O. Hints neither attest payloads nor rule out later
primary reads. Failure discards the complete raw hint with a diagnostic, retaining
ordinary SSE/ancestor fallback. The sampler, replay and DEM bytes do not change.
See [regional coverage decision](docs/adr/0028-bounded-primary-terrain-coverage.md).

実行時タイル形式 `.fsdem` は `u16` 量子化 + タイル毎スケールの自前バイナリ（[ADR-0005](docs/adr/0005-runtime-tile-format.md)）。焼き込みは `flightsim-tilegen` が行う。

OSM の空港設備は、利用者が用意した地域 PBF から `flightsim-airportgen` が
滑走路・誘導路中心線、apron polygon、待機位置、地上灯火と必要な文字列属性を
section-directory 形式の `.fsairports` FSAP v3 へ焼く。reader は既存の FSAP v1 / v2
bytes も引き続き読み、writer も v1 / v2 の byte 互換性を維持する。v3 は payload
集約済み文字列と参照の展開量はそれぞれ 16 MiB、payload は 96 MiB、固定長
record 合計は 1,000,000 を上限とし、section の順序・
範囲・schema・flags・record size・checksum・末尾を確保前に厳格検査する。

実行時は PBF デコーダに依存せず、開始地点から ECEF 距離が最小の滑走路を選び、
その中心から 15 km 圏と交差する誘導路・apron、および圏内の待機位置・灯火だけを描く。
apron の各三角形頂点、誘導路の各 node、待機位置・標識・灯火で DEM を引く。surface は
apron → 誘導路 → 滑走路の順に lift を上げ、各路面標示と灯火にも固定 lift を割り当てて
重なりを決定論的にする。
元 PBF と派生 DB は同梱しない（[ADR-0008](docs/adr/0008-osm-airport-data.md)）。

Copernicus DEM GLO-30 の EGM2008 標高は、利用者が別途用意した対応する
GeographicLib 16-bit PGM グリッドで **`h = H + N`** として元の有効 pixel centre ごとに
WGS84 楕円体高へ変換し、その後に再標本化する。EGM96 も同じ経路を持つが、
入力 datum と model は必ず一致させる。unknown / 非対応 datum、CRS・単位の不整合は
CLI と library の両方で拒否する。提供元仕様による欠落 datum の明示宣言と、
値を変えない `--assume-ellipsoidal` は別の選択肢であり、変換の代用にしない。

`.fsdem` の byte 形式は変えず、WGS84 楕円体高 m の契約を守る。runtime に geoid は要らない。
各出力は同じディレクトリの一時ファイルから原子的に置換し、provenance は invocation の
開始前に INCOMPLETE、全成功後に COMPLETE とする。旧タイルを自動変換せず、今回の
対象外ファイルを attestation しない。ディレクトリ全体の transaction ではない
（[ADR-0005](docs/adr/0005-runtime-tile-format.md)、[検証記録](docs/qa/data-boundaries-2026-10-01.md)）。

PBF は root `vendor/osmpbf` の安全修正版 0.3.7 が算術・index・UTF-8・enum・配列・
圧縮ストリームを検証してから iterator へ渡す。敵対的 fixture の通常エラーと正常入力の
byte 互換性を検査するが、ファイル全体の索引・参照 node・apron 集約の総メモリは
sandbox 化していない（[ADR-0008](docs/adr/0008-osm-airport-data.md)）。

滑走路の舗装・標示は共有の最大 10 m グリッドで各頂点を DEM に沿わせる。灯火も
角ごとの標高と垂直面を持つ。renderer は app からの標高 callback のみを使い、
自ら地形と FDM を結線しない。描画 LOD と地面 sampler の三角形差による遮蔽は
残り得る（[滑走路 QA](docs/qa/runway-terrain-drape-2026-10-01.md)）。

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

The local [Balzers terrain sample](docs/examples/terrain-packages/balzers/README.md)
adds a reproducible 170-tile L10–13 prepared ZIP and a GUI-independent content
validation/import example. It reuses the existing package reader and immutable
store; it does not alter runtime activation, download sources, replay formats,
terrain sampling, source attribution or release gates.

**2026-10-01 の統合ソースの状態。** 実装と検証範囲、配布状況は分ける。
公開・CI の時点付き記録は [統合 QA](docs/qa/overnight-status-2026-10-01.md) を参照。
実装済みでないものを「ある」と書かないこと。

### 実装済み

| クレート | 内容 |
|---|---|
| `flightsim-core` | 単位型、WGS84 測地系、ECEF/NED/ENU 変換、floating origin、固定ステップ、描画座標フレーム |
| `flightsim-fdm` | ISA 標準大気、WGS84 正規重力、6DoF、失速、プロペラ推力、3 点式着陸装置、接地摩擦・ブレーキ、RK4、定常風と決定論的乱流 |
| `flightsim-world` | 地理座標系クアッドツリー、DEM、SSE-LOD、予算制ストリーミング、LRU、`.fsdem`、スカート付き地形メッシュ、合成滑走路、`.fsairports` v1/v2/v3 の厳格検証と最寄り滑走路選択 |
| `flightsim-tilegen` | 厳格な GeoTIFF の基準・単位検査、ローカル geoid 正規化、原子的タイル保存と provenance、検証付き OSM PBF → 空港 DB |
| `flightsim-assetgen` | `.env` から鍵を安全に読み、Meshy から glTF / glb を取得するオフライン CLI |
| `flightsim-sim` | 地形と FDM の結線、固定ステップ、滑走路中心線を追うフライトディレクタ、場周飛行、進入初期化、軌跡・着陸・飛行記録 |
| `flightsim-render` | 地形・滑走路・誘導路・apron・待機位置標示・ASCII 物理標識・滑走路/誘導路灯メッシュの GPU 投入、LOD 描画、floating origin、大気散乱、時刻・太陽、決定論的な雲層と雲中視程、glTF の軸・倍率補正 |
| `flightsim-input` | 複数 controller、校正・軸/ボタン再割り当て・JSON 保存、opt-in native channel、キーボード共存、trim、視点切替・追従カメラ |
| `flightsim-ui` | HUD、丸形 mask 付き姿勢計、操作説明・チュートリアル、記録・着陸評価・帰属、一時停止・墜落・リプレイ状態、入力診断、交通識別表示 |
| `flightsim-audio` | 出力に連動するエンジン音、対気速度に連動する風切り音、迎角で鳴る失速警報 |
| `flightsim-net` | 決定論的な合成交通、補間、bounded UDP の作成・参加・退出・再接続 |
| `flightsim-app` | 上記の統合、合成飛行場または OSM の最寄り滑走路と 15 km 圏の地上設備、風・乱流・時刻・雲層・着陸練習、2 機体 profile、難易度、同一 build replay、windowed/offscreen capture CLI |

HUD instruments and the notice/help/log column share one measured body above
the wrapping attribution footer. Small windows retain flight-control keys in a
compact reference, with complete aircraft guidance available in the existing
paused state. Instrument, notice and help font sizes remain unchanged. See
[ADR-0021](docs/adr/0021-measured-flight-help-layout.md).

雲描画は独立した Off / Light / High / Ultra 設定を持つ（F3、Shift+F3 で Light）。
Light は従来の 2 枚の PBR 平面・256² マスク・雲中 fog を保ち、マスクの雲量を
seed ごとに面積校正する。High/Ultra は同じ Earth-space 密度場と単一の曲面層を使い、
予算内の ray integration と不透明物体の深度に沿う HDR 合成を行う。
描画側の同一フレームの readiness gate が平面・影・fog と volume を切り替える。
詳細と予算は [ADR-0013](docs/adr/0013-bounded-cloud-quality.md) に固定した。

雲量は現在地の NOAA 月平均再解析値を周辺の一層へ適用する。雲底は出発地の粗い
モデル地表高 + geoid + 1,500 m、厚さは 1,200 m の描画近似で、直下の山に追随しない。
湿度・露点・鉛直安定度は未入力で、観測した雲種や現在の天気を表さない。
Light の位置近似は上位と異なるが、公称雲量・層高・視程は品質で変更しない。
従来の手動雲量・楕円体基準の雲底/雲頂・視程指定を優先し、物理には戻さない。
旧 v1 replay と気候無効時の雲量 0 も保つ。[操作と科学的限定](docs/cloud-quality.md)。

Authored weather is opt-in through render `RenderWeather` (validated sim
`WeatherSelection` plus executed `Seconds`). The host updates it before
`RenderSet::Weather`; the renderer resolves an independent deck so legacy climate
writes cannot overwrite authored settings. Ambient/fog/cloud extinction add;
upper readiness suppresses only the duplicated cloud contribution. Negative
authored layer heights remain valid without relaxing the legacy constructor.
Fog is camera-local homogeneous extinction with smooth layer boundaries, not
a visible distant bank. Rain/snow are one deterministic, bounded, lit opaque
proxy mesh in a fixed departure frame; no flight dynamics change. Off/Light,
High and Ultra cap it at 128/256/384 samples with inverse-cap area weighting.
Legacy/no-precipitation creates no precipitation assets. See
[modeled weather](docs/modeled-weather.md) for exact parameters, budgets,
approximations and the separate app/native validation gates.

New-flight wind/turbulence have an independent bounded map editor. App-owned
exact pending values and override flags are copied only for explicitly edited
fields, before wind-aware airborne construction. The aircraft transaction checks
their exact snapshot and invalidated Start generation through regional/scene
preparation. Visual weather selection never changes forces. Existing physical
laws, seeds, v3/v4 bytes and restart/replay conditions stay unchanged; see
[ADR-0017](docs/adr/0017-explicit-new-flight-forces.md) and
[controls and limits](docs/new-flight-wind.md).

Authored visibility and cloud base have a separate optional map child editor.
An app-owned exact canonical template plus explicitly edited SI values is resolved
once against the prepared departure source, then committed only by the existing
complete-aircraft transaction. Full bitwise draft/revision and Start-generation
checks reject stale Custom choices. Replay v3/v4/v5/v6 retain their existing
resolved weather blocks; codecs, forces and render budgets are unchanged. See
[ADR-0025](docs/adr/0025-authored-visibility-cloud-base.md). Native acceptance and
release remain separate gates.

CI の Windows / Linux で純 Rust 群と描画群の指定テストを実行する。対象は
`.github/workflows/ci.yml` の `HEADLESS` / `RENDER` を確認する。さらに
`clippy -D warnings`、`fmt --check`、依存規約検査、
`cargo doc -D warnings` に加え、Linux の Mesa/lavapipe で同梱 glTF を読み、
スクリーンショットを 1 枚描画する起動スモークを行う。リリース時は Windows zip を
新規ディレクトリに展開し、D3D12 のフォールバックアダプタで検査する。
alpha.20 は exact source の CI・展開後 model / PNG / exit 0 と実公開物の確認が済んだ。
alpha.21 向け workflow は profile JSON・両 GLB・Swift の `.blend` を必須同梱物とし、
Light Single cockpit と Swift Sport chase を個別に起動して 2 枚を検査・公開する。
この後者は未実行の最終配布ゲートで、現時点の公開成功を意味しない。

### 今後の範囲

- OSM の空港建物、衛星地表画像、METAR、観測/予報に基づく多層雲・地表の雲影、推力線オフセット
- 実 ADS-B 交通、Internet 向け認証・暗号化・NAT 越え・マッチメイキング・衝突の権威制御
- 写実的な機種別コックピット。現在は共通の手続き的内装を機体 profile の視点へ合わせる

### 実装済みだが検証を残すもの

- controller は software routing・保存・切断処理をテスト済み。物理機種の入力範囲、
  native code、符号・感度・切断復帰は未確認。native code は OS ごとに異なる
- 乱流は 24 の決定論的数値シナリオ、長時間 severe、global seam を検査済み。
  人による操縦感評価は未実施。旧 seed の大気は変わるため旧乱流 replay に互換保証はない
- 2 機体の離陸と無風・海面付近の 30 秒進入を検査済み。全飛行領域や実機性能の保証ではない
- 実 Copernicus の夜間・約 3 km 落下試験の画像でカメラと滑走路・灯火の不具合を修正。
  360 秒・17.533 km の巡航は数値 fixture に加え、native app の全再生・4 回の実 render-origin
  rebase・完了画像の地形と地平線まで確認。sampled observation は全遷移の動画検査ではなく、
  任意地域・視点・地形 clearance は未網羅。1.5 NM 夜間進入の灯火視認性は未確立で、
  V/S の極端な値は短い k 表記へ修正し、回帰試験と最新 app の実降下画像で収まりを確認済み
- CI の CPU Vulkan と Windows D3D12 fallback は物理 GPU・ベンダードライバ・FPS を
  検証しない。batch 終了修正は alpha.20 で確認済み、追加機能の alpha.21 配布 gate は未完了
- 実 app 2 プロセスの loopback では接続・host 停止/再起動・再参加・退出を確認。
  当初疑った PNG の文字欠けは針と小さい計器文字の重なりと切り分けた。単独実行と native resize
  でも help は読め、独立した欠落不具合は再現しなかった。スピーカー聴感、物理操縦装置、
  複数実マシン LAN / WAN は未確認

詳細は [docs/ROADMAP.md](docs/ROADMAP.md) と [統合 QA](docs/qa/overnight-status-2026-10-01.md)。
