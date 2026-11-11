//! A wild mon that has wandered on, and the ball thrown at it: flown in, the mon taken into it in a flash of
//! white, dropped, shaken as many times as the server said, and then kept shut or burst open. Only the timing
//! and the shapes are here; the server decides everything (desk/PROTOCOL.md, `POST /throw`).

use glam::Vec3;

/// How long each part takes, in seconds.
const FLY: f32 = 0.45;
const TAKE: f32 = 0.35;
const DROP: f32 = 0.3;
const SHAKE: f32 = 0.75;
const KEPT: f32 = 1.0;
const BURST: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Wandering about, to be thrown at.
    About,
    Flying(f32),
    Taking(f32),
    Dropping(f32),
    /// On the ground, the server not yet heard from.
    Waiting,
    Shaking { done: u32, time: f32 },
    Kept(f32),
    Bursting(f32),
    /// Going, of its own accord.
    Leaving,
}

/// What the server said of a throw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outcome {
    pub shakes: u32,
    pub caught: bool,
    /// Not caught, and still about to be thrown at again.
    pub stays: bool,
}

/// What a throw came to, once it has been played out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Done {
    Caught,
    BrokeOut,
    Fled,
}

/// How it is to be drawn: the mon's size as a share of its own and how white it is, and the ball, if it shows:
/// where, how far rocked over in radians, and how big.
pub struct Look {
    pub scale: f32,
    pub flash: f32,
    pub ball: Option<(Vec3, f32, f32)>,
}

pub struct Wild {
    pub encounter: String,
    pub phase: Phase,
    outcome: Option<Outcome>,
}

impl Wild {
    pub fn new(encounter: String) -> Wild {
        Wild { encounter, phase: Phase::About, outcome: None }
    }

    /// Whether a ball can be thrown at it now.
    pub fn about(&self) -> bool {
        self.phase == Phase::About
    }

    /// Whether it is caught up in a throw, and so stands where it is.
    pub fn busy(&self) -> bool {
        !matches!(self.phase, Phase::About | Phase::Leaving)
    }

    pub fn throw(&mut self) {
        if self.about() {
            self.phase = Phase::Flying(0.0);
            self.outcome = None;
        }
    }

    /// A ball flung by hand has struck it: there is no flight to draw, the mon goes straight into it.
    pub fn struck(&mut self) {
        if self.about() {
            self.phase = Phase::Taking(0.0);
            self.outcome = None;
        }
    }

    /// The server's answer, whenever it comes.
    pub fn answer(&mut self, outcome: Outcome) {
        self.outcome = Some(outcome);
    }

    /// Whether a catch has been played out to its end: the ball has lain shut for as long as it does.
    pub fn step_done(&self) -> bool {
        self.phase == Phase::Kept(KEPT)
    }

    pub fn leave(&mut self) {
        if !self.busy() {
            self.phase = Phase::Leaving;
        }
    }

    pub fn step(&mut self, dt: f32) -> Option<Done> {
        let (next, done) = match self.phase {
            Phase::Flying(t) if t + dt >= FLY => (Phase::Taking(0.0), None),
            Phase::Flying(t) => (Phase::Flying(t + dt), None),
            Phase::Taking(t) if t + dt >= TAKE => (Phase::Dropping(0.0), None),
            Phase::Taking(t) => (Phase::Taking(t + dt), None),
            Phase::Dropping(t) if t + dt >= DROP => (Phase::Waiting, None),
            Phase::Dropping(t) => (Phase::Dropping(t + dt), None),
            Phase::Waiting => match self.outcome {
                Some(outcome) if outcome.shakes == 0 => (Phase::Bursting(0.0), None),
                Some(_) => (Phase::Shaking { done: 0, time: 0.0 }, None),
                None => (Phase::Waiting, None),
            },
            Phase::Shaking { done, time } if time + dt >= SHAKE => {
                let outcome = self.outcome.expect("shaking only once the server has answered");
                if done + 1 < outcome.shakes {
                    (Phase::Shaking { done: done + 1, time: 0.0 }, None)
                } else if outcome.caught {
                    (Phase::Kept(0.0), None)
                } else {
                    (Phase::Bursting(0.0), None)
                }
            }
            Phase::Shaking { done, time } => (Phase::Shaking { done, time: time + dt }, None),
            Phase::Kept(t) if t + dt >= KEPT => (Phase::Kept(KEPT), Some(Done::Caught)),
            Phase::Kept(t) => (Phase::Kept(t + dt), None),
            Phase::Bursting(t) if t + dt >= BURST => match self.outcome {
                Some(Outcome { stays: false, .. }) => (Phase::Leaving, Some(Done::Fled)),
                _ => (Phase::About, Some(Done::BrokeOut)),
            },
            Phase::Bursting(t) => (Phase::Bursting(t + dt), None),
            still => (still, None),
        };
        self.phase = next;
        done
    }

