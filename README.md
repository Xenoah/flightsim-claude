# flightsim-claude

地球規模のフライトシミュレータ。Rust + Bevy、Windows 対象。

**現状は M1（物理・地形の基盤）まで。描画はまだありません。** 何が動いて何が動かないかは
[ARCHITECTURE.md §7](ARCHITECTURE.md#7-現状のスコープ) を参照してください。

---

## 何ができるか（今）

```bash
cargo test --workspace           # 約 200 件、数秒で完了
```

- **WGS84 測地系と `f64` ECEF 世界座標** — 地球全体で振動しない位置表現。描画用の
  floating origin 付き
- **6DoF 飛行力学モデル** — ISA 標準大気、緯度依存の正規重力、失速を含む空力モデル、
  RK4 固定ステップ積分、3 点式着陸装置、接地摩擦とブレーキ。決定論的で、離着陸と
  10 分の飛行を数秒でヘッドレス検証できる
- **地形タイル基盤** — 地理座標系クアッドツリー（極まで表現可能）、DEM のバイリニア
  サンプリング、幾何誤差ベースの LOD 選択、フレーム予算つきストリーミング

```bash
cargo run -p flightsim-fdm --example aero_trace   # 空力の内訳を時系列で表示
```

## 何がまだないか

描画、実地形データの読み込み、天候、複数機体、オンライン。
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

設計の要は 1 点に集約されます。

> **`flightsim-core` / `flightsim-fdm` / `flightsim-world` は Bevy に依存しない。**

物理と地形が純 Rust であるおかげで、`cargo test` が GUI もアセットもなしに数秒で回ります。
これは慣習ではなく [CI で検査される規約](scripts/check-architecture.sh) です。

```text
                    ┌─────────────────┐
                    │ flightsim-app   │
                    └────────┬────────┘
          ┌──────────────┬───┴───┬──────────────┐
          ▼              ▼       ▼              ▼
       render         input     ui            net        ← Bevy 依存層
          └─────────────┴───────┬┴──────────────┘
                                ▼
                   ┌────────────┴────────────┐
                   ▼                         ▼
                 world                      fdm          ← 純 Rust
                   └────────────┬────────────┘
                                ▼
                              core
```

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

MIT OR Apache-2.0
