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

# `cargo tree` の出力からパッケージ名の一覧を得る。
# --edges normal で dev-dependencies と build-dependencies を除外する
# （テスト専用の依存は設計上の問題ではない）。
deps_of() {
    cargo tree --package "$1" --edges normal --prefix none 2>/dev/null \
        | awk '{print $1}' \
        | sort -u
}

# --------------------------------------------------------------------------
# 規約 1: core / fdm / world は Bevy に依存しない（ADR-0001）
#
# これらが GUI なしにテストできることが技術選定の根拠そのもの。
# ここが崩れると QA エージェントが回帰網を維持できなくなる。
# --------------------------------------------------------------------------
for crate in flightsim-core flightsim-fdm flightsim-world; do
    if deps_of "$crate" | grep -qiE '^bevy'; then
        fail "$crate depends on bevy" \
             "ADR-0001: core/fdm/world must stay engine-independent so that \`cargo test\` runs headless."
    fi
done

# --------------------------------------------------------------------------
# 規約 2: 依存は一方向のみ
#
# core ← fdm / world ← render / input / ui ← app
# 逆流も横断も禁止（ARCHITECTURE.md §2）。
# --------------------------------------------------------------------------
if deps_of flightsim-core | grep -qE '^flightsim-(fdm|world|render|input|ui|app|net)$'; then
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
