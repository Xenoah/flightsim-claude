# HANDOFF — 次の担当者への引き継ぎ

作成: 2026-08-01（v0.1.0 リリース直後）
更新: 2026-08-30（v0.6.0-alpha.12 = OSM apron・待機位置・標識・誘導路灯）

このプロジェクトは文脈ゼロの担当者が交代で入る前提。**着手前にこの文書を最後まで読むこと。**
ここに書いてあるのは「何をするか」だけでなく「すでに踏んだ地雷」も含む。

交代の都度の申し送りは [docs/handoff-notes/](handoff-notes/) にある。
**最新は [2026-08-30（→ Codex）](handoff-notes/2026-08-30-to-codex.md)。**

---

## 1. 現状を 30 秒で

**M2 達成、M3 を実装中。ゲームとして一周し、雲中の計器飛行も練習できる。**
引数なしで起動すると合成飛行場の滑走路中心線上から始まり、離陸して戻って
降りると着陸が 5 段階で評価される。地域 OSM PBF をオフライン変換すれば、
開始地点に最も近い実在滑走路で同じループを飛べ、その 15 km 圏にある誘導路、apron、
待機位置標示・物理標識、明示または決定論的に補った誘導路灯も地形に沿って表示される。

```bash
# タイルが無ければ先に焼く（実 DEM は要らない。合成地形で動く）
cargo run -p flightsim-tilegen --example synthetic_dem -- data/synthetic.tif
cargo run -p flightsim-tilegen -- --input data/synthetic.tif --output data/tiles     --min-level 8 --max-level 12

# 実滑走路（信頼できる提供元の PBF を利用者が用意。同梱しない）
cargo run -p flightsim-tilegen --bin flightsim-airportgen -- \
  --input data/region.osm.pbf --output data/region.fsairports

cargo run -p flightsim-app --release -- --tiles data/tiles              # 滑走路から
cargo run -p flightsim-app --release -- --tiles data/tiles \
  --airports data/region.fsairports --start 35.55,139.78                 # 最寄りの OSM 滑走路
cargo run -p flightsim-app --release -- --approach 1.5 --turbulence moderate  # 着陸練習
cargo run -p flightsim-app --release -- --difficulty beginner                 # 無風・案内あり
cargo run -p flightsim-app --release -- --difficulty realistic            # 横風・案内なし
cargo run -p flightsim-app --release -- --replay flight-001.fsreplay      # 記録の再生
cargo run -p flightsim-app --release -- --tiles data/tiles \
  --cloud-cover 0.55 --cloud-base 700 --cloud-top 1300 --cloud-visibility 300
```

| できること | 入口 |
|---|---|
| 滑走路から離陸 → 場周 → 着陸 → 5 段階評価 | 引数なし |
| 着陸だけ練習（進入の途中から始まる） | `--approach <海里>` |
| 風 | `--wind 270/10`（方位/ノット） |
| 乱流 | `--turbulence light\|moderate\|severe` |
| リプレイ用の記録を GUI 無しで作る | `cargo run -p flightsim-sim --example record_takeoff -- flight-001.fsreplay` |
| 飛行の保存と再生 | 常に記録中。`F9` で保存、`--replay <FILE>` で再生。再生中は `F5` 停止 / `F6`・`F7` 速度 / `F8` 10 秒戻る |
| 難易度（風・乱流・案内の既定をまとめて） | `--difficulty beginner\|normal\|realistic`。明示した `--wind` / `--turbulence` が勝つ。**採点には効かない** |
| 時刻・太陽位置 | `--time 05:30`（地方平均太陽時）、`--time-rate 60`、実行中は `,` `.` |
| 雲量・雲層・雲中視程 | `--cloud-cover 0.55 --cloud-base 700 --cloud-top 1300 --cloud-visibility 300` |
| OSM の最寄り滑走路と周辺地上設備 | PBF を `flightsim-airportgen` で焼き、`--airports <FILE>` |
| チュートリアル導線 | 既定で出る。`H` で消せる |
| ゲームパッド | 繋げば自動。キーボードと**軸ごとに**共存 |
| 検証用: 空中から落として評価表示を通す | `--drop 15` |

- 機体はテクスチャ付き glb を同梱（引数なしで出る）。`--no-model` で箱に戻る
- M2 の受け入れテストは `crates/flightsim-sim/tests/airport_circuit.rs`
- ワークスペースの全テストを Windows / Linux の CI で実行する。
  加えて lint・ドキュメント・依存規約・ソフトウェア Vulkan 起動を検査する
- `v0.6.0-alpha.6` 以降は、成功した `main` の CI 後に Windows x86_64 の zip を
  prerelease へ自動添付する。実行ファイル、`assets/`、README、変更履歴、帰属・
  ライセンス文を同梱する

## 2. 破ってはいけない制約

**CI が機械的に検査する。** 破ると `scripts/check-architecture.sh` が落ちる。

1. **`flightsim-core` / `flightsim-fdm` / `flightsim-world` / `flightsim-sim` /
   `flightsim-tilegen` に `bevy` を依存させない。**
   これらが GUI なしにテストできることが技術選定の根拠そのもの（[ADR-0001](adr/0001-engine-selection.md)）。
   Bevy を使えるのは `render` / `input` / `ui` / `app` だけ。
2. **依存は一方向。** `core` ← `fdm`/`world` ← 上位。
   **`fdm` から `world` を参照しない。** 地形標高は引数で受け取る。
3. **WGS84 の楕円体定数を `core` の外に書かない。** 座標変換は `flightsim-core` に集約する（[ADR-0002](adr/0002-coordinate-system.md)）。

CI が検査しないが、レビューで落とす規約:

4. **世界座標は `f64` ECEF。** `f32` を位置の正として持たない。
5. **公開 API の物理量は単位付き newtype。** 裸の `f64` を渡さない。
6. **FDM は決定論的。** 壁時計時間・乱数・グローバル可変状態を参照しない（[ADR-0004](adr/0004-simulation-loop.md)）。

---

## 3. 次のタスク

