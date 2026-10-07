# ATTRIBUTION

このプロジェクトが利用するデータとその帰属表示。

> **これは法的義務です。** OpenStreetMap（ODbL）と ESA WorldCover（CC BY 4.0）は
> 帰属表示を必須としています。データソースを追加したら**必ずこのファイルを更新し、
> ゲーム内のクレジット画面にも反映すること。** 実装漏れを許さない項目です。

---

## 現在利用しているデータ

### 同梱の全球ベース地形 — NOAA ETOPO 2022 / Copernicus GLO-90 / Natural Earth

> Global terrain: NOAA NCEI ETOPO 2022 (CC0), NGA-derived EGM2008 geoid (public domain), Natural Earth (public domain), modified Copernicus WorldDEM-90.

同梱の `global-terrain.fsgt` は ETOPO 2022 の ice-surface 標高と対応する EGM2008
geoid に、独立した湖・海岸の surface correction を加えた粗い全球ベースです。
60 秒角格子を stride 10 で取得し、2048×1024 の整列格子へ再標本化しています。
最終間隔は **10.546875 分角（赤道で約 19.5 km）** で、30 m DEM や衛星写真ではありません。
Natural Earth の陸地・湖 polygon で dry land / inland water / ocean を区別します。
海では H=0、内水は補正した静的な水面標高、陸地は負の標高も保持し、
WGS84 楕円体高を `h = H + N` として求めます。海岸・小島・現在の湖面水位を精密に
表すものではありません。ソース・ハッシュ・補正・再標本化は provenance に記録します。

Copernicus 由来の変更データを含むため、全球ベースは全体として単に public domain
ではありません。配布時は以下の required notices と license text を保持してください。

> produced using Copernicus WorldDEM™-90 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA; all rights reserved

> The organisations in charge of the Copernicus programme by law or by delegation do not incur any liability for any use of the Copernicus WorldDEM™-90

- 再配布する notices: [NOTICE-GLOBAL-TERRAIN.txt](docs/data/NOTICE-GLOBAL-TERRAIN.txt)
- 同梱の Copernicus GLO-90 license: [copernicus-glo90-license.pdf](docs/data/copernicus-glo90-license.pdf)
- 正確なデータ出典・処理: [global-sources.md](docs/data/global-sources.md)
- NOAA NCEI ETOPO 2022: https://www.ncei.noaa.gov/products/etopo-global-relief-model
- ETOPO 2022 metadata / CC0-1.0: https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ngdc.mgg.dem:etopo_2022
- EGM2008 geoid metadata: https://www.ngdc.noaa.gov/thredds/dodsC/global/ETOPO2022/60s/60s_geoid_netcdf/ETOPO_2022_v1_60s_N90W180_geoid.nc.das
- Natural Earth public-domain terms: https://www.naturalearthdata.com/about/terms-of-use/

### Balzers 開発用地域サンプル — Copernicus GLO-90 / NOAA geoid

[Balzers Terrain Package](docs/examples/terrain-packages/balzers/README.md) は
復旧した Liechtenstein サンプルから元の DEM 170 タイルを変更せず選択した地域データです。
GLO-90 の 90 m 級 DSM と NOAA ETOPO2022 EGM2008 geoid 由来で、全球ベースとは
別の成果物です。精度・範囲・再構成の限界は同ページに記録しています。

この ZIP の地形にはリポジトリの MIT/Apache ソフトウェアライセンスではなく、
[Copernicus GLO-90 一般公開ライセンス](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf)
（PDF 19–21 ページ）が適用されます。元のライセンス全文、変更データのクレジット、
免責と再配布時の義務は ZIP 内の `docs/copernicus-license-bundle.txt` と
`docs/notice.txt` に保持しています。出典・処理は `docs/source-provenance.md`、
今回の抽出は `docs/balzers-reconstruction.md` を参照してください。
公開時もこれらの記録を保持してください。商用・Windows 配布の審査は別途必要です。

### 標高 — Copernicus DEM GLO-30

`flightsim-tilegen` が読み込む対象です。**焼いたタイルを配布する場合、
この表示をゲーム内クレジットにも出すこと。**

> Produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus Defence and
> Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA;
> all rights reserved.

なお、リポジトリに実データは含まれていません（全球で数百 GB あるため）。
テストは合成 GeoTIFF で動いており、CI は実データを必要としません。

### 空港・滑走路・地上設備 — OpenStreetMap

