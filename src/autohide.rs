//! Ocultación determinista: sondeo del cursor y transición antes de avanzar la animación.
use crate::spring::Spring;
use std::time::{Duration, Instant};
pub struct AutoHide {
    pub motion: Spring,
    pub expanded: bool,
    hide_at: Option<Instant>,
}
impl AutoHide {
    pub fn new(now: Instant) -> Self {
        Self {
            motion: Spring::new(1.0, 360.0, 38.0),
            expanded: true,
            hide_at: Some(now + Duration::from_millis(1500)),
        }
    }
    pub fn reveal(&mut self) {
        self.expanded = true;
        self.motion.set_target(1.0);
        self.hide_at = None;
    }
    pub fn defer_hide(&mut self, now: Instant, delay: Duration) {
        self.hide_at = Some(now + delay);
    }
    pub fn sample(&mut self, keep_visible: bool, now: Instant) {
        if keep_visible {
            self.reveal();
        } else if self.expanded && self.hide_at.is_none() {
            self.defer_hide(now, Duration::from_millis(300));
        }
    }
    pub fn waiting(&self) -> bool {
        self.hide_at.is_some()
    }
    pub fn active(&self) -> bool {
        self.waiting() || self.motion.value != self.motion.target || self.motion.velocity != 0.0
    }
    pub fn step(&mut self, now: Instant, dt: f32) -> bool {
        if self.hide_at.is_some_and(|deadline| now >= deadline) {
            self.hide_at = None;
            self.expanded = false;
            self.motion.set_target(0.0);
        }
        // La primera iteración de salida ya debe devolver movimiento activo.
        self.motion.step(dt)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leaving_edge_without_entering_bar_still_hides_and_animates() {
        let now = Instant::now();
        let mut state = AutoHide::new(now);
        state.sample(true, now);
        state.sample(false, now);
        assert!(state.step(now + Duration::from_millis(301), 0.016));
        assert!(!state.expanded);
        assert!(state.motion.value < 1.0);
        for _ in 0..120 {
            state.step(now + Duration::from_secs(3), 0.016);
        }
        assert_eq!(state.motion.value, 0.0);
        assert!(!state.waiting());
    }
    #[test]
    fn hovering_cancels_hide_and_reentering_reverses_animation() {
        let now = Instant::now();
        let mut state = AutoHide::new(now);
        state.sample(true, now);
        for i in 0..100 {
            state.sample(true, now + Duration::from_millis(i * 40));
        }
        assert!(!state.waiting());
        state.sample(false, now);
        state.step(now + Duration::from_millis(301), 0.05);
        state.sample(true, now + Duration::from_millis(320));
        for _ in 0..120 {
            state.step(now + Duration::from_secs(3), 0.016);
        }
        assert_eq!(state.motion.value, 1.0);
        assert!(state.expanded);
    }
}