    /// How it looks now, for a mon `tall` units tall in a window showing `half` units either way from its middle;
    /// `clock` is any steady time in seconds.
    pub fn look(&self, half: f32, tall: f32, clock: f32) -> Look {
        let radius = half * 0.11;
        // In front of the mon, as the camera sees it.
        let front = half * 0.9;
        let heart = Vec3::new(0.0, tall * 0.5, front);
        let ground = Vec3::new(0.0, radius, front);
        match self.phase {
            // A small ball bobbing over its head says it is wild and can be thrown at.
            Phase::About => {
                let small = radius * 0.6;
                let over = (tall * 1.06 + small).min(half * 1.8 - small);
                Look { scale: 1.0, flash: 0.0, ball: Some((Vec3::new(0.0, over + (clock * 3.0).sin() * small * 0.35, 0.0), 0.0, small)) }
            }
            Phase::Flying(t) => {
                let along = t / FLY;
                let from = Vec3::new(-half * 0.95, radius, front);
                let at = from.lerp(heart, along) + Vec3::Y * (along * std::f32::consts::PI).sin() * half * 0.45;
                Look { scale: 1.0, flash: 0.0, ball: Some((at, along * 9.0, radius)) }
            }
            Phase::Taking(t) => {
                let along = t / TAKE;
                Look { scale: 1.0 - along, flash: (along * 3.0).min(1.0), ball: Some((heart, 0.0, radius)) }
            }
            Phase::Dropping(t) => {
                let along = (t / DROP).powi(2);
                Look { scale: 0.0, flash: 1.0, ball: Some((heart.lerp(ground, along), 0.0, radius)) }
            }
            Phase::Waiting | Phase::Kept(_) => Look { scale: 0.0, flash: 1.0, ball: Some((ground, 0.0, radius)) },
            Phase::Shaking { time, .. } => {
                // A rock one way and the other, then a pause before the next.
                let along = (time / (SHAKE * 0.6)).min(1.0);
                Look { scale: 0.0, flash: 1.0, ball: Some((ground, (along * std::f32::consts::TAU).sin() * 0.45, radius)) }
            }
            Phase::Bursting(t) => {
                let along = t / BURST;
                Look { scale: along, flash: 1.0 - along, ball: None }
            }
            Phase::Leaving => Look { scale: 1.0, flash: 0.0, ball: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays a throw out and says what it came to, how many times the ball rocked, and how long it took.
    fn play(outcome: Outcome, answered_after: f32) -> (Done, u32, f32) {
        let mut wild = Wild::new("e".into());
        wild.throw();
        let (mut time, mut rocks, mut was_shaking) = (0.0, 0, None);
        loop {
            if time >= answered_after && wild.outcome.is_none() {
                wild.answer(outcome);
            }
            let done = wild.step(1.0 / 30.0);
            time += 1.0 / 30.0;
            if let Phase::Shaking { done, .. } = wild.phase {
                if was_shaking != Some(done) {
                    rocks += 1;
                }
                was_shaking = Some(done);
            }
            if let Some(done) = done {
                return (done, rocks, time);
            }
            assert!(time < 30.0, "the throw never ended: {:?}", wild.phase);
        }
    }

    #[test]
    fn a_catch_rocks_three_times_and_is_kept() {
        let (done, rocks, _) = play(Outcome { shakes: 3, caught: true, stays: false }, 0.0);
        assert_eq!((done, rocks), (Done::Caught, 3));
    }

    #[test]
    fn a_miss_rocks_as_often_as_the_server_said_and_bursts() {
        assert_eq!(play(Outcome { shakes: 2, caught: false, stays: true }, 0.0).0, Done::BrokeOut);
        assert_eq!(play(Outcome { shakes: 2, caught: false, stays: true }, 0.0).1, 2);
        assert_eq!(play(Outcome { shakes: 0, caught: false, stays: true }, 0.0).1, 0);
        assert_eq!(play(Outcome { shakes: 1, caught: false, stays: false }, 0.0).0, Done::Fled);
    }

    #[test]
    fn the_ball_waits_on_the_ground_for_a_slow_server() {
        let quick = play(Outcome { shakes: 1, caught: false, stays: true }, 0.0).2;
        let slow = play(Outcome { shakes: 1, caught: false, stays: true }, 5.0).2;
        assert!(slow > 5.0 && slow - 5.0 < quick, "{quick} {slow}");
    }

    #[test]
    fn after_breaking_out_it_can_be_thrown_at_again_and_not_while_a_ball_is_out() {
        let mut wild = Wild::new("e".into());
        wild.throw();
        assert!(!wild.about());
        wild.throw();
        assert_eq!(wild.phase, Phase::Flying(0.0));
        wild.answer(Outcome { shakes: 0, caught: false, stays: true });
        while wild.step(1.0 / 30.0).is_none() {}
        assert!(wild.about());
    }

    #[test]
    fn the_mon_is_gone_into_the_ball_while_it_is_shut_and_whole_again_after() {
        let mut wild = Wild::new("e".into());
        assert_eq!(wild.look(1.0, 1.0, 0.0).scale, 1.0);
        wild.throw();
        wild.answer(Outcome { shakes: 1, caught: false, stays: true });
        let mut least = 1.0f32;
        while wild.step(1.0 / 30.0).is_none() {
            let look = wild.look(1.0, 1.0, 0.0);
            least = least.min(look.scale);
            if matches!(wild.phase, Phase::Shaking { .. }) {
                assert!(look.scale == 0.0 && look.ball.is_some());
            }
        }
        assert_eq!(least, 0.0);
        assert_eq!(wild.look(1.0, 1.0, 0.0).scale, 1.0);
    }
}
