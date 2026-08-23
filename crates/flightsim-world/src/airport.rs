//! 飛行場の幾何。滑走路の位置・向き・矩形上の判定。
//!
//! # 何のためにあるのか
//!
//! M2 の完了条件は「1 空港周辺で離陸 → 旋回 → 着陸」。そのためには
//! **「滑走路がどこにあるか」を 1 箇所に定義**しておく必要がある。
//! 緯度経度を app と sim と tilegen にそれぞれ書くと、片方だけ直された瞬間に
//! 「機体が滑走路の脇から離陸する」「着陸判定が別の場所を見ている」が起きる。
//!
//! ここは**幾何だけ**を持つ。滑走路を平らにするのは `flightsim-tilegen` の
//! オフライン処理（`examples/synthetic_dem.rs`）、機体を置くのは `flightsim-sim` の仕事。
//!
//! # 座標変換を自前で書かない
//!
//! 方位から前方ベクトルを作る箇所も含め、三角関数は
//! [`flightsim_core::Attitude`] / [`flightsim_core::LocalFrame`] にのみ触らせている。
//! 測地変換を各所で書くと、丸めと特異点（極・日付変更線）の扱いが分岐する（ADR-0002）。
//!
//! # データの出所
//!
//! [`Runway::synthetic`] は**実在しない合成フィクスチャ**であり、OpenStreetMap を
//! 含むいかなる外部データにも由来しない。したがって `ATTRIBUTION.md` の対象外。
//! 実空港（OSM の `aeroway=runway`）を取り込む際は帰属表示の追加が**必須**。

use flightsim_core::frames::Ned;
use flightsim_core::{Attitude, Degrees, Geodetic, LocalFrame, Meters, Radians};
use glam::DVec3;

/// 滑走路の末端からの相対位置。[`Runway::offsets`] が返す。
///
/// 着陸の評価に使う。「接地点が末端から何 m か」「中心線から何 m ずれたか」は
/// この 3 成分で決まる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunwayOffsets {
    /// 進入方位に沿った前方距離。末端が 0、反対端が [`Runway::length`]。
    pub longitudinal: Meters,
    /// 中心線からの横ずれ。**右が正**（[`flightsim_core::Attitude`] の機体 Y 軸と同じ向き）。
    pub lateral: Meters,
    /// 滑走路面からの高さ。上が正。
    ///
    /// 楕円体高の差そのものであり、接平面への投影ではない。滑走路面を
    /// 「一定の楕円体高の面」とみなしているため、地球の曲率による誤差が入らない
    /// （焼いた DEM も一定標高で平らにしてあるので、これが整合する定義）。
    pub vertical: Meters,
}

impl RunwayOffsets {
    /// 3 成分すべてが有限か。NaN は全状態に伝播するため、境界で検査する。
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.longitudinal.is_finite() && self.lateral.is_finite() && self.vertical.is_finite()
    }
}

/// 滑走路 1 本。
///
/// # 形状の定義
///
/// ```text
///        lateral +width/2
///              |
///   threshold -+------------------------> opposite_threshold
///        (0)   |        heading 方向             (length)
///              |
///        lateral -width/2
/// ```
///
/// 滑走路面は**一定の楕円体高 [`Runway::elevation`] の面**とする。接平面ではない。
/// 2 500 m の滑走路を接平面で表すと反対端が 0.49 m 沈み（`d²/2R`）、
/// 一定標高で焼いた DEM と食い違うため。
///
/// # `threshold.altitude` と [`Runway::elevation`] の関係
///
/// [`Runway::new`] は両者を一致させる。**幾何計算は常に [`Runway::elevation`] を正とし、
/// `threshold.altitude` は参照しない。** フィールドを直接書き換えて食い違わせた場合でも、
/// 高度の答えが 2 通り出ることはない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Runway {
    /// 進入端（着陸で最初に踏む側）の測地座標。
    pub threshold: Geodetic,
    /// 真方位。北が 0、東が π/2（`Attitude::yaw` と同じ規約）。
    pub heading: Radians,
    /// 全長。
    pub length: Meters,
    /// 全幅。中心線から左右に `width/2` ずつ。
    pub width: Meters,
    /// 滑走路面の楕円体高。
    pub elevation: Meters,
}

