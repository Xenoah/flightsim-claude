//! Pure one-shot loading/admitted-frame handshake; no render or GPU counters.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) draw_views: bool,
    pub(super) opportunity_epoch: Option<u64>,
}

#[derive(Debug)]
pub(super) struct AdmissionState {
    epoch: u64,
    active: bool,
    opened: bool,
    eligible: bool,
    requested: bool,
}

impl Default for AdmissionState {
    fn default() -> Self {
        Self {
            epoch: 0,
            active: true,
            opened: false,
            eligible: false,
            requested: false,
        }
    }
}

impl AdmissionState {
    /// `eligible` is supplied by the unchanged delay/frame/ReadyScene gates.
    /// An epoch acknowledges one prior admitted Render opportunity, not GPU or
    /// shader completion. Epoch zero never acknowledges a scene.
    pub(super) fn observe(
        &mut self,
        scene_changed: bool,
        eligible: bool,
        rendered_epoch: u64,
    ) -> Result<bool, &'static str> {
        if !self.active || self.requested {
            return Ok(false);
        }
        if scene_changed || (eligible && self.epoch == 0) {
            let Some(next) = self.epoch.checked_add(1) else {
                self.finish();
                return Err("screenshot admission generation exhausted");
            };
            self.epoch = next;
        }
        self.eligible = eligible;
        self.opened |= eligible;
        Ok(eligible && self.epoch != 0 && rendered_epoch == self.epoch)
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            draw_views: !self.active || self.opened || self.requested,
            opportunity_epoch: (self.active && self.eligible && !self.requested)
                .then_some(self.epoch),
        }
    }

    pub(super) fn requested(&mut self) {
        self.requested = true;
        self.opened = true;
        self.eligible = false;
    }

    pub(super) fn finish(&mut self) {
        self.active = false;
        self.opened = true;
        self.eligible = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligibility_opens_views_but_never_invents_a_prior_opportunity() {
        let mut state = AdmissionState::default();
        assert!(!state.snapshot().draw_views);
        assert_eq!(state.observe(true, false, 0), Ok(false));
        assert_eq!(state.observe(false, true, 0), Ok(false));
        let frame = state.snapshot();
        assert!(frame.draw_views);
        assert_eq!(frame.opportunity_epoch, Some(1));
        assert_eq!(state.observe(false, true, 1), Ok(true));
        state.requested();
        assert!(state.snapshot().draw_views);
        assert_eq!(state.snapshot().opportunity_epoch, None);
        assert_eq!(state.observe(false, true, 1), Ok(false));
    }

    #[test]
    fn changed_or_unready_scene_cannot_use_an_old_acknowledgement() {
        let mut state = AdmissionState::default();
        assert_eq!(state.observe(true, true, 0), Ok(false));
        assert_eq!(state.observe(true, false, 1), Ok(false));
        assert!(
            state.snapshot().draw_views,
            "admission remains open once begun"
        );
        assert_eq!(state.snapshot().opportunity_epoch, None);
        assert_eq!(state.observe(true, true, 1), Ok(false));
        assert_eq!(state.snapshot().opportunity_epoch, Some(3));
        assert_eq!(state.observe(false, true, 2), Ok(false));
        assert_eq!(state.observe(false, true, 3), Ok(true));
    }

    #[test]
    fn snapshots_do_not_follow_concurrent_main_state_changes() {
        let mut state = AdmissionState::default();
        let loading = state.snapshot();
        assert_eq!(state.observe(true, true, 0), Ok(false));
        let warm = state.snapshot();
        assert_eq!(state.observe(true, false, 1), Ok(false));
        assert!(!loading.draw_views);
        assert_eq!(loading.opportunity_epoch, None);
        assert!(warm.draw_views);
        assert_eq!(warm.opportunity_epoch, Some(1));
        assert_eq!(state.snapshot().opportunity_epoch, None);
    }

    #[test]
    fn cancellation_or_failure_does_not_rearm_from_late_or_repeated_acks() {
        for rendered in [0, 1, u64::MAX] {
            let mut state = AdmissionState::default();
            assert_eq!(state.observe(true, true, 0), Ok(false));
            state.finish();
            state.finish();
            assert!(state.snapshot().draw_views);
            assert_eq!(state.snapshot().opportunity_epoch, None);
            assert_eq!(state.observe(true, true, rendered), Ok(false));
        }
    }

    #[test]
    fn zero_cannot_qualify_and_generation_overflow_fails_closed() {
        let mut state = AdmissionState::default();
        assert_eq!(state.observe(false, true, 0), Ok(false));
        assert_eq!(state.snapshot().opportunity_epoch, Some(1));
        state.epoch = u64::MAX;
        assert_eq!(
            state.observe(true, true, u64::MAX),
            Err("screenshot admission generation exhausted")
        );
        assert!(state.snapshot().draw_views);
        assert_eq!(state.snapshot().opportunity_epoch, None);
        assert_eq!(state.observe(false, true, u64::MAX), Ok(false));
    }
}
