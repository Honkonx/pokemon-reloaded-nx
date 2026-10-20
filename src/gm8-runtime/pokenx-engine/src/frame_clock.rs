//! Simulation time owed by the 60 Hz presentation loop. No platform or GL calls.

pub const PRESENTATION_HZ: u32 = 60;
pub const MAX_STEPS_PER_PRESENT: usize = 8;
// A long blocking GML dialog or suspension must not create a burst of catch-up ticks.
const DISCONTINUITY_NS: u64 = 250_000_000;

#[derive(Default)]
pub struct FrameClock {
    debt_ns: u64,
    started: bool,
    initial_step: bool,
}

impl FrameClock {
    pub fn reset(&mut self) { *self = Self::default(); }

    /// Returns true when a long host interruption reset the simulation clock.
    pub fn advance(&mut self, elapsed_ns: u64) -> bool {
        let interrupted = elapsed_ns > DISCONTINUITY_NS;
        if !self.started || interrupted {
            self.debt_ns = 0;
            self.initial_step = true;
            self.started = true;
        } else {
            self.debt_ns = self.debt_ns.saturating_add(elapsed_ns);
        }
        interrupted
    }

    /// Consume one tick at the CURRENT room speed, which can change during a frame.
    /// The caller bounds catch-up work; unconsumed time remains owed.
    pub fn take_step(&mut self, room_speed: u32) -> bool {
        if self.initial_step {
            self.initial_step = false;
            return true
        }
        let period = (1_000_000_000 / u64::from(room_speed.max(1))).max(1);
        if self.debt_ns >= period {
            self.debt_ns -= period;
            true
        } else { false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consume(clock: &mut FrameClock, rate: u32) -> usize {
        let mut count = 0;
        while count < MAX_STEPS_PER_PRESENT && clock.take_step(rate) { count += 1; }
        count
    }

    #[test]
    fn sixty_presentations_preserve_thirty_simulation_ticks() {
        let mut clock = FrameClock::default();
        clock.advance(0);
        assert_eq!(consume(&mut clock, 30), 1); // initial frame
        let mut ticks = 0;
        for _ in 0..60 { clock.advance(16_666_667); ticks += consume(&mut clock, 30); }
        assert_eq!(ticks, 30);
    }

    #[test]
    fn fast_rooms_and_rate_changes_use_the_current_tick_duration() {
        let mut clock = FrameClock::default();
        clock.advance(0);
        assert_eq!(consume(&mut clock, 120), 1);
        clock.advance(16_666_667);
        assert_eq!(consume(&mut clock, 120), 2);
        clock.advance(16_666_667);
        assert_eq!(consume(&mut clock, 30), 0);
        clock.advance(16_666_667);
        assert_eq!(consume(&mut clock, 30), 1);
    }

    #[test]
    fn bounded_catchup_retains_debt_instead_of_silently_dropping_ticks() {
        let mut clock = FrameClock::default();
        clock.advance(0); consume(&mut clock, 60);
        clock.advance(200_000_000);
        assert_eq!(consume(&mut clock, 60), MAX_STEPS_PER_PRESENT);
        assert_eq!(consume(&mut clock, 60), 4);
    }

    #[test]
    fn suspension_and_long_modal_waits_do_not_fast_forward_the_game() {
        let mut clock = FrameClock::default();
        clock.advance(0); consume(&mut clock, 60);
        assert!(clock.advance(5_000_000_000));
        assert_eq!(consume(&mut clock, 60), 1);
        clock.reset();
        assert!(!clock.advance(0));
        assert_eq!(consume(&mut clock, 0), 1);
        assert!(!clock.take_step(0));
    }
}