impl Runway {
    /// 末端の測地座標・方位・寸法から作る。
    ///
    /// `threshold` の高度は `elevation` で上書きされる（食い違いを構造的に防ぐ）。
    #[must_use]
    pub const fn new(
        threshold: Geodetic,
        heading: Radians,
        length: Meters,
        width: Meters,
        elevation: Meters,
    ) -> Self {
        Self {
            threshold: Geodetic::new(threshold.latitude, threshold.longitude, elevation),
            heading,
            length,
            width,
            elevation,
        }
    }

    /// 度・メートルで指定する。外部データや設定ファイルからの読み込み用。
    #[must_use]
    pub fn from_degrees(
        latitude_deg: f64,
        longitude_deg: f64,
        heading_deg: f64,
        length_m: f64,
        width_m: f64,
        elevation_m: f64,
    ) -> Self {
        Self::new(
            Geodetic::from_degrees(latitude_deg, longitude_deg, elevation_m),
            Degrees(heading_deg).to_radians(),
            Meters(length_m),
            Meters(width_m),
            Meters(elevation_m),
        )
    }

    /// 合成飛行場の滑走路。**実在しない。**
    ///
    /// `crates/flightsim-tilegen/examples/synthetic_dem.rs` が焼く合成地形と
    /// **対になったフィクスチャ**である。あの例はこの関数の返り値を読んで、
    /// 滑走路の矩形とその周囲を標高 8 m の平地に彫る。
    /// **数値を変えたら合成 DEM を焼き直すこと**（`data/tiles` の再生成が要る）。
    ///
    /// | 項目 | 値 |
    /// |---|---|
    /// | 末端 | 35.548°N, 139.775°E |
    /// | 方位 | 050° |
    /// | 全長 | 2 500 m |
    /// | 全幅 | 45 m |
    /// | 標高 | 8 m |
    ///
    /// 合成地形は東が海岸低地・西が山地なので、海岸寄りの平坦部に置いてある。
    #[must_use]
    pub fn synthetic() -> Self {
        Self::from_degrees(35.548, 139.775, 50.0, 2_500.0, 45.0, 8.0)
    }

    /// 反対側の末端。
    #[must_use]
    pub fn opposite_threshold(&self) -> Geodetic {
        self.point_at(self.length, Meters::ZERO)
    }

    /// 滑走路の中心。
    #[must_use]
    pub fn center(&self) -> Geodetic {
        self.point_at(self.length * 0.5, Meters::ZERO)
    }

    /// 反対側から進入する場合の方位。`[0, 2π)`。
    #[must_use]
    pub fn reciprocal_heading(&self) -> Radians {
        Radians(self.heading.get() + core::f64::consts::PI).wrap_positive()
    }

    /// 離陸開始位置。末端から 150 m 進んだ中心線上。
    ///
    /// **アプリの既定開始地点はこれを使うこと。** 緯度経度を直接書くと、
    /// 滑走路を動かしたときに機体だけ取り残される。
    /// 150 m は末端をわずかに空けるための余裕であって、物理的な根拠はない。
    #[must_use]
    pub fn takeoff_start(&self) -> Geodetic {
        self.point_at(Meters(150.0), Meters::ZERO)
    }

    /// 末端からの前方距離・横ずれで指定した点の測地座標。高度は [`Runway::elevation`]。
    ///
    /// 範囲を制限しない。滑走路の外（進入経路や周囲の平地）も指定できる。
    #[must_use]
    pub fn point_at(&self, longitudinal: Meters, lateral: Meters) -> Geodetic {
        let (forward, right) = self.axes_ned();
        let offset = forward * longitudinal.get() + right * lateral.get();
        let surface = self
            .local_frame()
            .ned_to_ecef_position(Ned(offset))
            .to_geodetic();

        // 接平面上で動かした点は、遠いほど楕円体面から浮く（2 500 m で 0.49 m）。
        // 滑走路面は一定標高の面なので、高度は貼り直す。
        Geodetic::new(surface.latitude, surface.longitude, self.elevation)
    }

