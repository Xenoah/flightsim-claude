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

---

## 利用予定のデータ（[ADR-0003](docs/adr/0003-terrain-data.md)）

パイプライン実装時にここへ移し、ゲーム内クレジットにも追加すること。

### 空港・滑走路・建物

**OpenStreetMap** — © OpenStreetMap contributors
ODbL v1.0 で提供。https://www.openstreetmap.org/copyright

派生データベースを配布する場合、ODbL の share-alike 条項が適用されます。
**配布形態を決める前にライセンス条件を確認すること。**

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

現時点でリポジトリに含めているモデルは無い（`assets/` は `.gitignore` 対象）。

---

## ソフトウェア

| 依存 | ライセンス | 使う場所 |
|---|---|---|
| [glam](https://github.com/bitshifter/glam-rs) | MIT OR Apache-2.0 | 全体（線形代数） |
| [tiff](https://github.com/image-rs/image-tiff) | MIT | `flightsim-tilegen`（GeoTIFF デコード） |
| [clap](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | `flightsim-tilegen`（CLI） |
| [bevy](https://bevyengine.org/) | MIT OR Apache-2.0 | 描画層（[ADR-0007](docs/adr/0007-bevy-version.md) で 0.18.1 に固定） |
| [ureq](https://github.com/algesten/ureq) | MIT OR Apache-2.0 | `flightsim-assetgen`（HTTP） |
| [criterion](https://github.com/bheisler/criterion.rs) | MIT OR Apache-2.0 | ベンチ（dev-dependency） |

`tiff` と `clap` はオフラインのタイル生成ツールのみが使い、実行時には載りません
（[ADR-0003](docs/adr/0003-terrain-data.md)）。

本プロジェクト自体は MIT OR Apache-2.0 です。
