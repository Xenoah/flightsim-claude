#!/usr/bin/env bash
#
# ARCHITECTURE.md の依存規約を機械的に検査する。
#
# 規約を人間の注意力で守らせると必ず破られる。破られた時点でビルドを落とすのが
# このスクリプトの役目。CI から呼ばれるが、ローカルでも実行できる。
#
#   ./scripts/check-architecture.sh
#
set -uo pipefail

failures=0

fail() {
    echo "FAIL: $1"
    echo "      $2"
    failures=$((failures + 1))
}

# 検査対象のクレート。
CRATES=(flightsim-core flightsim-fdm flightsim-world flightsim-tilegen flightsim-sim flightsim-assetgen)

# `cargo tree` の出力からパッケージ名の一覧を得る。
# --edges normal で dev-dependencies と build-dependencies を除外する
# （テスト専用の依存は設計上の問題ではない）。
deps_of() {
    cargo tree --package "$1" --edges normal --prefix none 2>/dev/null \
        | awk '{print $1}' \
        | sort -u
}

# --------------------------------------------------------------------------
# 事前検査: 依存グラフがそもそも読めること
#
# deps_of は cargo tree の失敗を握り潰す（パイプラインの中で使うので、
# 関数内から exit しても親シェルは止まらない）。その結果、マニフェストに
# 誤りがあると **全ての規約検査が「依存ゼロ」として黙って通る**。
#
# 安全網が事故時に無言で外れるのが最悪なので、ここで先に落とす。
# 実際にこの穴を踏んで気付いたため、検査を足してある。
# --------------------------------------------------------------------------
for crate in "${CRATES[@]}"; do
    if ! output=$(cargo tree --package "$crate" --edges normal --prefix none 2>&1); then
        echo "FAIL: could not resolve the dependency tree for $crate"
        echo "      Every architecture check would pass vacuously in this state, so this is fatal."
        echo "      cargo tree said:"
        echo "$output" | sed 's/^/        /'
        exit 2
    fi
done

# --------------------------------------------------------------------------
# 規約 1: core / fdm / world / sim / tilegen は Bevy に依存しない（ADR-0001）
#
# これらが GUI なしにテストできることが技術選定の根拠そのもの。
# ここが崩れると QA エージェントが回帰網を維持できなくなる。
# --------------------------------------------------------------------------
for crate in "${CRATES[@]}"; do
    if deps_of "$crate" | grep -qiE '^bevy'; then
        fail "$crate depends on bevy" \
             "ADR-0001: these crates must stay engine-independent so that \`cargo test\` runs headless. \
The headless runner (sim) and the offline baker (tilegen) have no use for a render engine either."
    fi
done

# --------------------------------------------------------------------------
# 規約 2: 依存は一方向のみ
#
# core ← fdm / world ← render / input / ui ← app
# 逆流も横断も禁止（ARCHITECTURE.md §2）。
# --------------------------------------------------------------------------
if deps_of flightsim-core | grep -qE '^flightsim-(fdm|world|render|input|ui|audio|app|net)$'; then
    fail "flightsim-core depends on a higher-level crate" \
         "core is the bottom of the dependency graph and must depend on nothing in this workspace."
fi

if deps_of flightsim-fdm | grep -qE '^flightsim-world$'; then
    fail "flightsim-fdm depends on flightsim-world" \
         "The FDM must receive terrain elevation as an argument, not fetch it. See .claude/agents/simulation.md."
fi

if deps_of flightsim-world | grep -qE '^flightsim-fdm$'; then
    fail "flightsim-world depends on flightsim-fdm" \
         "world and fdm are siblings; neither may depend on the other."
fi

# flightsim-tilegen はオフラインのツールで、world の上に乗る。
# 逆に runtime 側がツールへ依存すると、実行時に GeoTIFF デコーダを抱え込むことになる
# （ADR-0003 が禁じているもの）。
for crate in flightsim-core flightsim-fdm flightsim-world; do
    if deps_of "$crate" | grep -qE '^flightsim-tilegen$'; then
        fail "$crate depends on flightsim-tilegen" \
             "tilegen is an offline tool that sits above world. Runtime crates must not pull in the GeoTIFF decoder (ADR-0003)."
    fi
done

# flightsim-sim は fdm と world の上に乗る統合層（ADR-0006）。
# 下位クレートが sim を参照すると、FDM 単体のテストが地形データを要求するようになり、
# 「fdm は world を参照しない」という規約が実質的に骨抜きになる。
#
# 注意: sim / tilegen への上向き依存は、実際には Cargo が循環依存として先に拒否する
# （sim は fdm と world に依存しているため）。したがってこの 2 つの検査が発火する
# ことはまずなく、実質は上の事前検査が守っている。規約を明文化するために残してある。
for crate in flightsim-core flightsim-fdm flightsim-world; do
    if deps_of "$crate" | grep -qE '^flightsim-sim$'; then
        fail "$crate depends on flightsim-sim" \
             "sim is the integration layer above fdm and world. Depending on it upwards defeats the fdm/world separation (ADR-0006)."
    fi
done

# --------------------------------------------------------------------------
# Bevy 依存層（render / input / ui / audio）は互いに依存しない。
#
# 同階層どうしが繋がると、片方を差し替えるのにもう片方が付いてくる。
# 例: 音が ui に依存すると、HUD を持たない構成で音が鳴らせなくなる。
# 繋ぐ必要があるものは app が両方を知って結線する（ARCHITECTURE.md §2）。
# --------------------------------------------------------------------------
SIBLINGS=(flightsim-render flightsim-input flightsim-ui flightsim-audio)
for crate in "${SIBLINGS[@]}"; do
    for other in "${SIBLINGS[@]}"; do
        [ "$crate" = "$other" ] && continue
        if deps_of "$crate" | grep -qE "^$other\$"; then
            fail "$crate depends on its sibling $other"                  "render / input / ui / audio are siblings. Wire them together in app instead (ARCHITECTURE.md 2)."
        fi
    done
done

# --------------------------------------------------------------------------
# 規約 3: 座標変換は flightsim-core にのみ置く（ADR-0002）
#
# 各クレートが独自に三角関数で測地変換を書くと、丸めと特異点の扱いが分岐し、
# 原因特定が極めて困難なズレになる。
#
# 完全な静的解析はできないので、測地変換の定数（WGS84 の離心率など）が
# core の外に現れていないかを見る。ヒューリスティックだが実効性はある。
# --------------------------------------------------------------------------
suspicious=$(grep -rn --include='*.rs' \
    -e '6378137' \
    -e '298\.257' \
    -e '0\.00669437' \
    crates/ 2>/dev/null \
    | grep -v '^crates/flightsim-core/' || true)

if [ -n "$suspicious" ]; then
    fail "WGS84 ellipsoid constants found outside flightsim-core" \
         "ADR-0002: all geodetic conversions live in flightsim-core. Found:"
    echo "$suspicious" | sed 's/^/        /'
fi

# --------------------------------------------------------------------------

if [ "$failures" -eq 0 ]; then
    echo "architecture checks passed"
    exit 0
fi

echo
echo "$failures architecture violation(s). See ARCHITECTURE.md and docs/adr/."
exit 1
