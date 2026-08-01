# ATTRIBUTION

このプロジェクトが利用するデータとその帰属表示。

> **これは法的義務です。** OpenStreetMap（ODbL）と ESA WorldCover（CC BY 4.0）は
> 帰属表示を必須としています。データソースを追加したら**必ずこのファイルを更新し、
> ゲーム内のクレジット画面にも反映すること。** 実装漏れを許さない項目です。

---

## 現在利用しているデータ

**なし。**

M1 時点ではタイル生成パイプラインが未実装のため、実データを一切読み込んでいません。
テストは全て合成データで動いています。

---

## 利用予定のデータ（[ADR-0003](docs/adr/0003-terrain-data.md)）

パイプライン実装時にここへ移し、ゲーム内クレジットにも追加すること。

### 標高

**Copernicus DEM GLO-30**
Produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus Defence and
Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA;
all rights reserved.

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

## ソフトウェア

| 依存 | ライセンス |
|---|---|
| [glam](https://github.com/bitshifter/glam-rs) | MIT OR Apache-2.0 |

本プロジェクト自体は MIT OR Apache-2.0 です。
