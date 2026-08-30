# ATTRIBUTION

このプロジェクトが利用するデータとその帰属表示。

> **これは法的義務です。** OpenStreetMap（ODbL）と ESA WorldCover（CC BY 4.0）は
> 帰属表示を必須としています。データソースを追加したら**必ずこのファイルを更新し、
> ゲーム内のクレジット画面にも反映すること。** 実装漏れを許さない項目です。

---

## 現在利用しているデータ

### 標高 — Copernicus DEM GLO-30

`flightsim-tilegen` が読み込む対象です。**焼いたタイルを配布する場合、
この表示をゲーム内クレジットにも出すこと。**

> Produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus Defence and
> Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA;
> all rights reserved.

なお、リポジトリに実データは含まれていません（全球で数百 GB あるため）。
テストは合成 GeoTIFF で動いており、CI は実データを必要としません。

### 空港・滑走路 — OpenStreetMap

`flightsim-airportgen` は、利用者が用意した地域 `.osm.pbf` から
`aeroway=runway` の中心線を実行時空港 DB へ変換します。

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
派生 DB を公開・配布する人は ODbL の share-alike 条件を確認してください。

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
| `assets/aircraft/light_single.glb` | 2026-08-21、Meshy text-to-3D（preview → refine） | **有料プラン。再配布可** |

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
