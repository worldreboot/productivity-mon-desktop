//! A ball to pick up and fling at a wild mon: it rests on the ground near the mon, is carried by the mouse,
//! and let go flies as it was thrown, falling as things fall. If it meets the mon it has struck it (and the
//! throw goes to the server as any does); if it lands, it lies a moment and is back where it rested, not spent.
//! Positions are the ball's middle on the screen, in pixels; nothing here asks Windows anything.

/// How fast a flung ball gains downward speed, in pixels a second each second, and the fastest it may be flung.
const GRAVITY: f32 = 1800.0;
const FASTEST: f32 = 3200.0;
/// How long it lies where it landed before it is back at rest, and how far back the hand's path is looked at
/// to tell how fast it was let go, in seconds.
const LIES: f32 = 0.7;
const SWING: f32 = 0.09;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Where it waits to be picked up.
    Rest,
    Held,
    Flying,
    /// On the ground after a miss, for this many seconds more.
    Landed(f32),
    /// It struck the mon: what happens to it now is drawn in the mon's own window.
    Away,
}

pub struct Toss {
    pub x: f32,
    pub y: f32,
    speed: (f32, f32),
    /// How far round it has turned, in radians.
    pub spin: f32,
    pub state: State,
    rest: (f32, f32),
    /// Where the hand has hold of it, from its middle; and where the hand has lately been, and when.
    grip: (f32, f32),
    path: Vec<(f32, f32, f32)>,
}

impl Toss {
    pub fn new(x: f32, y: f32) -> Toss {
        Toss { x, y, speed: (0.0, 0.0), spin: 0.0, state: State::Rest, rest: (x, y), grip: (0.0, 0.0), path: Vec::new() }
    }

    /// Puts it to rest at a new place.
    pub fn rest_at(&mut self, x: f32, y: f32) {
        (self.rest, self.x, self.y, self.speed, self.state) = ((x, y), x, y, (0.0, 0.0), State::Rest);
    }

    /// Whether it is to be seen in its own window.
    pub fn shown(&self) -> bool {
        self.state != State::Away
    }

    /// Whether it is somewhere a frame behind where it will be the next: in the hand or in the air.
    pub fn moving(&self) -> bool {
        matches!(self.state, State::Held | State::Flying)
    }

    /// The hand takes it, at `cursor`, if it is lying somewhere to be taken. Whether it did.
    pub fn pick_up(&mut self, cursor: (f32, f32)) -> bool {
        if !matches!(self.state, State::Rest | State::Landed(_)) {
            return false;
        }
        self.state = State::Held;
        self.grip = (cursor.0 - self.x, cursor.1 - self.y);
        self.path.clear();
        true
    }

    /// The hand, holding it, is at `cursor` at `time` seconds.
    pub fn carry(&mut self, cursor: (f32, f32), time: f32) {
        if self.state != State::Held {
            return;
        }
        (self.x, self.y) = (cursor.0 - self.grip.0, cursor.1 - self.grip.1);
        self.path.push((time, cursor.0, cursor.1));
        self.path.retain(|(then, ..)| time - then <= SWING * 2.0);
    }

    /// The hand lets go at `time` seconds: it flies on as the hand was moving.
    pub fn let_go(&mut self, time: f32) {
        if self.state != State::Held {
            return;
        }
        // From where the hand was a moment ago to where it is: a fling is the last of the swing, not all of it.
        let lately: Vec<&(f32, f32, f32)> = self.path.iter().filter(|(then, ..)| time - then <= SWING).collect();
        self.speed = match (lately.first(), lately.last()) {
            (Some(from), Some(to)) if to.0 - from.0 > 0.004 => ((to.1 - from.1) / (to.0 - from.0), (to.2 - from.2) / (to.0 - from.0)),
            _ => (0.0, 0.0),
        };
        let fast = (self.speed.0.powi(2) + self.speed.1.powi(2)).sqrt();
        if fast > FASTEST {
            self.speed = (self.speed.0 * FASTEST / fast, self.speed.1 * FASTEST / fast);
        }
        self.state = State::Flying;
    }

