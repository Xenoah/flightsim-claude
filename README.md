# flightsim-claude

地球規模のフライトシミュレータ。Rust + Bevy、Windows 対象。

**M2 のゲームループを実装済み。M3 の実機・操縦感検証と、M4 の拡張を進めています。**
実地形・滑走路・時刻・風・乱流・雲層、着陸練習と 5 段階評価に加え、
2 機種の選択、保存できる入力設定、リプレイ、合成交通、ローカル/LAN 同期を実装しています。
この README は alpha.21 のソースと同梱機能の説明です。配布物の公開状態と対象版は
[Releases](https://github.com/Xenoah/flightsim-claude/releases) で確認してください。Windows 版は
対象 commit の CI、展開 zip からの 2 機種の撮影・正常終了、checksum 検査の後にだけ公開されます。
Windows 版のダウンロード・再配布前に、下記の
[Microsoft ランタイムの補足規約と同意方法](#microsoft-ランタイムの補足規約と同意方法)
を確認してください。
2026-10-01 の実測・修正・未検証事項は
[検証記録](docs/qa/overnight-status-2026-10-01.md)、設計上の範囲は
[ARCHITECTURE.md §7](ARCHITECTURE.md#7-現状のスコープ) を参照してください。

2026-10-02 の開発版には、オフラインの全世界地形と月別気候、地図からの新規飛行を追加しました。
`M` で世界地図を開けます。全球の基礎格子は約 20 km、地域 DEM があればそちらを優先します。
気候は 1991–2020 年の月別再解析値とモデル近似で、現在の天気ではありません。
[世界地図の操作とデータ](docs/global-map.md)、[最終統合 QA](docs/qa/global-map-integration-2026-10-02.md)、
[商用配布候補の検証と未解決項目](docs/qa/commercial-staging-2026-10-02.md) を参照してください。

地表の開発版には、全世界で使える手続き的な材質の細部と、任意の地域 OSM データから
道路・建物の輪郭・土地利用を描く機能を追加しています。個々の樹木、外壁、未記載の高さは
描画上の近似です。全球の都市データや衛星写真は同梱しません。
[地域の地表データと起動方法](docs/regional-scenery.md) と
[地域 DEM の描画精度・残る誤差](docs/qa/regional-render-detail-2026-10-02.md) を参照してください。

---

## 何ができるか（今）

```bash
# 純 Rust 側。数秒
cargo test -p flightsim-core -p flightsim-fdm -p flightsim-world \
    -p flightsim-sim -p flightsim-tilegen -p flightsim-assetgen -p flightsim-net
# 描画層。Bevy を含むので重い
cargo test -j 2 -p flightsim-render -p flightsim-input -p flightsim-ui -p flightsim-audio -p flightsim-app
```

An explicit alternate Reinhard build is documented in [analytical tonemapping](docs/analytic-tonemapping.md). Ordinary LUT/Tony defaults remain unchanged; the alternate appearance has Linux-only validation.

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
  オフライン CLI。CRS・単位・鉛直基準を検査し、対応する利用者提供の EGM2008 / EGM96
  グリッドで `h = H + N` を適用して WGS84 楕円体高へ正規化する。出所不明の標高を
  黙って楕円体高にはしない。日付変更線・極・nodata・書き込み失敗も回帰検査する
- **OSM の空港地上設備を使える** — 利用者が用意した地域 `.osm.pbf` から、滑走路・
  誘導路・エプロン・待機位置・地上灯火を決定論的な `.fsairports` FSAP v3 へ焼く
  オフライン CLI。閉じた apron way と hole 付き multipolygon、待機位置 node / 路面標示
  way、明示的な TXE / TXC / RGL 灯火を扱う。明示灯火が無い誘導路は決定論的に灯火を補い、
  `lit=no` は補完しない。起動時は開始地点へ最も近い滑走路と 15 km 圏の設備だけを選び、
  各 geometry の標高を DEM から取得して描画する。待機位置標識は外部フォントを使わない ASCII
  geometry である。離陸・進入・滑走路描画・灯火・着陸評価は同じ滑走路を共有し、OSM
  由来 DB を実際に読んだときだけ画面に ODbL の帰属を表示する
- **実地形の上をヘッドレスで飛べる** — 焼いたタイルから接地平面（標高と勾配）を作って
  FDM へ渡し、離陸 → 上昇 → 巡航 → 旋回 → 進入 → 接地までを軌跡 CSV に出力する
- **機体 3D モデルを読める** — glTF / glb を機体軸へ合わせる補正層つき。モデルごとに
  違う「前」「上」の軸と大きさを引数で吸収するので、差し替えても描画コードを触らない。
  **Light Single と Swift Sport の 2 機種を選択可能**。版付き JSON で FDM・外形・
  視点・音・入力レートを切り替える。Swift Sport は本リポジトリで Blender により制作し、
  再生成スクリプト・編集用 `.blend`・`.glb` を同梱する。実機性能の認証モデルではない
  追加の [Meadow Trainer](docs/aircraft/meadow-trainer.md) は独自制作の高翼機外形で、
  `--aircraft assets/aircraft/meadow_trainer.json` から選ぶ。Light Single と同じ数値の
  飛行特性・操作設定を使う外観の選択肢であり、新しい実機性能モデルではない
- **機体モデルの取得** — Meshy の API から取ってくるオフライン CLI。API キーは `.env`
  から読み、**引数では受け取らない**（コマンドラインはプロセス一覧とシェル履歴に残る）
- **合成飛行場とゲームループ** — 引数なしで滑走路の中心線上から始まり、離陸して
  戻って降りると**着陸が 5 段階で評価される**（沈下率・滑走路上か・バンク）。
  滑走路は舗装・中心線・ピアノキーつき。回帰テスト用フライトディレクタは6 km手前から
  中心線を連続捕捉し、左右6 m/sの直角横風でも滑走路内へ接地する
- **複数コントローラと保存できる軸設定** — pitch / roll / yaw / throttle / brake / flaps
  を軸・ボタンへ割り当て、校正値・反転・デッドゾーン・感度を JSON に保存する。
  キーボードと軸ごとに共存し、F10 / F11 でデバイスと入力値を確認できる。
  未対応 HOTAS チャンネル用の `--native-controllers` は明示 opt-in。
  **実機の接続・符号・感度・切断復帰は未検証**
- **風と乱流** — `--wind 270/10`（方位/ノット）と `--turbulence moderate`。
  空力が相対速度で効くので横風着陸が成立する。向かい風で離陸滑走が実際に短くなる。
  **乱流は物理ステップ時刻と位置の決定論的な関数**。日付変更線・極の連続性と
  描画フレームのまとめ方によらない同一入力の再現性を回帰検査する。
  数値シナリオと人による操縦感評価は別で、後者は未実施
- **着陸練習** — `--approach 1.5` で滑走路の手前 1.5 海里・3 度の進入角から始まる。
  毎回場周を一周しなくてよい
- **音** — エンジン音・風切り音・**失速警報**。波形は録音でも同梱ファイルでもなく、
  **毎標本その場で合成する**（[ADR-0009](docs/adr/0009-synthesised-audio.md)）。
  Light Single の既定は**戦闘機の低バイパス比ターボファン**: ファンの翼通過音（32 枚 × N1/60 で
  4.3〜7.1 kHz の「キーン」）、翼端が超音速のときだけ出る**バズソー**、
  Strouhal 数 0.2 で決まる排気の広帯域音、アフターバーナー。
  スプールは上がるのに数秒かかる。
  `--engine piston` でピストン単発（点火パルス列 + 排気管の共鳴）にも切り替わる。
  失速が近いことを計器を見ずに知る手段は、これしかない。
  **`--engine` は音だけの上書き**。Light Single は従来どおりタービン音を既定とし、
  Swift Sport はピストン音を使う。どちらの FDM も固定脚のピストン・プロペラ機である
- **墜落** — 沈下率 5 m/s 超・バンク 20 度超・機首下げ 15 度超で接地すると機体が
  壊れ、そこで止まる。**何がまずかったかを数字で出す**（「沈下率 13.3 m/s、脚が
  持たない」）。閾値の根拠と、それが認証資料の「壊れる値」ではないことは
  `crates/flightsim-sim/src/crash.rs` に書いてある。**難易度では変わらない**
- **コックピット内装** — Cessna 172 の実寸から手続き的に組んである
  （客室幅 40 in、計器 3.125 in、シックスパックの T 字配置）。計器盤・
  グレアシールド・風防の支柱・側窓・操縦輪・座席。**3D モデルは調達していない**ので、
  内装には第三者のモデル素材を使わない。**左席に座る**（172 は左が機長席）
- **昇降舵トリム** — `[` / `]`。**手を離したときに釣り合う速度**を決める。
  Light Single の地上開始は従来の 0.09、Swift Sport は 0.08。
  進入開始は各機体の専用トリム・姿勢・出力を使い、無風・海面付近の 3 度進入を
  30 秒間保持する数値回帰がある。一般の高度・風や接地までの無操縦飛行は保証しない。
  キーを離してもスロットルとトリムは保持するが、ピッチ舵は中立へ戻る。
  引き続けると失速し、早すぎる離陸は失速していなくても沈下し得る。地上では約 75 kt EAS から
  S / Down を穏やかに入れ、機首が最初に上がり始めたら離して様子を見る（水平な滑走路で約 3 度が目安）。
  一定秒数の押下や離陸までの引き続けを目安にしない。初期上昇は 70–80 kt EAS、
  トリムは安定後に必要な方向だけ調整する。
  機種認証された速度や無操縦飛行の保証ではない。詳しくは [キーボード操縦ガイド](docs/keyboard-flight.md)
- **一時停止とやり直し** — `Esc` で止まり、`R` でその場で最初からやり直す。
  **失敗しても再起動しなくていい。** やり直すと飛行記録・着陸評価・案内も一緒に
  戻る（機体だけ戻すと、前回の結果が画面に残り続ける）
- **リプレイ** — 記録可能な飛行では、`F9` で v3 `.fsreplay` に保存、
  `--replay <FILE>` で再生する。入力・フレーム時間・条件・検査用キーフレームを記録し、
  フレーム 0 の状態から同じ物理を回し直す。一時停止・速度変更・10 秒戻しつき。
  後退はフレーム 0 から上限付きのバッチで再実行し、時計・端数・飛行記録も戻す。
  機体指紋の不一致は起動を拒否し、位置のドリフト検出時は停止して表示する。
  **同一ビルド・機体・地形での再現性を検査した機能**で、旧乱流記録や別環境の互換は保証しない
- **難易度** — `--difficulty beginner|normal|realistic` が風・乱流・案内の既定を
  まとめて決める。`--wind` / `--turbulence` を書けばそちらが勝つ。
  **着陸の採点は難易度で変えない** — 甘くすると、上達したのか設定を下げただけなのかが
  分からなくなり、点が意味を失う
- **チュートリアル導線** — 「今なにをすべきか」を画面中央上に 1〜2 行で出す。
  一度着陸したら二度と出ず、`H` でいつでも消せる。**上級者の邪魔をしない**
- **時刻と太陽位置** — `--time 05:30`（地方平均太陽時）と `--time-rate 60`。
  天文計算（Meeus / NOAA と同じ低精度式）で日の出・南中・日没が正しい位置に来る。
  夏至の東京の南中高度 77.75°、分点の日の出は真東、極の白夜と極夜まで一致する
- **決定論的な雲層と雲中視程** — 雲量は `--cloud-cover`（0〜1）、雲底・雲頂は
  `--cloud-base` / `--cloud-top`（楕円体高 m）、雲中視程は `--cloud-visibility`（m）で
  指定する。同じ設定なら同じ雲場になり、雲へ入ると外が見えにくくなる。
  通常の新規飛行では月別の地域気候から雲量を近似し、明示した雲設定を優先する。
  `--cloud-cover 0` で快晴にできる。現在の観測天気ではない

- **練習用交通とローカル/LAN 同期** — `--traffic synthetic` の決定論的な周辺機、
  `--host` / `--join` の version 1 UDP セッション、補間・stale 表示・タイムアウト・再接続。
  周辺機は汎用メッシュで位置・向き・識別を示す。**実航空交通データではない**。
  信頼できるローカル/LAN 向けで、認証・暗号化・NAT 越えは無い。Internet へ公開しない。
  [同一マシンの実 app 2 プロセス](docs/qa/high-altitude-lan-alpha21-2026-10-01.md) で
  接続・host 再起動後の自動再参加・F12 退出を確認済み

次は既定ゲームパッド設定です。変更手順は
[入力設定と診断の QA](docs/qa/input-controllers-attitude-2026-10-01.md) を参照してください。

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

# DEM と一致する公式グリッドを別途用意する（グリッドは同梱・自動取得しない）
cargo run -p flightsim-tilegen -- \
    --input data/copernicus.tif --output data/tiles --min-level 8 --max-level 12 \
    --source-vertical-datum egm2008 --geoid-grid data/egm2008-5.pgm --geoid-model egm2008

# 地域 PBF を別途用意する（検証付き parser も総メモリ消費の sandbox ではない）
cargo run -p flightsim-tilegen --bin flightsim-airportgen -- \
    --input data/japan-latest.osm.pbf --output data/japan.fsairports

cargo run -p flightsim-sim --bin flightsim-headless --     --tiles data/tiles --start 35.553,139.781 --output flight.csv

cargo run -p flightsim-app --release -- --tiles data/tiles \
    --cloud-cover 0.55 --cloud-base 700 --cloud-top 1300 --cloud-visibility 300

cargo run -p flightsim-app --release -- --tiles data/tiles \
    --airports data/japan.fsairports --start 35.55,139.78

cargo bench --workspace                           # 性能測定（criterion）
```

`--source-vertical-datum` は提供元の仕様を確認した上で欠落 metadata を補う指定で、
矛盾する GeoTIFF tag を上書きしません。`--assume-ellipsoidal` は変換の代用ではありません。
古い `.fsdem` は自動変換されないので、正しい基準で別の出力先へ焼き直してください。
[再現可能な実 DEM の取得・焼き込み手順](docs/qa/data-boundaries-2026-10-01.md) に出典と制限があります。

### 機体・入力・交通・撮影の入口

カスタム機体 JSON の公開仕様・単位・既定値・モデル軸・制約は
[Aircraft profile v1 authoring guide](docs/aircraft-profiles.md) と
[JSON Schema](schemas/aircraft-profile-v1.schema.json) を参照してください。

```bash
cargo run -p flightsim-app -- --help
cargo run -p flightsim-app -- --list-aircraft
cargo run -p flightsim-app -- --aircraft swift-sport --view chase --traffic synthetic
cargo run -p flightsim-app -- --aircraft assets/aircraft/swift_sport.json --approach 1.5

# 設定ファイルの新規作成。既存ファイルは上書きしない
cargo run -p flightsim-app -- --write-input-config controls.json
cargo run -p flightsim-app -- --input-config controls.json --input-diagnostics
cargo run -p flightsim-app -- --native-controllers --input-config controls.json --input-diagnostics

# 同じコンピュータの別プロセスから参加。LAN の bind は必要な場合だけ明示する
cargo run -p flightsim-app -- --host
cargo run -p flightsim-app -- --join 127.0.0.1:41520 --callsign PILOT2

# 実際の scene / UI を PNG に描画。headless は表示サーバーなしで撮影して終了
cargo run -p flightsim-app -- --headless-screenshot capture.png --screenshot-delay 3
cargo run -p flightsim-app -- --screenshot window.png --exit-after-screenshot
```

`--screenshot-delay` は最小待ち時間です。撮影は 30 フレーム以上経過し、機体の fit と
地形・近傍の滑走路の CPU 側の準備が整った後に行います。通常の全球地形では要求した
表示タイルの一致と橋・地表面の commit を待ちます。粗い地域データ・全球データの詳細度
上限・空データは、要求範囲の初回探索と利用可能なメッシュ準備を終えた地形を撮影します。
欠落・失敗済みタイルの通常の再試行は撮影を妨げません。GPU・shader の完了を保証する
ものではありません。

F9 は空いている連番名を原子的に確保して記録を保存します。再生は同じ機体を選んで
`--aircraft swift-sport --replay flight-001.fsreplay` のように指定します。
新しい記録は `yaw_rate_p` を含む完全な機体 ID を持つ v3 です。旧 v1/v2 は既定で拒否し、
対応する revision-2 の Light Single / Swift Sport に限り `--legacy-replay-compatibility`
で明示的に基準機体を仮定できます。その場合も旧ファイルが `yaw_rate_p` を記録して
いないことを常時表示します。名前や位置ドリフトでは欠けた値を証明できません。
手動の雲量・雲底・雲頂・視程指定がある通常飛行は記録と F9 を無効にして理由を表示し、
リプレイと同時指定した場合は起動を拒否します。雲の描画品質設定はこの制限の対象外です。
モデル天候付き v3 は検証済みの記録値と実行済みシミュレーション時間を描画へ復元します。
天候の起動引数によるリプレイの上書きは拒否します。詳細は [リプレイ互換方針](docs/replay-identity.md)。
F5 は再生停止/再開、F6 / F7 は速度、F8 は 10 秒戻し。
再生は frame zero から物理時計・端数も復元し、不正な数値のファイルや機体指紋の不一致を
拒否します。同一 build / 機体 / 地形が前提で、旧乱流記録の再現性は保証しません。
F10 は入力診断、F11 は診断ページ、F12 は参加中の LAN セッションから退出です。
`--host` の既定は `127.0.0.1:41520`。詳細は [ADR-0010](docs/adr/0010-local-traffic-sessions.md)。

### Windows で起動する

[Releases](https://github.com/Xenoah/flightsim-claude/releases) の各公開済み版に添付された
`flightsim-claude-v<version>-windows-x86_64.zip` を使ってください。
バージョン更新だけでは配布完了を意味しません。Windows のビルド・展開後の起動検査・
公開が成功した版だけに zip が付きます。展開後、`assets/` を
`flightsim-app.exe` と同じフォルダに置いたまま実行してください。これは開発途中の
prerelease です。

#### Microsoft ランタイムの補足規約と同意方法

この Windows 版に実際に組み込まれる Microsoft の実行時処理・起動処理コードだけに、
[補足規約（英語正文、2026-10-09-2）](docs/release/components/MICROSOFT-COMPONENT-TERMS.txt)
が適用されます。[日本語参考訳](docs/release/components/MICROSOFT-COMPONENT-TERMS.ja.txt) と
[コンポーネント表示](docs/release/components/MICROSOFT-COMPONENT-NOTICE.txt) も ZIP に同梱します。
対象コードの保護、Microsoft と供給者の免責・損害賠償責任、再配布時の義務を含みます。
FlightSim のソース等の MIT／Apache-2.0 と各部品の既存ライセンス、通常の FlightSim の
ベンチマークは変わりません。既存の著作権表示も保持します。

初回起動時は、シミュレーターに入る前に規約全文と日本語参考訳、既存の著作権表示を
表示します。「同意して続行 / Agree and continue」を明示的に選んだ場合だけ続行し、
「同意せず終了 / Decline and exit」は同意を保存せず終了します。同意は初期選択しません。
規約画面から同梱のコンポーネント表示も確認できます。
規約画面は Windows 標準の Windows PowerShell 5.1 と .NET Windows Forms を使用します。
画面を開けない環境では同意を推測せず終了し、システムの実行制限を変更しません。

**再配布者は、再配布前に `flightsim-app.exe --component-terms` を実行し、同じ規約を
確認して明示的に同意してください。** この専用操作はシミュレーターを起動せず、
同意・不同意の選択後に終了します。あとから規約を再表示するときも同じ操作を使えます。
ZIP の規約・表示を残し、その先の再配布者と外部エンドユーザーにも、補足規約に従って
対象コードを保護する条件への同意を求めてください。表示やダウンロードだけを同意とは扱いません。

保存するのは、利用者自身の明示的な選択による、正確な規約の版・ハッシュに対応する
端末内の同意記録だけです。内容が変われば再度選択を求めます。アカウント、テレメトリー、
個人情報や同意記録のサーバー送信は追加しません。Visual Studio の購入・アカウント作成は不要です。

Windows、UCRT、グラフィックスドライバー、x64 Visual C++ v14 ランタイムは外部の前提です。
必要なランタイムは [Microsoft の公式案内](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170)
からサポート中の x64 v14 パッケージを入手してください。Microsoft の DLL やインストーラーを
この ZIP に別途同梱したり、非公式サイトから個別 DLL を取得したりする手順ではありません。

alpha.20 は source `f3d32d816cc625d10e4309f3506c150280e749a5` の CI、展開した zip の
Windows WARP 起動・モデル読込・完全な PNG・終了コード 0 を確認済みです。
公開アーカイブの SHA-256 と実 PNG の確認記録は [統合 QA](docs/qa/overnight-status-2026-10-01.md#verified-alpha20-release)
にあります。alpha.21 の 2 機種入り配布・各機体の smoke は別の公開ゲートです。

## 未実装・未検証

- OSM の空港建物、衛星地表画像、METAR、高品質なボリューム雲、実 ADS-B 交通、
  公開 Internet 向けの認証・暗号化・NAT/マッチメイキングは今後の範囲
- 物理ゲームパッド / HOTAS の機種別試験、乱流の人による操縦感評価、実スピーカーでの
  聴感確認、物理 GPU・ベンダードライバの検証は未実施
- フライトディレクタの中心線誘導は回帰テスト用で、ILS・航法データ・autoland ではない
- 実 Copernicus DEM の夜間・約 3 km の落下試験の画像で見つけた camera lag と
  滑走路・灯火の埋没を修正。[360 秒・17.533 km の巡航](docs/qa/high-altitude-2026-10-01.md)
  は native app で最後まで再生し、4 回の実 render-origin rebase と、完了時の地形・地平線を確認。
  静止画と sampled observation の範囲であり、全 LOD 遷移や GPU の滑らかさは保証しない。
  DEM と描画 LOD 三角形の差による遮蔽と、夜間 1.5 NM 進入で役立つ灯火の視認性は残る課題。
  極端な降下時の V/S は短い k 表記へ修正し、回帰試験と最新 app の降下画像で再確認済み
- 小さい計器の針がラベル・数値を横切るため、重なる文字は読みにくい場合がある。
  当初疑った文字欠けは原寸画像・単独実行・native resize で再確認し、独立した欠落不具合は確認されなかった
- ソフトウェア Vulkan / D3D12 の検査は実 GPU の代替ではなく、FPS を保証しない。
  Windows の PNG 保存成功と正常終了は別の検査項目として扱う

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
| [ADR-0008](docs/adr/0008-osm-airport-data.md) | OSM 空港地上設備の配布境界と FSAP 実行時 DB |
| [ADR-0009](docs/adr/0009-synthesised-audio.md) | 音の合成と物理モデルとの境界 |
| [ADR-0010](docs/adr/0010-local-traffic-sessions.md) | 純 Rust の交通データ層とローカル/LAN の限界 |

設計の要は 1 点に集約されます。

> **`flightsim-core` / `flightsim-fdm` / `flightsim-world` / `flightsim-sim` / `flightsim-tilegen` / `flightsim-net` は Bevy に依存しない。**

物理と地形が純 Rust であるおかげで、`cargo test` が GUI もアセットもなしに数秒で回ります。
これは慣習ではなく [CI で検査される規約](scripts/check-architecture.sh) です。

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
```

`net` は純 Rust。受信した状態を app が描画へ渡し、自機の物理へは書き戻しません。

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

Windows 実行ファイルに組み込まれる Microsoft コードには、上記の
[コンポーネント補足規約](docs/release/components/MICROSOFT-COMPONENT-TERMS.txt) が別途適用されます。
FlightSim のソースと別ライセンスの部品について、既存ライセンスが認める権利は変わりません。

## Offline global map and climate (development branch)

Press `M` for geographic destinations, exact coordinates and monthly previews, or
start with `--world-map`. Bundled coarse world terrain and NOAA 1991-2020 climate
means work offline; local DEMs take priority. This is not live weather or globally
detailed30m terrain. See [the global map guide](docs/global-map.md),
[data provenance](docs/data/global-sources.md) and [ADR-0011](docs/adr/0011-offline-global-terrain-climate.md).
Validation and publication status are reported separately.