    /// 末端を原点とした相対位置。
    ///
    /// 前方距離・横ずれは**高度に依存しない**。対象点をいったん滑走路面（一定標高）へ
    /// 落としてから接平面へ投影しているため、上空 12 000 m を通過中でも真下と同じ値になる。
    /// これをやらないと、高度に比例して前方距離が伸び（12 000 m で約 2.4 m）、
    /// 高い所を飛ぶほど滑走路が長く見えるという妙な振る舞いになる。
    ///
    /// # 接平面近似
    ///
    /// 一定標高の面と接平面のずれから、往復（[`Runway::point_at`] → `offsets`）には
    /// `d³/(2R²)` の残差が出る。2 500 m で 0.2 mm、9 km で 9 mm。滑走路の寸法では無視できる。
    #[must_use]
    pub fn offsets(&self, position: Geodetic) -> RunwayOffsets {
        let on_surface = Geodetic::new(position.latitude, position.longitude, self.elevation);
        let ned = self
            .local_frame()
            .ecef_to_ned_position(on_surface.to_ecef());
        let (forward, right) = self.axes_ned();

        RunwayOffsets {
            longitudinal: Meters(ned.0.dot(forward)),
            lateral: Meters(ned.0.dot(right)),
            vertical: position.altitude - self.elevation,
        }
    }

    /// 末端からの前方距離。手前が負、反対端が [`Runway::length`]。
    #[must_use]
    pub fn longitudinal_offset(&self, position: Geodetic) -> Meters {
        self.offsets(position).longitudinal
    }

    /// 中心線からの横ずれ。右が正。
    #[must_use]
    pub fn lateral_offset(&self, position: Geodetic) -> Meters {
        self.offsets(position).lateral
    }

    /// 滑走路の矩形の上か。**高度は見ない**（上空を通過中でも真）。
    ///
    /// 境界は閉区間。末端ちょうど・幅の縁ちょうどは滑走路上と判定する。
    /// ただし比較は浮動小数の厳密比較であり、**境界から 1 nm 程度の点がどちらに転ぶかは
    /// 保証しない**（[`Runway::point_at`] の往復に `d³/(2R²)` の残差があるため、
    /// 縁ちょうどを狙って作った点は符号が振れる）。実用上の距離では問題にならない。
    ///
    /// 地球の裏側は偽を返す。接平面への投影は対蹠点を原点付近へ折り返してしまうため、
    /// 楕円体法線の向きで半球を検査している（[`Runway::is_same_hemisphere`]）。
    /// NaN を含む座標も偽（比較が全て偽になるため、明示的な分岐は要らない）。
    #[must_use]
    pub fn contains(&self, position: Geodetic) -> bool {
        if !self.is_same_hemisphere(position) {
            return false;
        }
        let offsets = self.offsets(position);
        (0.0..=self.length.get()).contains(&offsets.longitudinal.get())
            && offsets.lateral.get().abs() <= self.width.get() * 0.5
    }

    /// 滑走路面に固定したローカル NED 系。原点は末端、高度は [`Runway::elevation`]。
    #[must_use]
    pub fn local_frame(&self) -> LocalFrame {
        LocalFrame::new(Geodetic::new(
            self.threshold.latitude,
            self.threshold.longitude,
            self.elevation,
        ))
    }

    /// 対象点が滑走路と同じ半球にあるか（楕円体法線の内積が正）。
    ///
    /// 接平面への投影は地球の裏側を原点付近へ折り返す。対蹠点の前方距離・横ずれは
    /// どちらもほぼ 0 になり、**判定だけ見れば滑走路の中心に見える**。
    /// 高度で弾く方法もあるが、それでは「上空を通過中」と区別が付かない。
    #[must_use]
    pub fn is_same_hemisphere(&self, position: Geodetic) -> bool {
        let here = self.local_frame().up_ecef();
        let there = LocalFrame::new(position).up_ecef();
        here.dot(there) > 0.0
    }