M1（ヘッドレスで妥当に飛ぶ）と M2（1 空港周辺で離陸→旋回→着陸）は達成済み。
**いま M3 の途中。** 雲と雲中視程（[Issue #11](../../../issues/11)）と、
OSM 滑走路（[Issue #21](../../../issues/21)）・誘導路（[Issue #25](../../../issues/25)）・
apron / 待機位置 / 標識 / 誘導路灯（[Issue #27](../../../issues/27)）の取り込みと描画は完了。
TASK-C（難易度設定）も完了。**M3 で実装できる項目は残っていない**——
残りはコックピット内装（モデル調達待ち）と、実機・実データ・人の判断が要る検証だけ。

M4 に入っていて、リプレイ（[Issue #12](../../../issues/12)）は完了。
残りは機体追加（[#10](../../../issues/10)。モデル調達が要る）、
METAR、ライブ交通（[#13](../../../issues/13)）、
オンライン共有ワールド（[#14](../../../issues/14)）。
継続課題は [ROADMAP](ROADMAP.md) に記録している。

### TASK-A: 計器盤（`flightsim-ui`）— 完了

2026-08-30 完了。コックピット視点に対気速度・姿勢・高度・昇降・方位・出力の
丸型 6 計器を置いた。外形モデルは視界を塞ぐので隠してあり
（`ExteriorModel` + `update_model_visibility`）、内装の 3D モデルはまだ無い。

- 角度から針への変換は Bevy 非依存の純関数として検査する
- 針の中心ずれと操作説明との重なりは実際のスクリーンショットで発見して修正した
- 太陽高度に連動する照明も実装済み。**alpha.8 タグは計器盤までで、照明は alpha.9**

### 天候: 雲と視程（`flightsim-render` / `flightsim-app`）— 完了

- `--cloud-cover`（0〜1）、`--cloud-base` / `--cloud-top`（楕円体高 m）、
  `--cloud-visibility`（m）で雲層を設定する。既定は雲量 0 の快晴
- 固定 seed の周期的な 2D value/fBm noise なので、同じ時刻・位置・設定なら同じ雲場になる
- 雲底・雲頂は alpha mask 付き PBR 平面。雲中だけ distance fog を使い、
  大気散乱の `ClearColor` は変えない
- これは計器飛行を成立させる最小実装。高品質なボリューム雲と METAR は後続

### TASK-B: 空港データ — 完了

2026-08-30、[Issue #21](../../../issues/21) で `aeroway=runway`、
[Issue #25](../../../issues/25) で `aeroway=taxiway`、[Issue #27](../../../issues/27) で
apron・待機位置・標識・誘導路灯まで完了。

- `flightsim-airportgen` が地域 `.osm.pbf` をオフラインで `.fsairports`（FSAP v3）へ焼く。
  生 PBF と派生 DB は同梱しない（[ADR-0008](adr/0008-osm-airport-data.md)）
- 滑走路は中心線 way の先頭・末尾 node から方位と長さを作る。幅欠落・不正は 45 m、
  面形状・端点欠落・縮退は理由別に除外する
- 誘導路は `area=yes` を除外し、閉じた中心線を含む全 node を OSM 順に保持する。
  幅欠落・不正は 15 m。node 欠落・不正座標・縮退が一つでもあれば way 全体を除外する。
  `ref`・`surface`・灯火 metadata も保持する
- `width` は滑走路・誘導路とも数値、`m`、`ft` を扱う
- apron は閉じた `aeroway=apron` way と hole 付き multipolygon を扱う。ring は member way
  を OSM ID 順に反転も含めて接続し、hole を保って三角形分割する。各三角形辺は最大 75 m
- 待機位置は `aeroway=holding_position` の node / way と、
  `aeroway=aerodrome_marking + aerodrome_marking=holding_position` の way を扱う。
  種別は `holding_position:type` を正典として従来 tag より優先する。有効な明示停止線 way が
  holding node を member に持つ場合は way の geometry・幅・source を優先し、不正 way なら
  node fallback を残す。近接距離だけでは統合しない
  node は誘導路 node と共有されず、幅 + 1 m の corridor 内にも候補が無い場合に除外する。
  候補が複数なら距離、way ID、segment index の順で決定論的に選ぶ
- 明示灯火は `aeroway=navigationaid + navigationaid=txe|txc|rgl` の node。
  明示点の無い channel は誘導路 metadata から決定論的に補うが、`lit=no` は補完しない
- 入力 PBF と出力 DB が同じ実ファイルなら hard link 経由でも変換前に拒否する。
  出力は同じディレクトリで完全に書いて同期した一時ファイルから原子的に置換する
- FSAP v3 は 24-byte header と 32-byte directory entry を使う section 形式。section 1〜7 は
  core v2 record / apron triangle / holding / ground light / taxiway attribute / string index /
  string bytes。v1 / v2 の reader・writer と byte 表現はそのまま互換
- reader / writer の上限は固定長 record 合計 1,000,000、集約済み文字列と
  参照の展開量がそれぞれ 16 MiB、payload 96 MiB。
  section の kind 順・一意性・連続範囲、schema、flags、record size、件数積、予約領域、
  全 payload checksum、末尾データを厳格に検査し、不正 DB は読み込まない
- app は `--start` から ECEF 中心距離が最小の 1 本を選び、開始・進入・描画・
  灯火・着陸評価で共有する。`--start` 省略時は選んだ滑走路上へ自動配置する
- active runway の中心から 15 km 圏と線分・三角形が交差する誘導路・apron、および圏内の
  待機位置・灯火だけを描く。各 geometry 点で DEM を引き、surface は apron → 誘導路 →
  滑走路の順に lift を上げ、路面標示・灯火にも固定 lift を割り当てて z-fighting を避ける
- 滑走路側を判定できる待機位置は 2 実線 + 2 破線。待機位置と関連誘導路の `ref` が
  揃えば、中心線右側へ 3x5 glyph の物理標識を置く。**画面文字は ASCII のみ**で、
  非 ASCII、8 文字超、未収録 glyph は DB に保持しても標識へ描かない。盤面の winding・
  法線・文字 lift は接近側へ揃え、片面 culling でも正面を読める
- OSM 滑走路を実際に選んだ場合だけ、画面右下に ASCII の帰属を表示する。
  詳細は `ATTRIBUTION.md`。PBF も派生 DB も ODbL 由来で、公開時は share-alike を確認する
- alpha.10 では Haneda の実 PBF を滑走路 9 本 / 456 bytes へ変換し、2 回の SHA-256
  一致と実滑走路・帰属表示を目視済み
- alpha.11 では Haneda 小領域 PBF（13,450 bytes）から滑走路 3 way、誘導路 113 way /
  1,023 segment を 65,688 bytes へ変換。2 回の出力 SHA-256
  `B29B8E599ABA81AFDBD130F4C2DC14E49382F47FF0914D7042A4594080C74D09` が一致し、free view の
  `--screenshot` で舗装・黄色中心線・曲線・junction・滑走路との重なり・帰属を目視済み
- alpha.12 では Haneda 小領域 PBF（75,393 bytes、SHA-256
  `075B94E8723336A8C1B32B271DE2EF3944717E5A60B98F34DC26A2264257B6B2`）から滑走路 3、
  誘導路 113 / 1,023 segment、apron 3 / 2,940 triangle、待機位置 16、明示灯火 0 を
  258,151 bytes へ変換した。2 回の出力 SHA-256 は
  `93ADF41982F15F26896FAF19F1BBB0CC3024AE15DF4C925E9371C476495DD87B` で一致し、readback
  件数も一致。待機位置 node 19 + marking way 13 のうち、誘導路へ関連しない node 3 件を
  除外し、way と node を共有する 13 件は明示 way へ統合した。昼の free / cockpit view と
  夜の free view で apron、単一の停止線、`34L-16R` / `A11` 標識、灯火、OSM 帰属を目視済み。
  この extract に無い multipolygon hole と明示灯火は合成 fixture で検査する

空港名・滑走路 `ref`・建物は未実装。OSM の `surface=*` は表示用の限定した列挙へ写像し、
未知値は `Unknown` として保持する。PBF parser 自体の hardening は Issue #23 のまま。

### TASK-C: 難易度設定 — 完了

2026-08-31 完了。`--difficulty beginner|normal|realistic` が風・乱流・
チュートリアル案内の既定をまとめて決める（`Difficulty` in `flightsim-app/src/main.rs`）。

| | 風 | 乱流 | 案内 |
|---|---|---|---|
| `beginner` | 無風 | 無し | 出す |
| `normal`（既定） | 無風 | light | 出す |
| `realistic` | 滑走路に斜め 40 度・12 kt | moderate | 出さない |

**着陸評価の閾値は難易度で変えないと決めた。** この引き継ぎノートの旧版は
「閾値も変えられる」と書いていたが、変えなかった。甘くすると同じ操縦に違う点が付き、
上達したのか設定を下げただけなのかが分からなくなって、点が意味を失う。
沈下率 1 m/s の接地は誰が出しても 1 m/s。

**難易度は既定値を決めるだけで、`--wind` / `--turbulence` を打ち消さない。**
ここが逆だと明示指定が黙って無視され、効かない理由を掴めない。
明示指定が生き残ることを検査で固定してある（`an_explicit_wind_survives_the_difficulty_preset`、
`an_explicit_calm_survives_the_hardest_preset`）。触るときは壊さないこと。

### リプレイ（`flightsim-sim::replay`）— 完了

2026-08-31 完了。**踏んだ地雷を 2 つ残す。**

1. **読み込み側で回転を無条件に正規化してはいけない。** 既に単位長でも
   割り算の丸めで最下位ビットが変わる。`length()` が厳密に 1.0 になるかは
   環境で違うので、**同じファイルが OS によって別の値に読める**。
   Linux の CI だけで往復の一致検査が落ちて発覚した。単位長からのずれが
   1e-12 を超えるときだけ直す
2. **画面を見ないと分からない不具合が、また 3 件出た。** テストは全部
   通っていた。計器が手元の操縦桿を映していた（機体は加速中なのに
   スロットル 0%）、再生中にチュートリアルが効かないキーを指示していた、
   帯が 1 行に収まらず折り返していた。
   `cargo run -p flightsim-sim --example record_takeoff` で記録を 1 本作り、
   `--replay <FILE> --screenshot <PNG> --screenshot-delay 25` で撮ること

### 確認済みの制限と未検証事項

ここの本文は申し送りのスナップショット。着手可否と完了状態は
[GitHub Issues](../../../issues) が正本で、文書同期は [Issue #15](../../../issues/15) で追跡する。

- ゲームパッドの変換ロジックとキーボード共存はテスト済みだが、
  実機の符号・感度は未確認（[Issue #2](../../../issues/2)）
- 夜間の滑走路灯とコックピット照明は実装済み（[Issue #3](../../../issues/3)）
- フライトディレクタは回帰テストの駆動装置。6 km手前から滑走路中心線を連続捕捉し、
  左右6 m/sの直角横風でも滑走路内へ接地する（[Issue #4](../../../issues/4)）。
  ILS・航法データ・認証されたautolandではない
- 乱流は強度上限・連続性・決定論を検証済みだが、操縦感は未調整（[Issue #5](../../../issues/5)）
- 実 Copernicus DEM を使った夜間・高高度の見え方は未確認（[Issue #6](../../../issues/6)）
- Copernicus DEM GLO-30 は EGM2008 標高だが、tilegen は WGS84 楕円体高へ
  変換せず格納している。地形・接地・滑走路は局所的に揃うが、絶対高度に
  ジオイド高相当の系統誤差が残る（[Issue #22](../../../issues/22)）
- `osmpbf 0.3.7` は細工・破損 PBF の全経路を panic-free にしない。入力は信頼できる
  提供元に限り、parser hardening は [Issue #23](../../../issues/23) で追跡する
- CI の起動スモークは Mesa/lavapipe の CPU Vulkan で同梱 glTF と 1 枚の描画を
  確認する（[Issue #8](../../../issues/8)）。Windows zip もクリーンな展開先から
  D3D12 フォールバックで検査するが、実 GPU、ベンダードライバ、性能は保証しない
- 雲の最小実装（[Issue #11](../../../issues/11)）は完了。高品質なボリューム雲と
  METAR は後続。HOTAS・軸再割り当て（[Issue #9](../../../issues/9)）、
  追加機体（[Issue #10](../../../issues/10)）、リプレイ（[Issue #12](../../../issues/12)）、
  交通（[Issue #13](../../../issues/13)）、オンライン（[Issue #14](../../../issues/14)）は未着手

### リリース経路

`.github/workflows/release.yml` は `main` の **push 由来の CI が成功した場合だけ**動く
（[Issue #7](../../../issues/7)）。
CI が検査した SHA を Windows で `--locked --release` ビルドし、Cargo metadata から
workspace version を読み、`v<version>` タグが無いときだけ作る。同名タグが別 SHA を
指していれば失敗する。zip を新規ディレクトリへ展開し、同梱モデルの読み込みと
完全な PNG を Windows 上で確認してから公開する。release と zip のアップロードは
再実行しても同じ結果になる。

書き込み権限は、ソースを checkout も実行もしない publish job だけが持つ。
build job は読み取り権限で、両 job 間の zip は SHA-256 を照合する。
`agent/` branch の same-repository PR が merge された場合は、PR の head SHA から
ref が変わっていないことを確認し、PR コードを checkout せずに branch を整理する。

---

以下は完了済みのタスク記録。**設計判断の経緯が書いてあるので、
似た作業をする前に該当箇所を読むこと。**

### TASK-1: 接地反力と着陸装置（`flightsim-fdm`）— 完了

2026-08-03 完了。小さく、自己完結し、フライトシムの中核ループ（離陸→着陸）を開けた。
平坦な合成地形でテストできるので、実データを待たずに進められる。

**やること**

- `Environment` に `ground_elevation: Meters` を追加する
  - v0.1.0 で意図的に入れなかった。「今ないものを設計しない」方針で、
    使う実装が現れるまで空けておいた。**今がその時。**
  - **`flightsim-world` に依存してはならない。** 呼び出し側が標高を引いて渡す
- 着陸装置を `AircraftConfig` に追加
  - 機体軸での接地点座標（前脚 1 + 主脚 2 が最小）
  - ばね定数・減衰係数・最大ストローク
- 接地反力の計算
  - 各脚について、地形からの貫入量に比例する垂直抗力（ばね）+ 貫入速度に比例する減衰
  - 接地面での摩擦（転がり抵抗、横方向の拘束）
  - ホイールブレーキ（`ControlInputs` に `brakes` を追加）
- **サブステップ判定に脚の剛性を反映する**
  - `FlightDynamics::required_substeps` は現在、角速度しか見ていない
  - 脚のばねは剛性が高く、`1/120 s` では反発が増幅して機体が跳ね飛ぶ
  - ADR-0004 に「接地反力を実装したら、その剛性も判定に加えること」と明記済み

**受け入れ条件（テストで示すこと）**

- [x] 静止した機体が地面に置かれ、**沈まず跳ねない**。10 分積分して高度が ±1cm 以内
- [x] 沈み込み量が妥当。機体重量 ÷ ばね定数の理論値と一致すること
- [x] スロットル全開で滑走し、対気速度が離陸速度を超えると**浮く**
- [x] 降下率 3 m/s 程度での接地が発散しない（跳ね返って空中に戻らない）
- [x] 降下率 10 m/s（ハードランディング）でも NaN を出さない
- [x] 傾斜地形（`ground_elevation` が場所で変わる）で機体が傾く
- [x] **決定論が保たれている。** 既存の `identical_inputs_produce_bit_identical_trajectories` が通ること

**実装メモ**

- `Environment::with_ground_plane` は固定した基準測地位置、`ground_elevation`、
  `GroundSlope` を受け取る。単一標高だけでは傾斜面を表現できないため、脚間を
  ローカル平面として評価する
- 最大ストローク後は高剛性のバンプストップを使う。高度クランプはしていない
- 低速摩擦は `tanh` で連続化し、静止付近の符号振動を防いだ
- 接触時は `sqrt(k/m)` と接地点の回転有効質量からサブステップ数を増やす
- 判断理由と不採用案は [ADR-0004](adr/0004-simulation-loop.md) に追記済み

**注意**

- 地面すり抜け対策に「高度をクランプする」ような処理を書かないこと。
  それは物理ではなく、着陸の手応えが完全に失われる。
- 摩擦を Coulomb 摩擦でそのまま実装すると、静止時に符号が振動する。
  低速域で線形化するなど、**振動しない定式化を選び、その理由をコメントに残すこと。**

### TASK-2: タイル生成 CLI（`flightsim-tilegen`）— 完了

2026-08-05 完了。実行時タイル形式を [ADR-0005](adr/0005-runtime-tile-format.md) で定め、
`flightsim-world::dem::io` に読み書きを、`flightsim-tilegen` に焼き込み CLI を実装した。

**やること**

1. **[ADR-0005](adr/) を書く — 実行時タイル形式の決定**
   - 既存 ADR と同じ構成（背景・選択肢・決定・帰結）。**コストを書かない ADR は不合格**
   - 提案する出発点（採用/変更は判断に任せる）:
     - 1 タイル 1 ファイル。パスは `tiles/{level}/{x}/{y}.fsdem`
     - ヘッダ: マジック、フォーマット版、`TileId`、格子サイズ、幾何誤差、標高の
       スケールとオフセット
     - 本体: `u16` に量子化した標高（`f32` の半分のサイズ。スケール 0.25m なら
       16km のレンジを 25cm 分解能で表せる）
     - リトルエンディアン固定
   - **フォーマット版を必ず入れること。** 後で変えたくなった時に、古いタイルを
     黙って誤読するのが最悪
2. `flightsim-world` に読み書きを実装（`dem::io`）
   - 書き込みは CLI から、読み込みは実行時から使う
   - **不正なファイルで panic しないこと。** `Result` で返す
3. `crates/flightsim-tilegen` — バイナリクレート
   - 入力: ローカルの Copernicus DEM GLO-30 GeoTIFF + 緯度経度の矩形 + レベル範囲
   - 出力: 上記形式のタイル群
   - **まずローカルファイル入力だけ実装する。** ダウンロードは別タスク
   - GeoTIFF の読み込みは `tiff` クレートを検討（`gdal` は C ライブラリ依存で
     Windows のセットアップが重い）
   - **このクレートは `bevy` に依存しないが、`flightsim-world` には依存してよい**
     （上位レイヤのツールなので依存の向きは正しい）

**受け入れ条件**

- [x] 合成した GeoTIFF からタイルを生成し、読み戻して**元の標高と一致**する
- [x] 量子化の誤差がスケール値の半分以下に収まる
- [x] 幾何誤差がタイル生成時に算出され、ファイルに埋め込まれる
- [x] 日付変更線・極をまたぐ範囲を指定しても正しいタイルが出る
- [x] 壊れたファイル・切り詰め・版違いで panic せずエラーを返す
- [x] `ATTRIBUTION.md` を更新（Copernicus DEM を「現在利用しているデータ」へ）

**実装メモ**

- 形式は `u16` 量子化 + **タイル毎**スケール。全球固定スケールより常に精度が良く、
  将来 海底地形（-11 000 m）を扱う際も形式を変えずに済む
- フォーマット版と FNV-1a チェックサムを持つ。長さが正しいまま中身が壊れたファイルは
  サイズ検査では捕まらない
- 粗いタイルは**面積平均**でリサンプリングする。点サンプリングだとエイリアシングが
  幾何誤差として算出され、平野が過剰に細分化される
- 検証は Python で独立に writer / reader を書いて突き合わせた。同じ実装者の
  writer と reader を往復させても、共通の誤解は検出できないため

**積み残し（次の担当者へ）**

- **被覆外の fill が幾何誤差を押し上げる。** 実データと fill の段差が崖として扱われ、
  実測で被覆完全なタイル最大 10.7 m に対し fill を含むタイルは最大 375.3 m。
  実データが無い場所ほど細分化されるという逆転が起きる。
  当面は `--bounds` で被覆内に限定する。将来はフェザリングか低被覆タイルの除外
- ダウンロード機能は無い。GeoTIFF は手元に用意する
- このタスク完了時点では標高のみだった。現在は OSM 滑走路・誘導路・apron・待機位置・
  標識・誘導路灯に対応済み。空港建物と地表画像は未対応

### TASK-3: ヘッドレス統合ランナー — 完了

2026-08-06 完了。`flightsim-sim` を新設し（[ADR-0006](adr/0006-simulation-integration-layer.md)）、
焼いたタイルの標高を FDM へ渡して実地形の上を飛ばせるようにした。

**やること**

- `.fsdem` を読み、`TileCache` に載せ、任意の測地座標の標高を返す層
  （`flightsim-world` 側。`DemTile::elevation_at` は既にある）
- そこから `Environment::with_ground_plane` に渡す基準位置・標高・ローカル勾配を作る。
  **`flightsim-fdm` は `flightsim-world` を参照できない**ので、この結線は
  上位（新しい統合クレートか example）で行う
- 軌跡を CSV か JSONL で吐くヘッドレスランナー

**受け入れ条件**

- [x] 実地形（焼いたタイル）の上で離陸 → 旋回 → 着陸の軌跡が出る
- [x] 地形標高が反映されている（平地と山で接地高度が違う）
- [x] タイル境界をまたいでも標高が不連続に飛ばない（実測 最大 0.07 m）
- [x] 決定論が保たれている（同じ入力列でビット単位一致）
- [x] タイルが無い領域でも破綻しない（海上を飛べる）

**実装メモ**

- 「離陸 → 旋回 → 着陸」を開ループの舵角時系列で出すのは非現実的なので、
  PD 制御の**決定論的フライトディレクタ**を駆動装置として持たせた。
  積分項は持たない（状態を持つとリプレイの再現性を損なう）
- **フレアでピッチを直接指定しないこと。** スロットル全閉で機首を上げると余計に
  減速し、かえって沈下率が増える。実測で 3.07 m/s。降下率保持なら 1.82 m/s
- 接地平面は 1 ステップの間固定する（ADR-0004 の契約）
- 接地時の車輪めり込み 0.028 m は、理論値 10 232 N ÷ 360 000 N/m = 0.0284 m と一致した

**積み残し（次の担当者へ）**

- **衝突判定が無い。** 上昇率を上回る速さで迫り上がる地形に対し、機体は接地したまま
  斜面を引きずられて登る。実測で 200 秒間その状態が続いた。M2 以降で扱う
- **fill を含むタイルは実行時から実データと区別できない**（下の地雷を参照）
- 場周飛行の計画は地形を考慮しない。山越えの経路では時間内に完了しない
- ストリーミングの `StreamingScheduler`（1 フレームの読み込み上限）をまだ使っていない。
  ヘッドレスでは同期読みで済むが、**M2 で描画に繋ぐ際は必ず予算制に載せること**

### M2 前の掃除 — 完了

Bevy を載せる前に、下層の欠陥を潰してから積むための作業。GUI が絡むと
「描画のバグ」に見えるものが実は下層の欠陥だった、という切り分けが極めて難しくなる。

**敵対的テスト 27 件を追加**（`tests/stress.rs`、`tests/render_rehearsal.rs`）。
極・日付変更線・1 時間飛行・極小キャッシュ・縮退した設定値などを突いた。
見つかった欠陥は 2 件。

1. **`&T` が `TileSource` を実装していなかった。** 1 つの供給元を複数の `Terrain` で
   共有できず、タイルを丸ごと複製するしかなかった
2. **浮いてしまった機体が誰にも管理されない状態になった。** 零迎角でも揚力係数は
   正なので、回転速度に達しなくても速度だけで浮く。実測で 146 m まで上昇したが
   フェーズは `TakeoffRoll` のままで、翼は水平固定・高度は無管理だった。
   接地していなければ無条件に上昇フェーズへ移るようにした

**`FloatingOrigin` / `LodSelector` / `StreamingScheduler` の予行演習**も入れた
（`render_rehearsal.rs`）。この 3 つは M2 の中核でありながら、自クレートの単体テスト
以外で一度も動いていなかった。実際の飛行軌跡をカメラ軌道にして、毎フレーム
「LOD 選択 → 予算内でストリーミング → floating origin 適用」を回している。
**M2 の描画フレームはこの手順をそのまま実装すればよい。**

### 性能の実測値

`cargo bench --workspace`（criterion）。**60 Hz フレームの予算は 16 667 µs。**

| 項目 | 実測 | 1 フレームあたり | フレーム比 |
|---|---:|---:|---:|
| FDM 1 ステップ（空中） | 3.21 µs | 6.4 µs（2 ステップ） | 0.04% |
| FDM 1 ステップ（接地中） | 26.5 µs | 53 µs（2 ステップ） | 0.32% |
| 標高クエリ（キャッシュ命中） | 62.8 ns | — | — |
| 接地平面 1 回（5 探査） | 311 ns | 0.62 µs | 0.004% |
| LOD 選択 | 1.5〜2.4 µs | 2.4 µs | 0.014% |
| floating origin 打ち直し（10 万点） | 61.0 µs | 61 µs | 0.37% |
| タイル復号 65×65 | 10.5 µs | 84 µs（予算 8 枚） | 0.50% |

**結論: 描画より下は全部合わせても 1 フレームの約 1%。** M2 の性能は
レンダラがほぼ全てを決める。下層の最適化に時間を使わないこと。

分かったこと 3 つ。

- **ADR-0002 が警告していた floating origin のフレームスパイクは、測ったら非問題だった。**
  10 万オブジェクトで 0.37%。ADR に測定値を追記して懸念を閉じた
- **接地中の物理は空中の 8 倍重い**（脚の剛性でサブステップが増えるため）。
  それでもフレーム比 0.32% で、実用上は無視できる
- **タイルの符号化は復号の 5 倍遅い**（65×65 で 56.7 µs 対 10.5 µs）。
  実行時には効かないが、tilegen の処理時間には効く

測定は 1 台の Windows 機で取ったもの。**絶対値ではなく桁と比率を見ること。**

### TASK-4: M2 の入口 — Bevy 統合 — 完了

[docs/ROADMAP.md](ROADMAP.md) の M2。担当は `rendering` と `app`。

**最初に守ること**

- **結線を再実装しないこと。** 地形 → 接地平面 → FDM は `flightsim-sim` にある。
  Bevy 層はそれを呼ぶだけにする。同じ結線が 2 箇所にあると片方だけ直されて
  挙動が食い違う（ADR-0006）
- **`Transform`（`f32`）を位置の正にしないこと。** 世界座標は `f64` ECEF の
  コンポーネントが正で、`Transform` は floating origin を適用した派生値
  （ADR-0002）。これをやらないと地表で約 76cm の量子化が起き機体が振動する
- **補間結果を物理状態に書き戻さないこと。** 決定論が壊れ、リプレイと
  ネットワーク同期の前提が崩れる（ADR-0004）
- **タイル読み込みを予算制にすること。** `StreamingScheduler` が既にある。
  無制限に読むとスタッターになる

**最初の目標**

焼いた地形の上を、`flightsim-headless` と同じ軌跡で飛ぶ様子が**画面に出る**こと。
軌跡が一致することを確かめられるので、描画側のバグと物理側のバグを分離できる。

---

## 4. すでに踏んだ地雷

同じ穴に落ちないように。**すべて実際にテストが捕まえた実バグ。**

### 単位・規約

- **安定微係数は「舵角 1 rad あたり」で定義されている。**
  正規化入力 `[-1, 1]` にそのまま掛けると舵角 57.3° 相当の過大な効きになる。
  教科書の値を写す時は最大舵角［rad］を掛けること。
  これを見逃してフルエルロンで 217°/s ロールしていた（実機は 60〜75°/s）。
  → `crates/flightsim-fdm/src/aircraft.rs` の `AeroCoefficients` のドキュメントを読むこと
- 操縦入力の符号は**操縦指示の向き**（正のエレベータ = 機首上げ）。
  空力の教科書の舵面変位角とは符号が逆。→ `controls.rs`

### 数値

- **`f64::clamp` は NaN を素通りさせる。** `NaN.clamp(0.0, 1.0)` は `NaN`。
  外部から来る値をクランプする時は `is_nan()` を先に見ること。
  大気モデル・操縦入力・DEM サンプリングの 3 箇所で踏んだ。
- **角度の正規化は半開区間の上端を保証しない。**
  `-1e-16 + 2π` は丸めで `2π` ちょうどになる。`[0, 2π)` を返すつもりが `2π` を返す。
  → `Radians::wrap_positive` の実装とコメントを読むこと
- **角度の比較に単純な減算を使わない。** 359° と 1° の差は 358° ではなく 2°。
  `Radians::shortest_difference_to` を使う。
- **教科書のシグモイド式は指数の比になっていてオーバーフローする。**
  `inf / inf = NaN`。ロジスティック関数の積へ変形すれば構造的に安全。
  → `aero::stall_blend` のドキュメントに導出を書いてある。
- **パラメータ 0 が式を縮退させることがある。**
  失速ブレンド率 0 は「モデル無効」のつもりが σ=0.75（常時 75% 失速）を返していた。
  「0 を渡したら何が起きるか」を必ず試すこと。

### ツールと環境

- **`cargo doc` は関数名とモジュール名の衝突を拒否する。** `pub mod generate` と
  `pub fn generate` が同居すると `error: generate is both a function and a module`。
  `cargo test` も `clippy` も通るので、CI の doc ジョブまで気付かない
- **MSRV は 1.85。** `usize::is_multiple_of` は 1.87 で安定化されたので使えない。
  clippy の `incompatible_msrv` が捕まえる
- **clippy は `9..=7` のような逆転レンジをリテラルで書くと拒否する**
  （`reversed_empty_ranges`）。逆転レンジを引数に渡すテストを書きたい場合は
  `RangeInclusive::new(9, 7)` で作る
- **`cargo tree` の失敗を握り潰すと、依存規約の検査が全部黙って通る。**
  `scripts/check-architecture.sh` が実際にこの状態だった。
  **安全網を足したら、意図的に違反を注入して本当に落ちることを確かめること。**
  常に通る検査は何も保証しない
- **秘密は `.env` に置く。`setx` に頼らない。** `flightsim-assetgen` の
  `MESHY_API_KEY` は、リポジトリ直下の `.env`（`.gitignore` 対象。書式は
  追跡している `.env.example`）から読む。**環境変数だけで渡そうとすると、
  現在のシェルで設定しても新しく起動したプロセスからは見えず、
  「設定したのに読まれない」で時間を溶かす**
- **秘密を扱う型は `Debug` を自分で書く。** `EnvFile` の `Debug` は鍵の
  名前だけを出す。`derive(Debug)` のままだと、うっかり `dbg!` した瞬間に
  鍵がターミナルとログに残る。**一度ログに出た鍵は、そのログが残る限り漏れ続ける**

### 統合

- **フレアでピッチを直接指定しない。** スロットル全閉で機首を上げると減速して
  かえって沈下率が増える。実測 3.07 m/s に対し、降下率保持なら 1.82 m/s
- **`fill` を含むタイルは実行時から実データと区別できない。**
  焼いた範囲の縁で実測 179 m の段差に遭遇したが、1201 サンプル全てで
  「地形データあり」と報告された。`--min-coverage` はこの**嘘**を消すが、
  **段差自体は消えない**（データ境界では不可避）
- **標高が引けないことを 0 m で表さない。** `Terrain::elevation_at` は `Option` を返す。
  ここで 0 を返すと「本当に海面」と「データが無い」が永久に区別できなくなる
- **重心の対地高度と車輪の対地高度を混同しない。** この機体で 1 m ずれる。
  接地判定は車輪側

### 描画

**目視で確かめること。** 描画は自動テストが極めて難しい。
`--screenshot <PATH>` で 1 枚撮れるようにしてあるので、変更したら必ず撮る。
下の 3 件は**全てテストが通る状態で絵だけ壊れていた**。

- **回転の向きは「合成した結果どこを向くか」で検査する。** 機体軸 → カメラ軸の
  回転を逆に作り、地平線が画面の真ん中に縦に立った。テストの側も逆向きの性質を
  検査していたため通ってしまい、転置を渡しても気付けなかった
- **ECEF 軸のメッシュを ENU 軸の描画フレームへ置くときは回転を与える。**
  忘れるとタイルが緯度経度に応じた角度だけ傾く。**赤道・本初子午線の近くでは
  正しく見える**ので、東京で試して初めて分かった
- **光量と露出は組で決める。** `FULL_DAYLIGHT`（2 万 lux）と
  `Exposure::SUNLIGHT`（10 万 lux 級）は噛み合わず、空だけ明るく地面が真っ黒になる
- **頂点色は線形 RGB。`Color::srgb` と同じ数値を渡すと明るく浅くなる。**
  実際に取り違えて、同じ地点・同じ光で画面の色が (0.328, 0.347, 0.229) から
  (0.509, 0.521, 0.374) に変わった。**目視では「そんなものか」で済んでしまい、
  画素を測って初めて分かった。** 色を疑うときは平均画素値を測ること
- **地形を見るのに実 DEM の入手は要らない。**
  `cargo run -p flightsim-tilegen --example synthetic_dem` が合成 DEM を書く。
  実在しない地形だが、**地形が映るか・LOD が切り替わるか・色が妥当か**は見える。
  投影のずれや nodata や境界の崖は現れないので、**実データの代わりにはならない**
- **薄明は「昼の度合い」と「天空光の強さ」を単純に掛けない。** 前者は
  -6°..+6° を覆うが、後者は地平線で 0 になるので、掛けると市民薄明が
  丸ごと潰れて日没の瞬間に夜になる。**実測（高度 -0.18° で既に夜と同値）で
  見つけた。** 夜そのものが暗いのは物理的に正しく、答えは滑走路灯
- **コックピット視点では機体の外形を隠す。** 目線は胴体の内側にあるので、
  外形を描くと視界が自分の機体で塞がる。プレースホルダの箱では気付きにくいが、
  実モデルを既定にした瞬間に**起動直後の画がこれになった**。内装モデルは無い
- **Bevy の feature を既定オフで列挙するなら `reflect_auto_register` を必ず入れる。**
  0.18 は型登録を自動化していて、`GltfPlugin` は `register_type` を一度も呼ばない。
  切れていると glTF シーンの生成で `scene contains the unregistered type` と
  panic する。**コンパイルも clippy も描画層のテストも CI も全部通る**
  （CI は GPU が無いので描画を実行しない）。実際に取得した `.glb` を読むまで
  出なかった。詳細は [ADR-0007](adr/0007-bevy-version.md)
- **画像の feature は「書き出す形式」ではなく「読み込む形式」で決める。**
  `png` だけ入れていたが、Meshy はテクスチャを JPEG で返す。`jpeg` が無いと
  **テクスチャが抜けるのではなく glTF の読み込みが丸ごと失敗する**
  （`invalid image mime type: image/jpeg`）。供給元を増やすときは、
  そこが実際に何を返すかを見てから足すこと
- **合成データで通ることは、外部データで通ることを意味しない。** 上の 3 件とも、
  glTF 経路のテストは全て合成データで書いてあり、全部緑のまま壊れていた
- **Bevy のアセット起点はこのリポジトリを指さない。** `BEVY_ASSET_ROOT` →
  `CARGO_MANIFEST_DIR` → 実行ファイルの隣、の順で決まる。`cargo run -p flightsim-app`
  では `CARGO_MANIFEST_DIR` が `crates/flightsim-app` になり、そこに `assets/` は無い。
  **文書に書いてあった起動コマンドは一度も動いていなかった**（`BEVY_ASSET_ROOT` を
  付けて試していたので気付かなかった）。今は `assets_directory()` が上へ辿って
  実体を見つけ、`AssetPlugin::file_path` に絶対パスで渡している
- **自前の存在確認を、フレームワークの解決先とずらさない。** 最初の修正で
  `option_env!("CARGO_MANIFEST_DIR")`（コンパイル時）を見ていたため、
  「見つかった」と言った直後に Bevy が `Path not found` を出した。
  **同じ場所を見ること**
- **`LogPlugin` より前の `warn!` は消える。** `parse_arguments` は `App::new()` の
  前に走るので、そこでの警告は購読者が居らず何も出ない。`--bogus-flag` を渡しても
  無言だった。指摘は溜めて、`Startup` スケジュールから出すこと
- **Bevy の大気散乱は `world_position.y` を海抜高度として読む。** 描画座標を
  ECEF 相対にすると空の色が緯度経度で出鱈目になる。`RenderFrame` を使うこと

### 決定論

- **乱数を引かない。** FDM は決定論的でなければならない（ADR-0004）。
  乱流は「時刻と位置の決定論的な関数」（4 次元の値ノイズ、`flightsim-fdm/src/turbulence.rs`）
  として作ってある。**検査はビット一致で行うこと**（`assert_eq!(a.to_bits(), b.to_bits())`）。
  近い値で通してしまうと、リプレイと同期が静かに壊れる
- **1 ステップごとに独立な値を引くと「揺れ」ではなく「痙攣」になる。**
  120 Hz で符号が反転する力は物理的にありえない。空間相関長と時間相関を
  明示的な定数として持ち、**1 物理ステップあたりの変化量に上限がある**ことを
  検査すること

### 文字と表示

- **画面に出す文字列は ASCII に保つ。** Bevy の既定フォントに `°`（U+00B0）の
  字形が無く、実機で豆腐（□）が出た。`deg` と綴ること。
  `nothing_on_screen_uses_glyphs_the_default_font_lacks` と
  `the_landing_report_stays_ascii` が検査している。**新しい表示文字列を
  足したら、この検査にも足すこと**
- **色を疑うときは目視ではなく画素を測る。** 頂点色を sRGB のまま渡した
  不具合は、スクリーンショットを見ただけでは「そんなものか」で通ってしまい、
  平均画素値（(0.328,0.347,0.229) → (0.509,0.521,0.374)）を測って初めて分かった

### コードの直し方

- **文字列一致の置換をするなら、一致したことを必ず確認する。** `cargo fmt` が
  行を折り返した後の文字列に対して置換をかけて、**黙って何も起きない**まま
  「直したはず」で進んだ。この作業中に 2 回踏んだ。Edit のように
  失敗が分かる手段を使うか、置換後に grep すること

### 地理データ

- **GeoTIFF の `PixelIsArea` / `PixelIsPoint` で基準点が半画素ずれる。**
  `GTRasterTypeGeoKey`（1025）を見ること。取り違えると 30 m データで 15 m ずれる
- **投影座標系のラスタを度として読むと、静かに全く違う場所の地形になる。**
  `GTModelTypeGeoKey`（1024）が 2（geographic）であることを検査する
- **経度の引き算をそのまま画素座標に使わない。** +180° と -180° は同じ場所だが
  差は 360° になる。差を `[-π, π)` に畳んでから割ること
- **粗いタイルを点サンプリングで焼かない。** 元データより粗い足跡では面積平均する。
  点サンプリングのエイリアシングがそのまま幾何誤差として算出され、
  平野が過剰に細分化される
- **被覆外を定数で埋めると、その段差が崖として幾何誤差に乗る。**
  実測で被覆完全なタイル最大 10.7 m に対し fill を含むタイルは最大 375.3 m だった
- **apron の active-airport 判定を頂点だけで行わない。** 大きな三角形は全頂点が 15 km
  圏外でも面が探索円を横切る。点と三角形の最短距離で判定する。誘導路も同じ理由で
  node だけでなく線分との距離を見る
- **OSM の明示灯火と procedural fallback を重ねない。** `txe` / `txc` は channel ごとに
  抑止し、`lit=no` は必ず `None` のままにする。入力順や `HashMap` 順で配置を変えない
- **画面文字列の ASCII 制約は物理標識にも適用する。** OSM の `ref` 自体は UTF-8 のまま
  FSAP v3 に保持するが、3x5 glyph にできない非 ASCII・8 文字超・未収録文字は描画しない

### テスト

- **同じ実装者の writer と reader を往復させても、共通の誤解は検出できない。**
  実行時タイル形式は仕様書だけを見て Python で独立に組み直して突き合わせた。
  ファイル形式やプロトコルを定義したらこれをやること
- **外部の公表値と照合すること。** 「実装がこう返すから正しい」は検証にならない。
  ISA 標準大気の表、WGS84 の定義値、国際フィート・海里の定義値を使っている。
- **境界と特異点を必ず試す。** 経度 ±180°、緯度 ±90°、対気速度 0、失速角前後。
  地形コードのバグはほぼここに集中する。
- タイルのテストヘルパーはレベルに注意。level 4 は 32 列しかない。
  数百枚扱うテストは level 9 以上を使う。

### 調査手法

原因が分からない時は**内訳を時系列で出す**。ロール率が計算値の 1/3 しか出ない問題は、
`cargo run -p flightsim-fdm --example aero_trace` で横滑り角が 27° まで発達しているのを
見て初めて特定できた（逆ヨー係数が一桁過大だった）。
テストの合否だけでは「なぜその値か」は分からない。

---

## 5. 作業の進め方

- 1 タスク 1 PR。**`main` に直接コミットしない**
- 変更を出す前に必ず全部通す:
  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test -p flightsim-core -p flightsim-fdm -p flightsim-world \
    -p flightsim-sim -p flightsim-tilegen -p flightsim-assetgen --all-targets
  cargo test -p flightsim-core -p flightsim-fdm -p flightsim-world \
    -p flightsim-sim -p flightsim-tilegen -p flightsim-assetgen --doc
  cargo bench -p flightsim-core -p flightsim-fdm -p flightsim-world \
    -p flightsim-sim -p flightsim-tilegen -p flightsim-assetgen --no-run
  cargo test -j 2 -p flightsim-render -p flightsim-input \
    -p flightsim-ui -p flightsim-app --all-targets
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
  bash scripts/check-architecture.sh
  ```
- 設計判断をしたら [docs/adr/](adr/) に記録する。**却下した案とコストも書く**
- `ARCHITECTURE.md` §7（現状のスコープ）と `docs/ROADMAP.md` を実態に合わせて更新する。
  **実装済みでないものを「ある」と書かない**
- 落ちているテストは「落ちている」と報告する。確認したことと推測を区別する

エージェント別の詳細な指示は [.claude/agents/](../.claude/agents/) にある。
TASK-1 は `simulation`、TASK-2 は `world`、TASK-3 と M2 の Bevy 統合は完了。
現在は M3。TASK-A（計器盤）、雲・雲中視程、TASK-B（OSM 滑走路・誘導路・apron・
待機位置・標識・誘導路灯）は完了。
TASK-C（難易度設定）とリプレイ（[Issue #12](../../../issues/12)）も完了。
次は M4 の残り（機体追加・METAR・ライブ交通）か、
[Issue #22](../../../issues/22)（ジオイド適用。データ源の選定が要る）。
結線を触る場合は `flightsim-sim` の公開 API を先に読むこと。