    /// `dt` seconds go by. `ground` is how low its middle can get; `mon` is the middle of the mon it is flung at
    /// and how near that counts as striking it. True when it has just struck.
    pub fn step(&mut self, dt: f32, ground: f32, mon: (f32, f32, f32)) -> bool {
        match self.state {
            State::Flying => {
                // In steps short enough that a fast ball cannot pass through the mon between two of them.
                let fast = (self.speed.0.powi(2) + self.speed.1.powi(2)).sqrt().max(1.0);
                let steps = ((fast * dt / (mon.2 * 0.5).max(4.0)).ceil() as usize).clamp(1, 64);
                let each = dt / steps as f32;
                for _ in 0..steps {
                    self.speed.1 += GRAVITY * each;
                    self.x += self.speed.0 * each;
                    self.y += self.speed.1 * each;
                    self.spin += self.speed.0 * each / 28.0;
                    if ((self.x - mon.0).powi(2) + (self.y - mon.1).powi(2)).sqrt() <= mon.2 {
                        self.state = State::Away;
                        return true;
                    }
                    if self.y >= ground {
                        self.y = ground;
                        self.state = State::Landed(LIES);
                        return false;
                    }
                }
                // Flung clean off the desk: it is not coming down anywhere it can be seen.
                if (self.x - self.rest.0).abs() > 6000.0 {
                    self.rest_at(self.rest.0, self.rest.1);
                }
            }
            State::Landed(left) if left <= dt => self.rest_at(self.rest.0, self.rest.1),
            State::Landed(left) => self.state = State::Landed(left - dt),
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND: f32 = 1000.0;
    /// A mon standing three hundred pixels to the right of where the ball rests, its middle forty up.
    const MON: (f32, f32, f32) = (800.0, 960.0, 45.0);

    /// Picks the ball up and swings it for a tenth of a second at a speed, then lets go.
    fn flung(speed: (f32, f32)) -> Toss {
        let mut toss = Toss::new(500.0, GROUND);
        assert!(toss.pick_up((503.0, 998.0)));
        for step in 0..=10 {
            let time = step as f32 * 0.01;
            toss.carry((503.0 + speed.0 * time, 998.0 + speed.1 * time), time);
        }
        toss.let_go(0.1);
        toss
    }

    /// Lets time go by until the ball is at rest again or has struck. Whether it struck.
    fn played(toss: &mut Toss) -> bool {
        for _ in 0..600 {
            if toss.step(1.0 / 30.0, GROUND, MON) {
                return true;
            }
            if toss.state == State::Rest {
                return false;
            }
        }
        panic!("the ball never came to rest: {:?}", toss.state);
    }

    #[test]
    fn flung_at_the_mon_it_strikes_it() {
        let mut toss = flung((900.0, -250.0));
        assert_eq!(toss.state, State::Flying);
        assert!(played(&mut toss));
        assert!(toss.state == State::Away && !toss.shown());
        // However hard: a fast ball does not pass through between two frames.
        assert!(played(&mut flung((3000.0, -250.0))));
    }

    #[test]
    fn flung_the_other_way_it_lands_lies_a_moment_and_is_back_where_it_rested() {
        let mut toss = flung((-600.0, -400.0));
        let mut landed = false;
        for _ in 0..600 {
            assert!(!toss.step(1.0 / 30.0, GROUND, MON));
            landed |= matches!(toss.state, State::Landed(_)) && toss.x < 400.0 && toss.y == GROUND;
            if toss.state == State::Rest {
                break;
            }
        }
        assert!(landed, "it came down to the left of where it was");
        assert_eq!((toss.x, toss.y, toss.state), (500.0, GROUND, State::Rest));
    }

    #[test]
    fn let_go_without_a_swing_it_only_drops() {
        let mut toss = Toss::new(500.0, GROUND);
        toss.pick_up((500.0, 1000.0));
        toss.carry((500.0, 700.0), 0.0);
        // Held still for a second before it is let go.
        toss.carry((500.0, 700.0), 1.0);
        toss.let_go(1.0);
        assert!(!played(&mut toss));
    }

    #[test]
    fn it_is_only_taken_from_where_it_lies() {
        let mut toss = flung((100.0, -900.0));
        assert!(!toss.pick_up((0.0, 0.0)), "not out of the air");
        while !matches!(toss.state, State::Landed(_)) {
            toss.step(1.0 / 30.0, GROUND, MON);
        }
        assert!(toss.pick_up((toss.x, toss.y)), "but from where it landed");
        let mut struck = flung((900.0, -250.0));
        played(&mut struck);
        assert!(!struck.pick_up((0.0, 0.0)), "nor once it has struck");
    }
}