    /// 数値がすべて有限か。
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.threshold.latitude.is_finite()
            && self.threshold.longitude.is_finite()
            && self.threshold.altitude.is_finite()
            && self.heading.is_finite()
            && self.length.is_finite()
            && self.width.is_finite()
            && self.elevation.is_finite()
    }

    /// 滑走路の前方・右方向を、末端のローカル NED で表した単位ベクトル。
    ///
    /// 方位から向きを作るのに `sin`/`cos` を直接書かず、
    /// [`flightsim_core::Attitude`]（機体 X = 前、Y = 右）を経由する。
    /// 方位の符号規約が 2 箇所に散らないようにするため。
    fn axes_ned(&self) -> (DVec3, DVec3) {
        let rotation = Attitude::new(Radians::ZERO, Radians::ZERO, self.heading).to_quaternion();
        (rotation * DVec3::X, rotation * DVec3::Y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_close {
        ($actual:expr, $expected:expr, $tol:expr) => {{
            let (a, e, t) = ($actual, $expected, $tol);
            assert!(
                (a - e).abs() <= t,
                "expected {a} ≈ {e} (tolerance {t}), difference was {}",
                (a - e).abs()
            );
        }};
    }

    /// 合成 DEM 側と共有しているフィクスチャの値。**片方だけ変えると地形と食い違う。**
    #[test]
    fn synthetic_fixture_matches_the_documented_numbers() {
        let runway = Runway::synthetic();
        assert_close!(runway.threshold.latitude_degrees(), 35.548, 1e-12);
        assert_close!(runway.threshold.longitude_degrees(), 139.775, 1e-12);
        assert_close!(runway.heading.to_degrees().get(), 50.0, 1e-12);
        assert_close!(runway.length.get(), 2_500.0, 1e-12);
        assert_close!(runway.width.get(), 45.0, 1e-12);
        assert_close!(runway.elevation.get(), 8.0, 1e-12);
        // 末端の高度は elevation と一致していること（2 通りの答えを持たない）。
        assert_close!(runway.threshold.altitude.get(), 8.0, 1e-12);
        assert!(runway.is_finite());
    }

    // --- 幾何の外形 ---

    #[test]
    fn opposite_threshold_is_one_length_along_the_heading() {
        let runway = Runway::synthetic();
        let far = runway.opposite_threshold();

        // ECEF 上の直線距離で照合する。大圏距離（球近似）は 0.5% ずれ得るので使わない。
        assert_close!(
            runway.threshold.to_ecef().distance_to(far.to_ecef()).get(),
            2_500.0,
            0.05
        );
        // 反対端は滑走路の座標系で (length, 0)。
        let offsets = runway.offsets(far);
        assert_close!(offsets.longitudinal.get(), 2_500.0, 1e-3);
        assert_close!(offsets.lateral.get(), 0.0, 1e-6);
        assert_close!(offsets.vertical.get(), 0.0, 1e-9);
        // 高度は一定（接平面ではなく一定標高の面）。
        assert_close!(far.altitude.get(), 8.0, 1e-12);
    }

    #[test]
    fn center_is_halfway_between_the_thresholds() {
        let runway = Runway::synthetic();
        let center = runway.center();
        let to_near = runway
            .threshold
            .to_ecef()
            .distance_to(center.to_ecef())
            .get();
        let to_far = runway
            .opposite_threshold()
            .to_ecef()
            .distance_to(center.to_ecef())
            .get();

        assert_close!(to_near, 1_250.0, 0.05);
        assert_close!(to_far, 1_250.0, 0.05);
        assert_close!(runway.longitudinal_offset(center).get(), 1_250.0, 1e-3);
        assert_close!(runway.lateral_offset(center).get(), 0.0, 1e-6);
    }

    #[test]
    fn reciprocal_heading_is_the_opposite_direction() {
        let runway = Runway::synthetic();
        assert_close!(runway.reciprocal_heading().to_degrees().get(), 230.0, 1e-9);

        // 方位 350° の反方位は 170°。単純な加算だと 530° になる。
        let north = Runway::from_degrees(0.0, 0.0, 350.0, 2_000.0, 45.0, 0.0);
        assert_close!(north.reciprocal_heading().to_degrees().get(), 170.0, 1e-9);
        // 常に [0, 2π)。
        for heading in [0.0, 1.0, 179.0, 180.0, 181.0, 359.9] {
            let r = Runway::from_degrees(10.0, 20.0, heading, 2_000.0, 45.0, 0.0);
            let v = r.reciprocal_heading().get();
            assert!(
                (0.0..core::f64::consts::TAU).contains(&v),
                "{v} out of range"
            );
        }
    }

    // --- 方位の規約。外部（コンパス）の規約と突き合わせる ---

    #[test]
    fn heading_zero_runs_due_north() {
        let runway = Runway::from_degrees(35.0, 139.0, 0.0, 2_000.0, 45.0, 0.0);
        let far = runway.opposite_threshold();
        assert!(far.latitude_degrees() > runway.threshold.latitude_degrees());
        assert_close!(far.longitude_degrees(), 139.0, 1e-9);
    }

    #[test]
    fn heading_ninety_runs_due_east() {
        let runway = Runway::from_degrees(35.0, 139.0, 90.0, 2_000.0, 45.0, 0.0);
        let far = runway.opposite_threshold();
        assert!(far.longitude_degrees() > 139.0);

        // **真東へ「まっすぐ」進むと緯度はわずかに下がる。** 方位 090° の大円は
        // その地点が最北で、そこから赤道側へ寄っていくため。
        // 量は `(d²/2R²)·tan φ` ≒ 1.98e-6 度（0.22 m）。等緯度線を進む航程線ではない。
        let drop = 35.0 - far.latitude_degrees();
        assert!(drop > 0.0, "great circle should bend toward the equator");
        assert_close!(drop, 1.98e-6, 1e-7);
    }

    #[test]
    fn lateral_offset_is_positive_to_the_right() {
        // 北向き滑走路の右手は東。
        let runway = Runway::from_degrees(35.0, 139.0, 0.0, 2_000.0, 45.0, 0.0);
        let east_of_centreline = Geodetic::from_degrees(35.0, 139.001, 0.0);
        assert!(runway.lateral_offset(east_of_centreline).get() > 0.0);

        let west_of_centreline = Geodetic::from_degrees(35.0, 138.999, 0.0);
        assert!(runway.lateral_offset(west_of_centreline).get() < 0.0);
    }

    // --- 往復 ---

    #[test]
    fn point_at_and_offsets_are_inverses() {
        let runway = Runway::synthetic();
        for longitudinal in [-800.0, 0.0, 1.0, 1_250.0, 2_500.0, 9_000.0] {
            for lateral in [-3_000.0, -22.5, 0.0, 22.5, 3_000.0] {
                let point = runway.point_at(Meters(longitudinal), Meters(lateral));
                let offsets = runway.offsets(point);
                // 許容 0.02 m は接平面と一定標高面のずれ `d³/(2R²)`。
                // 最遠の (9 000, 3 000) で 0.011 m になる。滑走路上（2 500 m 以内）では
                // 0.2 mm 以下で、ここまで大きくはならない。
                assert_close!(offsets.longitudinal.get(), longitudinal, 0.02);
                assert_close!(offsets.lateral.get(), lateral, 0.02);
                assert_close!(offsets.vertical.get(), 0.0, 1e-9);
            }
        }

        // 滑走路の内側では往復誤差がミリメートル未満であること。
        for longitudinal in [0.0, 625.0, 1_250.0, 2_500.0] {
            let point = runway.point_at(Meters(longitudinal), Meters(20.0));
            assert_close!(runway.offsets(point).longitudinal.get(), longitudinal, 1e-3);
            assert_close!(runway.offsets(point).lateral.get(), 20.0, 1e-3);
        }
    }

    #[test]
    fn vertical_offset_is_the_height_above_the_surface() {
        let runway = Runway::synthetic();
        let center = runway.center();
        let overhead = Geodetic::new(center.latitude, center.longitude, Meters(308.0));
        assert_close!(runway.offsets(overhead).vertical.get(), 300.0, 1e-9);

        let below = Geodetic::new(center.latitude, center.longitude, Meters(-2.0));
        assert_close!(runway.offsets(below).vertical.get(), -10.0, 1e-9);
    }

    // --- contains ---

    #[test]
    fn contains_the_centre_and_the_takeoff_start() {
        let runway = Runway::synthetic();
        assert!(runway.contains(runway.center()));
        assert!(runway.contains(runway.takeoff_start()));
        assert_close!(
            runway.longitudinal_offset(runway.takeoff_start()).get(),
            150.0,
            1e-3
        );
        assert_close!(
            runway.lateral_offset(runway.takeoff_start()).get(),
            0.0,
            1e-6
        );
    }

    #[test]
    fn contains_rejects_points_outside_the_width() {
        let runway = Runway::synthetic();
        // 幅 45 m なので縁は ±22.5 m。
        assert!(runway.contains(runway.point_at(Meters(1_250.0), Meters(22.4))));
        assert!(runway.contains(runway.point_at(Meters(1_250.0), Meters(-22.4))));
        assert!(!runway.contains(runway.point_at(Meters(1_250.0), Meters(22.6))));
        assert!(!runway.contains(runway.point_at(Meters(1_250.0), Meters(-22.6))));
        assert!(!runway.contains(runway.point_at(Meters(1_250.0), Meters(500.0))));
    }

    #[test]
    fn contains_rejects_points_beyond_the_ends() {
        let runway = Runway::synthetic();
        assert!(!runway.contains(runway.point_at(Meters(-0.5), Meters::ZERO)));
        assert!(!runway.contains(runway.point_at(Meters(2_500.5), Meters::ZERO)));
        assert!(!runway.contains(runway.point_at(Meters(-50_000.0), Meters::ZERO)));
    }

    #[test]
    fn contains_includes_the_threshold_boundaries() {
        let runway = Runway::synthetic();
        // 末端ちょうど・反対端ちょうどは滑走路上。
        assert!(runway.contains(runway.threshold));
        assert!(runway.contains(runway.opposite_threshold()));
        // 0.1 m 手前・0.1 m 先は外。境界の 1 m 以内で判定が反転することを示す。
        assert!(!runway.contains(runway.point_at(Meters(-0.1), Meters::ZERO)));
        assert!(!runway.contains(runway.point_at(Meters(2_500.1), Meters::ZERO)));
        assert!(runway.contains(runway.point_at(Meters(0.1), Meters::ZERO)));
        assert!(runway.contains(runway.point_at(Meters(2_499.9), Meters::ZERO)));
    }

    #[test]
    fn contains_ignores_altitude() {
        let runway = Runway::synthetic();
        let center = runway.center();
        for altitude in [-100.0, 0.0, 8.0, 1_000.0, 12_000.0] {
            let above = Geodetic::new(center.latitude, center.longitude, Meters(altitude));
            assert!(
                runway.contains(above),
                "altitude {altitude} m should not change containment"
            );
        }
    }

    // --- 特異点と縮退 ---

    #[test]
    fn the_antipode_is_not_on_the_runway() {
        // **赤道上が最悪ケース。** 楕円体法線が地心方向と一致するため、対蹠点は
        // 接平面へ投影すると前後・左右ともちょうど 0、つまり滑走路の中心に折り返される。
        // 緯度が付くと法線が傾くのでずれるが（合成滑走路の 35.5°N では約 42 km）、
        // それに頼ると赤道の滑走路だけが壊れる。
        let runway = Runway::from_degrees(0.0, 139.775, 50.0, 2_500.0, 45.0, 8.0);
        let antipode = Geodetic::from_degrees(0.0, 139.775 - 180.0, 8.0);

        let offsets = runway.offsets(antipode);
        assert!(offsets.is_finite());
        // 投影だけでは滑走路の中心に見えてしまうことを、まず記録しておく。
        assert_close!(offsets.longitudinal.get(), 0.0, 1e-6);
        assert_close!(offsets.lateral.get(), 0.0, 1e-6);
        // それでも滑走路上ではない。半球の検査がこれを弾く。
        assert!(!runway.is_same_hemisphere(antipode));
        assert!(!runway.contains(antipode));

        // 合成滑走路（35.5°N）でも当然に偽。
        let synthetic = Runway::synthetic();
        let far_side = Geodetic::from_degrees(-35.548, 139.775 - 180.0, 8.0);
        assert!(synthetic.offsets(far_side).is_finite());
        assert!(!synthetic.contains(far_side));
    }

    #[test]
    fn distant_points_stay_finite() {
        let runway = Runway::synthetic();
        for (lat, lon, alt) in [
            (90.0, 0.0, 0.0),
            (-90.0, 0.0, 0.0),
            (90.0, 180.0, 12_000.0),
            (0.0, 180.0, 0.0),
            (0.0, -180.0, 0.0),
            (-35.548, -40.225, 0.0),
            (35.548, 139.775, 400_000.0),
            (0.0, 0.0, -10_000.0),
        ] {
            let position = Geodetic::from_degrees(lat, lon, alt);
            let offsets = runway.offsets(position);
            assert!(
                offsets.is_finite(),
                "offsets at ({lat}, {lon}, {alt}) were not finite: {offsets:?}"
            );
            // panic しないことの確認も兼ねる。
            let _ = runway.contains(position);
        }
    }

    #[test]
    fn nan_input_is_rejected_without_panicking() {
        let runway = Runway::synthetic();
        let nan = Geodetic::new(Radians(f64::NAN), Radians(f64::NAN), Meters(f64::NAN));
        assert!(!runway.contains(nan));
        assert!(!runway.offsets(nan).is_finite());

        // 高度だけ NaN。水平位置は滑走路の中心。
        let center = runway.center();
        let bad_altitude = Geodetic::new(center.latitude, center.longitude, Meters(f64::NAN));
        // 高度を見ない判定なので真。ここで panic しないことが要点。
        assert!(runway.contains(bad_altitude));
    }

    // --- 日付変更線 ---

    #[test]
    fn a_runway_crossing_the_dateline_behaves_normally() {
        // 東経 179.99° から真東へ 2 500 m。反対端は西経側へ回り込む。
        let runway = Runway::from_degrees(0.0, 179.99, 90.0, 2_500.0, 45.0, 0.0);
        let far = runway.opposite_threshold();

        // 経度は [-180, 180] に収まっていること（180 を超えて 180.01 にならない）。
        assert!(
            (-180.0..=180.0).contains(&far.longitude_degrees()),
            "longitude {} escaped the valid range",
            far.longitude_degrees()
        );
        // 実際に日付変更線をまたいでいる。
        assert!(far.longitude_degrees() < 0.0, "{}", far.longitude_degrees());

        // 距離は素直に 2 500 m。経度の差（約 -359.98°）を距離に使うと桁違いになる。
        assert_close!(
            runway.threshold.to_ecef().distance_to(far.to_ecef()).get(),
            2_500.0,
            0.05
        );
        // 反対端は滑走路上と判定される。
        assert!(runway.contains(far));
        assert_close!(runway.longitudinal_offset(far).get(), 2_500.0, 1e-3);

        // またいだ先の点でも往復が成り立つ。
        for longitudinal in [0.0, 1_249.0, 2_500.0] {
            let point = runway.point_at(Meters(longitudinal), Meters(10.0));
            assert_close!(runway.longitudinal_offset(point).get(), longitudinal, 1e-3);
            assert_close!(runway.lateral_offset(point).get(), 10.0, 1e-3);
        }
    }

    #[test]
    fn a_runway_straddling_the_dateline_at_high_latitude_is_finite() {
        // 高緯度ほど経度 1 度が短く、またぎ方が急になる。
        let runway = Runway::from_degrees(70.0, -179.999, 45.0, 3_000.0, 60.0, 120.0);
        let far = runway.opposite_threshold();
        assert!(far.latitude.is_finite() && far.longitude.is_finite());
        assert!((-180.0..=180.0).contains(&far.longitude_degrees()));
        assert!(runway.contains(runway.center()));
    }

    // --- 極 ---

    #[test]
    fn a_runway_at_the_north_pole_is_finite() {
        // 末端が厳密に極。経度が定義できない特異点。
        let runway = Runway::from_degrees(90.0, 0.0, 0.0, 2_000.0, 45.0, 0.0);
        let far = runway.opposite_threshold();
        assert!(
            far.latitude.is_finite() && far.longitude.is_finite() && far.altitude.is_finite(),
            "pole produced a non-finite point: {far:?}"
        );
        // 極から真北へ 2 km 進むと、反対側の子午線を 2 km 下る。
        assert!(far.latitude_degrees() < 90.0);
        assert_close!(
            runway.threshold.to_ecef().distance_to(far.to_ecef()).get(),
            2_000.0,
            0.05
        );
        assert!(runway.contains(runway.center()));
        assert!(runway.contains(far));
    }

    #[test]
    fn a_runway_near_the_south_pole_round_trips() {
        let runway = Runway::from_degrees(-89.995, 42.0, 137.0, 2_400.0, 45.0, 2_800.0);
        // 末端ちょうど（前方距離 0）は避ける。往復の残差で符号が振れ、
        // 実測で -3.4e-10 m、すなわち閉区間の外側 0.34 nm に落ちることがある。
        for longitudinal in [1.0, 600.0, 2_399.0] {
            for lateral in [-20.0, 0.0, 20.0] {
                let point = runway.point_at(Meters(longitudinal), Meters(lateral));
                assert!(point.latitude.is_finite() && point.longitude.is_finite());
                let offsets = runway.offsets(point);
                assert_close!(offsets.longitudinal.get(), longitudinal, 1e-3);
                assert_close!(offsets.lateral.get(), lateral, 1e-3);
                assert!(runway.contains(point));
            }
        }
        // 極を越えた先も有限。
        let across = runway.point_at(Meters(2_400.0), Meters(0.0));
        assert!(across.latitude_degrees() >= -90.0);
    }

    #[test]
    fn a_runway_at_the_pole_rejects_the_far_side_of_the_earth() {
        let runway = Runway::from_degrees(90.0, 0.0, 0.0, 2_000.0, 45.0, 0.0);
        assert!(!runway.contains(Geodetic::from_degrees(-90.0, 0.0, 0.0)));
    }

    // --- 縮退した寸法 ---

    #[test]
    fn degenerate_dimensions_do_not_panic() {
        // 長さ 0・幅 0。末端だけが滑走路上になる。
        let point_runway = Runway::from_degrees(35.0, 139.0, 12.0, 0.0, 0.0, 5.0);
        assert!(point_runway.contains(point_runway.threshold));
        assert!(!point_runway.contains(point_runway.point_at(Meters(1.0), Meters::ZERO)));
        assert_close!(
            point_runway
                .threshold
                .to_ecef()
                .distance_to(point_runway.opposite_threshold().to_ecef())
                .get(),
            0.0,
            1e-9
        );

        // 負の長さ。範囲が空になるので何も含まない（panic しないことが要点）。
        let inverted = Runway::from_degrees(35.0, 139.0, 12.0, -100.0, 45.0, 5.0);
        assert!(!inverted.contains(inverted.threshold));
        assert!(!inverted.contains(inverted.center()));
    }
}
