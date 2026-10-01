# ROADMAP

## 前提の共有

要件のフルセット（全球の作り込まれた地形、膨大な空港・機体、ライブ交通、オンライン共有ワールド）は、規模として Microsoft Flight Simulator に相当します。あれは数百人年の産物です。

**このロードマップは「フルセットを最終目標に置きつつ、各段階で必ず動くものが手元にある」という形で切っています。** どの段階で止めても、そこまでのものは動きます。

各機能は「最小実装 → 拡張余地を残す」で作ります。インタフェースだけ先に切っておき、中身は後から差し替える。

## 進捗管理

未完了機能と未検証項目のステータスは [GitHub Issues](../../../issues) を正本とする。
このロードマップのチェックリストはマイルストーンのスナップショットであり、
差異があれば Issue を優先する。文書と branch 管理自体の同期は
[進捗管理 Issue #15](../../../issues/15) で追跡する。

**2026-10-01 更新。** チェックは実装の有無を表し、Issue の close・main 取り込み・
リリース成功とは別である。今回の局所検証と未完了の受け入れ条件は
[統合 QA](qa/overnight-status-2026-10-01.md) に整理した。M3 の物理機器・人の評価は未完了。
19:55 UTC 時点では撮影 hotfix の alpha.20 が公開済みで、以下の追加機能をまとめる
alpha.21 は未公開。staging の component merge とリリースを同じ完了に数えない。

---

## M1 — 物理と地形の基盤（完了）

ユーザー裁定により **FDM とワールド基盤を並行トラック** で進めます。

### トラック A: 飛行力学（`simulation` 担当）

- [x] ISA 標準大気（0〜32km、非標準日オフセット対応）
- [x] 6DoF 剛体運動、RK4 固定ステップ積分
- [x] 空力係数モデル（失速後の平板理論ブレンドを含む）
- [x] 決定論の保証（壁時計・乱数の非参照）
- [x] 定出力プロペラモデル（静止推力上限つき。ターボファンは機体追加時に別実装）
- [x] 着陸装置と接地反力（ばねダンパ、接地摩擦、ブレーキ、傾斜面）
- [x] 風・乱流（決定論的ノイズ）

### トラック B: ワールド基盤（`world` 担当）

- [x] 地理座標系クアッドツリーのタイル分割
- [x] DEM のバイリニアサンプリングと地形高度クエリ
- [x] 幾何誤差ベースの SSE-LOD 選択
- [x] タイルストリーミング（優先度キュー + LRU + フレーム予算）
- [x] Copernicus DEM からのオフラインタイル生成 CLI（`flightsim-tilegen`）
- [x] 地形メッシュ生成（スカートによる亀裂対策込み。M2 で描画と同時に完了）

### 共通基盤（`architect` 担当）

- [x] 単位付き newtype
- [x] WGS84 測地系、ECEF / NED / ENU 変換
- [x] floating origin
- [x] CI（テスト・clippy・fmt・依存規約検査・doc）

### 統合（`architect` 担当）

- [x] `flightsim-sim`: 地形 → 接地平面 → FDM の結線（[ADR-0006](adr/0006-simulation-integration-layer.md)）
- [x] 決定論的フライトディレクタ（回帰テストの駆動装置）
- [x] ヘッドレスランナー `flightsim-headless`（軌跡を CSV 出力）

**M1 完了条件**: ヘッドレスで「実地形の上を物理的に妥当に飛ぶ」軌跡を出力できること。描画はまだ無い。

**達成**。焼いたタイルの上で離陸 → 上昇 → 巡航 → 旋回 → 進入 → フレア → 接地 → 減速が
通り、軌跡を CSV で出力できる。接地時の沈下率 1.86 m/s、タイル境界での標高段差は
最大 0.07 m。

`flightsim-sim` が FDM と実地形を結線し、ヘッドレス統合ランナーと回帰テストから
同じ経路を使っている。M1 の積み残しはない。

---

## M2 — 見えるようになる（完了）

- [x] Bevy 統合（[ADR-0007](adr/0007-bevy-version.md) で 0.18.1 に確定）
- [x] 地形メッシュの生成（スカートによる亀裂対策込み）と描画
- [x] LOD ストリーミング（フレーム予算つき）
- [x] 大気散乱（Bevy 0.18 の Bruneton モデル。Rayleigh + Mie + オゾン）
- [x] 入力（キーボード。舵のレート制御と中立復帰）
- [x] 視点切替（コックピット／チェイス／フリー／タワー）
- [x] HUD（対気速度・高度・対地高度・昇降率・方位・姿勢・スロットル）
- [x] 機体の 3D モデル（glTF。テクスチャ付きの軽単発機を同梱。軸と倍率の補正層つき）
- [x] ゲームパッド（左スティック＋トリガー。HOTAS の軸マッピングは M3 で）
- [x] 地表の塗り分け（標高と傾斜。**衛星画像ではない**——画像はデータ源と権利を決めてから）

**M2 完了条件**: 1 空港周辺で離陸 → 旋回 → 着陸が通ること。
→ **達成**（2026-08-24、`crates/flightsim-sim/tests/airport_circuit.rs` が受け入れテスト。
合成飛行場・滑走路描画・着陸評価・ゲームパッドまで込み）。

---

## M3 — シミュレータとして成立する（現在）

- [x] 空港データ（OSM `aeroway=*`）
  - [x] `aeroway=runway` 中心線のオフライン変換、実行時 DB、最寄り選択
    （[Issue #21](../../../issues/21)）
  - [x] `aeroway=taxiway` 中心線の取り込み・描画（[Issue #25](../../../issues/25)）
  - [x] apron（閉じた way / hole 付き multipolygon）、待機位置標示と ASCII 物理標識、
    明示 TXE / TXC / RGL 灯火と `lit=no` を守る決定論的 fallback
    （[Issue #27](../../../issues/27)）
- [x] 計器一式（対気速度・姿勢・高度・昇降・方位・出力の 6 つ。コックピット視点で出る）
- [x] 計器の照明（太陽高度に連動。夜に盤面が読める）
- [x] コックピット内装（Cessna 172 の実寸から**手続き的に組む**。計器盤・
  グレアシールド・風防の支柱・側窓・操縦輪・座席。**3D モデルは調達していない**
  ので、再配布条件の判断が要らない）
- [x] 夜間の滑走路灯（縁灯・進入端灯・末端灯。太陽高度で滑らかに点消灯）
- [x] 定常風（`--wind 270/10`。FDM の空力と HUD へ。横風着陸が成立する）
- [x] 時刻・太陽位置（天文計算。時間加速つき。朝焼け・薄暮・夜）
- [x] 突風・乱流（決定論的な値ノイズ。時間・空間相関つき）
- [x] 決定論的な簡易雲層と雲中視程（雲量・雲底・雲頂・視程を CLI で設定。
  [Issue #11](../../../issues/11)）
- [x] チュートリアル導線（状態機械。今なにをすべきかを 1〜2 行で。`H` で消せる）
- [x] フライトディレクタの滑走路中心線への精密横誘導（[Issue #4](../../../issues/4)）
- [x] 複数 controller、pitch/roll/yaw/throttle/brake/flaps の軸/ボタン再割り当て、
  校正・反転・deadzone・感度の保存/再読込（[Issue #9](../../../issues/9)）。
  native channel は明示 opt-in、実 HOTAS の機種別確認は下の検証残件
- [x] F10/F11 の controller 診断と、キーボードのみでも動く trim の回帰修正
- [x] 姿勢計の circular material mask。旧 Bevy clip failure を実描画で再現し、
  正負 bank / 極端 pitch の計 36 case を pixel 検査（[Issue #33](../../../issues/33)）
- [x] EGM2008 / EGM96 local geoid 正規化、datum/CRS/units 拒否、provenance と
  atomic tile output（[Issue #22](../../../issues/22)）
- [x] PBF の検証済み decoder、敵対的 overflow/index/zlib fixture と正常出力互換
  （[Issue #23](../../../issues/23)）。総メモリ消費の sandbox ではない
- [x] 難易度設定（`--difficulty beginner|normal|realistic`。風・乱流・案内の既定を
  まとめて決める。**着陸の採点には効かない**——難易度で甘くすると点が意味を失う）

### M3 の検証残件

- [ ] ゲームパッド / HOTAS 実機の機種名・入力範囲・符号・感度・切断復帰を記録
  （[Issue #2](../../../issues/2)、[Issue #9](../../../issues/9) の hardware follow-up）。
  software 診断と設定保存は実装・自動検査済み
- [ ] 乱流強度を人が操縦して所感を記録（[Issue #5](../../../issues/5)）。24 数値シナリオ・
  許容範囲・長時間 severe と独立した人の worksheet は用意済み。数値 pass で代用しない
- [ ] 実 Copernicus DEM の表示受け入れ（[Issue #6](../../../issues/6)）。出典付き fixture の
  再現 bake と夜間・約 3 km 落下試験の画像確認は実施し、camera lag / runway burial を修正。
  [360 秒・17.533 km の巡航](qa/high-altitude-2026-10-01.md) は exact replay に加え、native app
  の 6:00 完走・4 回の実 render-origin rebase・完了画像の地形/地平線を確認。
  未撮影の LOD 遷移・夜間近地表の連続性・残る LOD mesh 遮蔽の広い判定は継続。
  1.5 NM 夜間進入で灯火が十分に視認できることも未確立
- [ ] 物理 GPU と Windows ドライバで起動・描画を確認
- [ ] 実スピーカーで音量・音色・警報の聴感を確認

### M3 の配布・起動検査

- [x] Windows x86_64 実行ファイルと必要アセットを、CI と展開後 smoke に成功した版の
  prerelease へ自動添付する経路（[Issue #7](../../../issues/7)）
- [x] alpha.20 / PR #42 の Windows batch capture 正常終了・zip・公開物を検証。
  alpha.19 は PNG 保存後に 180 秒で終了せず失敗したが、alpha.20 は exact source の CI と
  展開後 WARP smoke が成功。2026-10-01 19:08:32 UTC 公開、実 zip の SHA-256 と PNG も確認済み
- [ ] alpha.21 の final source CI と、展開 zip の Light Single / Swift Sport の個別 smoke、
  2 枚の公開 PNG・同梱 profile / GLB / Blender source・checksum を検証
- [x] Mesa/lavapipe の CPU Vulkan でアプリ起動・同梱 glTF・PNG 描画を検査
  （[Issue #8](../../../issues/8)）

CI の Mesa/lavapipe と Windows 配布の D3D12 フォールバック・スモークは、
アプリ起動・同梱 glTF 読み込み・実 scene の PNG 描画・正常終了を CPU 上で検査する。
alpha.21 向けの配布 workflow は 2 機種を別々に起動する。これから実行する gate の定義であり、
alpha.20 の実績を 2 機種へ広げない。実 GPU の代替ではない。

---

## M4 — 拡張

- [x] 機体 profile の versioned JSON、Light Single / オリジナル Swift Sport の選択、
  FDM・model・視点・音・入力 rate 切替と invalid load 拒否（[Issue #10](../../../issues/10)）。
  両機の離陸・30 秒無操縦進入を数値検査。実機性能認証ではない
- [ ] 高品質な雲のボリュームレンダリング（[Issue #11](../../../issues/11) では
  決定論的な簡易雲層まで実装済み）
- [ ] METAR 取り込み（現状の天候は CLI で手動設定）
- [x] リプレイ（[Issue #12](../../../issues/12)。操縦入力を記録して物理を回し直す。
  一時停止・速度変更・frame-zero から bounded 再実行する後退シーク、drift stop。
  **機体指紋の不一致は拒否する**。同一 build / terrain が前提、旧乱流記録は互換保証なし）
- [x] 交通 source interface、決定論的合成交通、位置・向き・識別表示、stale / 欠損処理
  （[Issue #13](../../../issues/13) の最小受け入れ範囲）
- [x] version 1 UDP の local session 作成・参加・退出、補間・遅延/欠損・再接続と
  loopback 統合検査（[Issue #14](../../../issues/14)、[ADR-0010](adr/0010-local-traffic-sessions.md)）。
  最終 candidate の実 app 2 プロセスで接続・host 再起動・新 session 再参加・F12 退出も確認
- [x] 当初疑った client PNG の文字欠けは、針が小さい計器文字を横切る現象と切り分けた。
  原寸・単独実行・native resize で help の欠落は再現しなかった。計器の重なりによる読みにくさは残る
  （[LAN QA](qa/high-altitude-lan-alpha21-2026-10-01.md)）
- [ ] 実 ADS-B traffic source とその利用条件・欠損品質の検証
- [ ] 複数実マシン LAN 検証と公開 Internet 向けの認証・暗号化・NAT・matchmaking。
  現在は trusted local/LAN 専用で、公開 online world を完成したとは呼ばない

---

## 意図的に後回しにしているもの

理由とセットで記録します。「忘れていた」と「今はやらないと決めた」を区別するため。

| 項目 | 後回しの理由 |
|---|---|
| フォトグラメトリ都市 | オープンデータ方針（ADR-0003）の帰結。OSM フットプリントの押し出しで代替する |
| 全球のタイル事前生成 | データ量が数百 GB 規模。地域指定で焼く CLI を先に作る |
| コリオリ力 | 巡航時で重力の約 0.26%。構造だけ空けて後で足す（ADR-0002） |
| 追加の実機モデル | Light Single とオリジナル Swift Sport は同梱。さらに増やす際は性能出典・モデルの来歴と再配布条件を個別に確認する |
| 内装の 3D モデル調達 | 内装は実寸から手続き的に組んだので、**モデルの調達は不要になった**。写実的な内装モデルを入れるかは別の判断 |
