# ADR-0008: OSM 空港をオフライン変換し、版付き実行時 DB で読む

- **状態**: 承認
- **日付**: 2026-08-30

## 背景

M3 の空港データ対応では、OpenStreetMap（OSM）の `aeroway=runway` と
`aeroway=taxiway` から実在する滑走路・誘導路を取り込む。最初の実装では
[`Runway::synthetic`] 1 本だけだった状態から滑走路中心線を FSAP v1 で導入し、
誘導路の折れ線を保持するため FSAP v2 へ拡張した。次に apron、待機位置、物理標識、
誘導路灯を一つの DB へ入れるため、FSAP v3 を section-directory 形式にした。

[ADR-0003](0003-terrain-data.md) は、生の GeoTIFF / OSM PBF を実行時に解析せず、
オフラインで中間形式へ焼くと定めている。OSM は ODbL であり、公開利用時には
OpenStreetMap への帰属とライセンスの明示も必要になる。

もう一つ、鉛直基準が一意ではない。OSM の `ele=*` は原則として平均海面上の
高さだが、欠落が多く、元の基準もタグだけでは検証できない。また Copernicus DEM
GLO-30 の公式な鉛直基準は EGM2008（EPSG:3855）だが、現在の tilegen はジオイド
変換を行わず標高値をそのまま格納している（[Issue #22](../../../issues/22)）。したがって、
OSM の別系統の高さを滑走路だけに入れても、使用中の地形とは揃わない。

[`Runway`]: ../../crates/flightsim-world/src/airport.rs
[`Runway::synthetic`]: ../../crates/flightsim-world/src/airport.rs

## 決定

### 派生データをリリースへ同梱しない

利用者が用意した地域 `.osm.pbf` を、オフライン CLI `flightsim-airportgen` で
実行時空港 DB へ変換する。リポジトリと prerelease には、OSM の PBF も派生 DB も
同梱しない。

この段階では、アプリ本体のライセンスと OSM 派生 DB の配布条件を混ぜない。
将来こちらで地域 DB を配布する場合は、取得元・スナップショット日・ODbL の
share-alike 提供方法を決めた別 ADR が必要である。

入力 PBF は信頼できる提供元から取得したものに限る。固定した `osmpbf 0.3.7` は
オフライン境界には置くが sandbox ではなく、細工・破損した protobuf の全経路を
panic-free にする保証はない。恒久対応は [Issue #23](../../../issues/23) で追跡する。
変換側が所有する候補・node collection は fallible に確保し、FSAP の候補 record 数を
PBF 走査中から 1,000,000 以下へ制限する。これは `osmpbf` 内部の allocation まで保証する
ものではない。変換結果は一時ファイルへ完全に書いてから置換するため、parser・上限・
確保のどこで失敗しても入力 PBF と既存 DB は破壊しない。

OSM DB を実際に読み込んだときだけ、ゲーム画面に
`Airport data: (c) OpenStreetMap contributors (ODbL)` を表示する。詳細 URL と
ライセンス説明は配布物に含まれる `ATTRIBUTION.md` に置く。

### OSM element の境界を feature ごとに固定する

`aeroway=runway` と `aeroway=taxiway` の**中心線 way**を対象とする。

- 滑走路は `area=yes` と、先頭・末尾 node が同じ閉じた way を面形状として除外する
- way の先頭 node を進入端、末尾 node を反対端とする
- 両端から真方位と長さを導出する。逆向きの利用は既存の反方位処理で扱う
- `width=*` はメートルとして読み、明示的な `m` / `ft` 接尾辞も境界で変換する
- 滑走路の幅が無い、有限でない、または正でない場合は 45 m を使い、件数を報告する
- node 欠落、非有限座標、同一点の両端など、滑走路を作れない way は理由別に数えて除外する

誘導路は OSM way の全 node を順番どおり保持する。`area=yes` は除外するが、先頭と末尾が
同じ閉じた中心線は有効なループとして受け入れる。幅の欠落・不正時は 15 m を使う。
参照 node が一つでも欠ける、座標が一つでも不正、点が 2 個未満、または隣接点が縮退する
way は、途中まで採用せず way 全体を理由別に数えて除外する。

面形状から幅を推定する案は採らない。中心線と面の重複排除、曲がったポリゴン、
日付変更線をまたぐ外接形状まで同時に解く必要があり、最初の実空港対応としては
責務が大きすぎる。

apron は次の二つだけを対象にする。

- 先頭と末尾が同じ閉じた `aeroway=apron` way
- `type=multipolygon + aeroway=apron` relation の `outer` / `inner` way

relation member は OSM way ID 順を基準に、必要なら向きを反転して閉じた ring へ接続する。
欠けた member、未知 role、閉じない ring、自己交差、不正な hole は feature 全体を除外する。
hole を残して三角形分割し、三角形の各辺が最大 75 m になるまで細分する。これは実行時に
各頂点で DEM を引いたとき、大きな面が平らな板にならないためである。`surface=*` は
asphalt / concrete / paved / grass / gravel / dirt / sand の限定列挙へ写像し、未知値は
`Unknown` とする。

待機位置は `aeroway=holding_position` の node / way、および
`aeroway=aerodrome_marking + aerodrome_marking=holding_position` の way を対象にする。
node は誘導路 node の共有を優先し、無ければ幅 + 1 m の corridor 内で距離、way ID、
segment index の順に最寄り誘導路の線分、方位、幅へ関連付ける。候補が無い node は除外する。
way は両端の中点・向き・長さから、誘導路を横断する路面標示として保存する。待機位置自身の
`ref` を優先し、無ければ関連誘導路の `ref` を補う。滑走路側を判定できた場合だけ、路面に
2 本の実線と 2 本の破線を描く。滑走路側を捏造できない場合は標示を省略する。

地上灯火は node の `aeroway=navigationaid` と `navigationaid=txe|txc|rgl` を対象にし、
それぞれ誘導路縁灯・中心線灯・滑走路警戒灯として保持する。明示的な TXE / TXC が
誘導路 corridor にある場合は同じ channel の procedural fallback だけを抑止する。
明示点が無い誘導路は edge / centerline を決定論的に補うが、`lit=no` は必ず無灯火のまま
にする。

標識は独立した OSM feature を推測せず、待機位置と関連誘導路の `ref` から生成する。
画面に出す文字列は Bevy の既定フォントで欠けない ASCII に限定する既存規約があるため、
物理標識もコード内の 3x5 ASCII glyph geometry とする。UTF-8 の `ref` は DB に保持するが、
非 ASCII、8 文字超、未収録文字を含む場合は標識だけを省略する。

### 実行時形式 `.fsairports`

リトルエンディアン固定。FSAP v1 は 24 バイトのヘッダと 48 バイトの滑走路レコード、
FSAP v2 は同じヘッダと 64 バイトの種別付き固定長レコードからなる。FSAP v3 は同じ
24 バイトのヘッダに 32 バイトの directory entry と 7 種の section を続ける。
reader / writer は v1 / v2 の byte 表現を変えずに扱い、`flightsim-airportgen` は地上設備を
含む v3 を出力する。

```text
ヘッダ
オフセット サイズ  フィールド
     0       4    マジック `FSAP`
     4       2    フォーマット版 (u16、1、2、3)
     6       2    フラグ (u16、0 のみ)
     8       4    レコード数（v1/v2）または section 数（v3）(u32)
    12       4    record size（v1 48、v2 64）または directory entry size（v3 32）
    16       8    ペイロードの FNV-1a チェックサム (u64)

v1 レコード（滑走路のみ）
オフセット サイズ  フィールド
     0       8    OSM way ID (i64)
     8       8    進入端の緯度 (f64、度)
    16       8    進入端の経度 (f64、度)
    24       8    反対端の緯度 (f64、度)
    32       8    反対端の経度 (f64、度)
    40       8    幅 (f64、m)

v2 レコード
オフセット サイズ  フィールド
     0       1    種別 (u8、0 = 滑走路、1 = 誘導路)
     1       7    予約領域 (0 のみ)
     8       8    OSM way ID (i64)
    16       4    segment index (u32、滑走路は 0、誘導路は way 内で 0 から連番)
    20       4    record flags (u32、0 のみ)
    24       8    線分始点の緯度 (f64、度)
    32       8    線分始点の経度 (f64、度)
    40       8    線分終点の緯度 (f64、度)
    48       8    線分終点の経度 (f64、度)
    56       8    幅 (f64、m)

v3 directory entry
オフセット サイズ  フィールド
     0       2    section kind (u16)
     2       2    schema version (u16、既知 section は 1)
     4       4    section flags (u32、core は 0、既知 optional は 1)
     8       4    1 record のバイト数 (u32)
    12       4    record 数 (u32)
    16       8    payload 先頭から section 先頭までの offset (u64)
    24       8    section の byte 長 (u64)
```

v3 の payload は directory 全体と直後に連続する section data 全体で、header の checksum は
そのすべてを対象にする。section kind と固定 record size は次のとおり。

| kind | section | record size | 内容 |
|---:|---|---:|---|
| 1 | core | 64 | v2 と同じ滑走路・誘導路 segment record（必須） |
| 2 | apron triangle | 64 | source kind / ID、surface、三角形 3 頂点 |
| 3 | holding position | 64 | source、位置、種別、方位、幅、文字列・誘導路参照、滑走路側 |
| 4 | ground light | 40 | source node、位置、TXE / TXC / RGL 種別 |
| 5 | taxiway attribute | 24 | way ID、`ref`、surface、補完すべき灯火 channel |
| 6 | string index | 8 | UTF-8 bytes 内の offset と長さ |
| 7 | string bytes | 1 | canonical UTF-8 文字列表 |

既知 section は kind 1〜7 の昇順に一度ずつ書く。core は required、残りは optional flag を
立てる。未知 required section は拒否し、未知 optional section は schema を解釈せず飛ばせる。
reader / writer は固定長 record の全 section 合計を 1,000,000、canonical string
bytes と各 feature への参照を展開した合計をそれぞれ 16 MiB、payload 全体を
96 MiB、section 数を 16 以下に制限する。directory を先に読み、これらの
上限と `count * record_size` を確認してから data を確保する。

緯度経度は OSM の外部データ境界に合わせて度で保存し、読み込み時に
`flightsim-core::Geodetic::from_degrees` へ渡す。レコードに方位・長さを重ねて
保存しない。両端と派生値を二重に持つと、不整合したファイルに二つの正解ができる。
誘導路は隣接する node ごとに 1 record とし、同じ way ID の segment を連結して元の
折れ線へ戻す。固定長のまま全 node を保存でき、任意長 payload の入れ子を reader に
持ち込まずに済む。

**標高は保存しない。** アプリが選択した滑走路の進入端で、実際に読み込んだ
地形の高さを引き、その数値を滑走路全体へ適用する。これは絶対高度の基準を修正する
処理ではなく、滑走路の描画・接地判定を現在の地形面に局所的に揃える処理である。
誘導路は長い折れ線の端が地形へ埋まらないよう、各 node で DEM を引いてからメッシュを
作る。apron は最大辺 75 m 以下へ細分した各三角形頂点、待機位置・標識・地上灯火は
各配置点で DEM を引く。地形データが無い場合は、既存契約どおり 0 m を使う。
z-fighting を避ける lift は apron surface 0.04 m、誘導路 surface 0.06 m、滑走路 surface
0.08 m、誘導路中心線 0.11 m、待機位置標示 0.115 m、灯火 0.12 m、滑走路標示 0.13 m に
固定する。

未知の版・フラグ・レコード長、サイズ不一致、末尾の余分なデータ、checksum 不一致、
未知の種別、非ゼロの予約領域、不正な座標・幅・縮退した線分は panic せず読み込みエラーに
する。さらに誘導路は segment index が 0 から連続すること、全 segment の幅が一致すること、
前 segment の終点と次 segment の始点が一致することを厳格に検査する。v3 は加えて section
kind の厳密な昇順・一意性、schema / flags / record size、連続かつ非重複の範囲、byte 長、
予約領域、UTF-8、文字列 index の範囲と一度だけの参照、参照先 feature の存在を検査する。

### 選択滑走路の 15 km 圏だけ地上設備を描く

FSAP は空港 relation に依存せず個々の feature を保持するため、地域 extract 全域を一度に
描かない。active runway の中心から 15 km 圏と交差する誘導路・apron、および圏内の
待機位置・地上灯火だけを起動時に選ぶ。誘導路は node に加えて線分との最短距離、apron は
三角形との最短距離を見る。全頂点が圏外でも線や面が探索圏を横切る feature を落とさない。

誘導路は舗装面と黄色中心線を way ごとに一つのメッシュとして描く。各線分を幅付きの帯へ
広げ、曲がり角と端点は円形の継ぎ目で塞ぐ。apron、待機位置標示、ASCII 物理標識、灯火も
feature 単位または色単位に束ね、OSM node ごとの entity 増加を避ける。

### 最寄り滑走路を決定論的に選ぶ

検索地点と各滑走路中心の ECEF 直線距離が最小のものを選ぶ。同距離なら OSM way ID の
小さい方を選ぶ。入力順や `HashMap` の反復順に依存させない。

`--airports` だけを指定した場合は、従来の合成飛行場の開始地点を検索地点とし、
選択した滑走路の離陸開始点と方位へ機体を置く。`--start` を明示した場合はその地点を
検索と spawn の両方に使い、`--heading` が無ければ選択滑走路の方位を使う。
`--approach` は選択滑走路から進入状態を作る。

## 却下した案

### 生 PBF をアプリで直接読む

実行時依存と起動時間が増え、ADR-0003 のオフライン境界を破る。PBF の way は node ID を
参照するため、地域 extract でも依存解決のための索引または複数走査が要る。これは
フレーム側ではなくツール側の仕事である。

### JSON / CSV を実行時形式にする

人が読める利点はあるが、文字列の表記揺れ・途中書き込み・未知フィールドの扱いが増える。
数値 geometry は固定長 record、`ref` は範囲検査できる canonical string table とし、
版・section schema・長さ・checksum で壊れ方を明確にできる。JSON の柔軟性を実行時へ
持ち込む必要はない。内容確認用の dump が必要になった時点で CLI に追加する。

### OSM 派生 DB を prerelease に同梱する

利用者には最も簡単だが、対象地域、更新周期、データ量、ODbL の派生 DB 提供方法が未決定。
まずパイプラインと利用経路を成立させ、配布は別判断にする。

## 帰結

**受け入れるコスト**

- 利用者は地域 PBF を別途用意して変換する必要がある
- PBF parser は sandbox ではないため、出所を信頼できない入力は扱わない
- v3 に空港名、滑走路 `ref`、建物、運用情報は入らない
- 幅欠落時の 45 m は安全な見た目の fallback であり、小規模・未舗装滑走路には広すぎる
- 誘導路の 15 m fallback は実際の幅を保証しない。`surface=*` は限定列挙への写像で、
  未知値の材質を再現しない
- 明示灯火が無いときの procedural 灯火は、運用上の実在・種類・間隔を保証するデータではない
- ASCII 物理標識は待機位置・誘導路の短い `ref` が両方必要で、OSM の全標識を再現しない
- DEM を平坦化しないため、起伏の強い場所では滑走路メッシュが地形へ埋まる可能性がある
- Copernicus DEM の EGM2008 標高を WGS84 楕円体高へ変換していないため、
  絶対高度にはジオイド高相当の系統誤差が残る（[Issue #22](../../../issues/22)）
- OSM way の向きは運用上の優先進入端を表さない。初期 spawn の向きはデータ順になる

**得られる保証**

- PBF パーサーはオフラインクレートだけに載り、実行時依存へ入らない
- 同じ PBF から同じ順序・同じ bytes の DB が生成される
- v1 / v2 の既存 bytes を読み書きでき、v3 の壊れた section・参照・文字列を黙って使わない
- 開始、進入、描画、灯火、着陸評価が一度選んだ同じ滑走路を見る
- 誘導路は OSM の全 node と順序を保ち、apron hole も保持する。active runway 周辺の
  地上設備だけを地形に沿って描く
- `lit=no` は procedural fallback を発生させず、明示 TXE / TXC と同じ channel を重ねない
- 画面と物理標識へ非 ASCII glyph を渡さない
- OSM を使わない従来起動には帰属表示も挙動変更も入らない

## 再検討条件

- 空港名・滑走路 `ref`・建物・運用情報を実装するときは、新 section または次版を検討する
- 物理標識を OSM の独立した sign mapping から取り込む場合は、安定した tagging と
  両面・矢印・複数 panel の表現を別設計する
- OSM 派生 DB を配布するときは、ODbL の提供方法とデータ provenance を別 ADR で決める
- ジオイドモデルを導入したら、ソースごとの鉛直基準を検証し、実 DEM と
  信頼できる OSM 標高を WGS84 楕円体高へ変換する経路を検討する
- 実地形で滑走路の浮き沈みが許容できなければ、tilegen 側で DEM 平坦化を追加する