`flightsim-airportgen` は、利用者が用意した地域 `.osm.pbf` から
滑走路・誘導路に加え、エプロン、待機位置、誘導路標識に使う `ref`、
TXE / TXC / RGL 地上灯火を実行時空港 DB へ変換します。これらはすべて
OpenStreetMap 由来データであり、変換後も ODbL の対象です。

> Airport data: © OpenStreetMap contributors

OpenStreetMap のデータは Open Data Commons Open Database License
（ODbL）v1.0 で提供されています。

- 帰属・データソース: https://www.openstreetmap.org/copyright
- ODbL v1.0: https://opendatacommons.org/licenses/odbl/1-0/
- ゲーム・シミュレーション向け表示指針:
  https://osmfoundation.org/wiki/Licence/Attribution_Guidelines

OSM の PBF と変換後の派生 DB は、リポジトリにも prerelease にも**同梱しません**。
OSM 空港 DB を実際に読み込んだ場合だけ、ゲーム画面にも
`Airport data: (c) OpenStreetMap contributors (ODbL)` と表示します。
画面表示は Bevy の既定フォントで欠けない ASCII に限定し、詳細 URL とライセンス本文は
このファイルを Windows 配布物へ同梱して示します。派生 DB を公開・配布する人は、
ODbL の attribution・notice・share-alike 条件を確認してください。

---

## 利用予定のデータ（[ADR-0003](docs/adr/0003-terrain-data.md)）

パイプライン実装時にここへ移し、ゲーム内クレジットにも追加すること。

### 地表画像

**Sentinel-2 (Copernicus Sentinel data)**
Contains modified Copernicus Sentinel data [年].

**Natural Earth** — パブリックドメイン。帰属表示は任意だが記載する。

### 土地被覆

**ESA WorldCover** — © ESA WorldCover project / Contains modified Copernicus
Sentinel data
CC BY 4.0。https://esa-worldcover.org/

---

## 生成した 3D モデル

`flightsim-assetgen` は [Meshy](https://www.meshy.ai/) の API でモデルを生成する。

**生成物の権利と利用条件は Meshy の契約プランに従う。** 配布する前に、
使用したプランの規約で商用利用・再配布が許されているかを確認すること。
生成物をリポジトリに含める場合は、どのプランで生成したかをここに記録する。

### 含めているモデル

| ファイル | 生成 | プラン |
|---|---|---|
| `assets/aircraft/light_single.glb` | 2026-08-21、Meshy text-to-3D（preview → refine） | 過去の記録: **有料プラン・再配布可**。生成時の根拠資料は未検証 |

**2026-10-02 商用配布監査での留保:** 上のプラン記録は履歴として残していますが、
生成 task、当日のプラン・適用規約、入力素材の権利を確認する資料は、今回の
リポジトリ監査では検証できていません。このモデルを商用配布可と改めて認定する
ものではありません。根拠が揃うまで、商用候補の staging からは除外し、
オリジナルの Swift Sport を使います。後日の一般規約や Meshy のクレジット追記だけで
この既存モデルを CC BY と再分類しないでください。
必要な証拠と残る配布条件は [商用配布監査](docs/release/commercial-distribution-audit.md) を参照。

軽単発機。4.75 MB、頂点 29,327、ベースカラー JPEG 1 枚（法線マップは無い）。
モデル座標系は **前 = −X、上 = +Y**（glTF の慣習である −Z 前方とは違う）。

**プランを変えたモデルを足すときは、この表に行を足すこと。** どのモデルが
どの条件で入ったのかが分からなくなると、リポジトリ全体を再配布できなくなる。

`assets/aircraft/` の他のファイルは `.gitignore` 対象。preview 段階の中間生成物は
`--refine` で作り直せるので入れていない。

---

## ソフトウェア

| 依存 | ライセンス | 使う場所 |
|---|---|---|
| [glam](https://github.com/bitshifter/glam-rs) | MIT OR Apache-2.0 | 全体（線形代数） |
| [tiff](https://github.com/image-rs/image-tiff) | MIT | `flightsim-tilegen`（GeoTIFF デコード） |
| [clap](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | `flightsim-tilegen` / `flightsim-airportgen` / `flightsim-headless`（CLI） |
| [osmpbf](https://github.com/b-r-u/osmpbf) | MIT OR Apache-2.0 | `flightsim-airportgen`（OSM PBF デコード） |
| [same-file](https://github.com/BurntSushi/same-file) | Unlicense OR MIT | `flightsim-airportgen`（入出力の同一ファイル検出） |
| [tempfile](https://github.com/Stebalien/tempfile) | MIT OR Apache-2.0 | `flightsim-airportgen`（DB の原子的な置換） |
| [bevy](https://bevyengine.org/) | MIT OR Apache-2.0 | 描画層（[ADR-0007](docs/adr/0007-bevy-version.md) で 0.18.1 に固定） |
| [ureq](https://github.com/algesten/ureq) | MIT OR Apache-2.0 | `flightsim-assetgen`（HTTP） |
| [criterion](https://github.com/bheisler/criterion.rs) | MIT OR Apache-2.0 | ベンチ（dev-dependency） |

`tiff` と `osmpbf` はオフライン生成専用。`same-file` と `tempfile` も空港 DB 生成専用。
`clap` はオフライン生成 CLI とヘッドレスランナーが使う。いずれも
`flightsim-app` の実行時依存には載らない（[ADR-0003](docs/adr/0003-terrain-data.md)）。

本プロジェクト自体は [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE) です。
この記述で第三者のデータ・モデル・フォント・依存ソフトウェアの個別条件を
MIT / Apache-2.0 に置き換えることはありません。配布対象と一致する依存関係の
ライセンス本文・著作権表示は `scripts/collect-dependency-notices.py` で収集し、
未解決項目と最終ターゲットの条件を別途確認してください。

### Bevy に埋め込まれるフォント・描画 LUT

- **Fira Mono:** Bevy 0.18.1 の `default_font` が subset フォントを実行ファイルへ
  埋め込みます。The Mozilla Foundation and Telefonica S.A. による
  SIL Open Font License 1.1 の [原文](docs/release/licenses/FiraMono-LICENSE) を
  同梱してください。これは Bevy のソースコード向け MIT / Apache 条件とは別です
- **Tony McMapface:** `tonemapping_luts` が埋め込む LUT の、Tomasz Stachowiak による
  [MIT の原文](docs/release/licenses/TonyMcMapface-LICENSE-MIT) を保存しています
- **AgX / Blender Filmic LUT:** 同じ feature で埋め込まれます。特定の上流素材の
  権利根拠・版の記録には未解決項目があります。Bevy の crate ライセンス欄だけで
  全素材が確認済みとはせず、[監査の残項目](docs/release/commercial-distribution-audit.md)
  を商用配布前に解決してください

## Original Swift Sport aircraft (2026-10-01)

`assets/aircraft/swift_sport.glb` and its editable `.blend` are original procedural geometry created for this repository in Blender 4.3.2. The reproducible source is `tools/blender/build_swift_sport.py`. No third-party mesh, texture, logo or branded aircraft design is included. These assets and the source script use this repository's MIT OR Apache-2.0 license.

Swift Sport is a generic two-seat sport aircraft, not certified data for any real aircraft. Its representative dynamics, camera and input settings are in `assets/aircraft/swift_sport.json`; the external model measures approximately7.12m long and9.4m span. The legacy Light Single model keeps its existing attribution and redistribution terms.

## NOAA monthly climate

Climate data provided by NOAA Physical Sciences Laboratory, Boulder, Colorado, USA:
NCEP/NCAR Reanalysis 1 monthly 1991-2020 long-term means. These public-domain federal
data were transformed into a compact offline atlas; no NOAA endorsement is implied.

- https://www.psl.noaa.gov/data/help/
- [Exact fields, processing and scientific limits](docs/data/global-climate.md)

These are reanalysis climatology, not live weather, forecast or airport observations.
Cloud fraction is a reanalysis field. Temperature altitude adjustment, broad biome
labels, cloud-layer geometry and snow/ice visual cues are modeled approximations.

## Original Meadow Trainer exterior (2026-10-04)

`assets/aircraft/meadow_trainer.glb`, its editable `.blend`, and
`tools/blender/build_meadow_trainer.py` are original procedural geometry/materials
created for this project in Blender 4.3.2. No downloaded model, Meshy output,
texture, logo, blueprint or branded aircraft is included. These new original
files, their studio previews, and external JSON profile use this repository's
MIT OR Apache-2.0 license. The profile reuses Light Single's existing numeric
dynamics and controls exactly; it does not add newly calibrated real-aircraft
performance data. See [provenance, axes and validation](docs/aircraft/meadow-trainer.md).
This addition does not alter the existing models' terms or the commercial
candidate's asset allowlist, and does not constitute release or rights clearance.
