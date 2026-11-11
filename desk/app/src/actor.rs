//! What a mon does on the desktop: stands, wanders along whatever it stands on, falls when that goes from under
//! it, and is carried by the mouse. Nothing here asks Windows anything; it is told the surfaces and the time.

use crate::desk::Surface;

/// How fast a falling mon gains speed, in pixels a second each second, and how fast it turns, in radians a second.
const GRAVITY: f32 = 2600.0;
const TURN: f32 = 7.0;
/// How far round from facing the screen it turns to walk, in radians.
const SIDEWAYS: f32 = 1.2;
/// How quickly it falls into and out of its stride, in shares a second.
const EASE: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Standing, for this many seconds more.
    Idle(f32),
    /// Walking to this x.
    Walk(f32),
    /// Falling at this many pixels a second.
    Fall(f32),
    Carried,
}

pub struct Actor {
    /// Where its feet are on the screen, in pixels.
    pub x: f32,
    pub y: f32,
    pub state: State,
    /// How far it is turned from facing the screen, and how much of its walk it is in, from none to all.
    pub yaw: f32,
    pub gait: f32,
    /// How far into its stride it is, in seconds.
    pub stride: f32,
    /// How fast it walks, in pixels a second, and how near an edge it will stand.
    pub pace: f32,
    pub margin: f32,
    /// For measuring what it costs: false and it never leaves where it stands, true and it never stands for long.
    pub restless: Option<bool>,
    /// Going for good: off the end of whatever it is on, and off the side of the screen.
    pub leaving: bool,
    /// What it stands on: the window, and how far it is from that window's left edge.
    owner: isize,
    offset: f32,
    seed: u64,
}

impl Actor {
    pub fn new(x: f32, y: f32, pace: f32, margin: f32, seed: u64) -> Actor {
        // Let go in the air: it finds the ground by falling to it.
        Actor { x, y, state: State::Fall(0.0), yaw: 0.0, gait: 0.0, stride: 0.0, pace, margin, restless: None, leaving: false, owner: 0, offset: 0.0, seed: seed | 1 }
    }

    /// A number from 0 up to 1, the next of its own.
    fn chance(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Whether it has to be drawn at a walk's rate: anything but standing with its walk fully faded out.
    pub fn moving(&self) -> bool {
        !matches!(self.state, State::Idle(_)) || self.gait > 0.0 || self.yaw.abs() > 0.001
    }

    /// The surface under its feet, if it is on one.
    fn standing<'a>(&self, surfaces: &'a [Surface]) -> Option<&'a Surface> {
        surfaces.iter().find(|s| s.owner == self.owner && (s.y as f32 - self.y).abs() < 1.5 && self.x >= s.x0 as f32 && self.x <= s.x1 as f32)
    }

    pub fn pick_up(&mut self) {
        self.state = State::Carried;
    }

    pub fn carry(&mut self, x: f32, y: f32) {
        if self.state == State::Carried {
            (self.x, self.y) = (x, y);
        }
    }

    pub fn let_go(&mut self) {
        if self.state == State::Carried {
            self.state = State::Fall(0.0);
        }
    }

    /// The desktop's windows have changed: it rides the window it stands on, or falls if that is gone from under it.
    pub fn resettle(&mut self, surfaces: &[Surface]) {
        if matches!(self.state, State::Fall(_) | State::Carried) {
            return;
        }
        let x = if self.owner == 0 { self.x } else { surfaces.iter().find(|s| s.owner == self.owner).map_or(self.x, |s| s.left as f32 + self.offset) };
        match surfaces.iter().find(|s| s.owner == self.owner && x >= s.x0 as f32 && x <= s.x1 as f32) {
            Some(surface) => (self.x, self.y) = (x, surface.y as f32),
            None => self.state = State::Fall(0.0),
        }
    }

    pub fn step(&mut self, dt: f32, surfaces: &[Surface]) {
        let mut walking = 0.0;
        match self.state {
            State::Carried => {}
            State::Fall(speed) => {
                let speed = speed + GRAVITY * dt;
                let below = self.y + speed * dt;
                // The highest thing between where its feet were and where they would now be.
                let landing = surfaces
                    .iter()
                    .filter(|s| self.x >= s.x0 as f32 && self.x <= s.x1 as f32 && s.y as f32 >= self.y - 1.0 && s.y as f32 <= below)
                    .min_by_key(|s| s.y);
                match landing {
                    Some(surface) => {
                        self.y = surface.y as f32;
                        self.owner = surface.owner;
                        self.state = State::Idle(0.6 + self.chance());
                    }
                    None => {
                        self.y = below;
                        self.state = State::Fall(speed);
                        // Nothing under it at all: it was let go off the side of every screen. Back over the nearest floor.
                        if !self.leaving && !surfaces.iter().any(|s| self.x >= s.x0 as f32 && self.x <= s.x1 as f32 && s.y as f32 >= self.y) {
                            if let Some(floor) = surfaces.iter().filter(|s| s.owner == 0).min_by_key(|s| ((self.x - (s.x0 + s.x1) as f32 / 2.0).abs()) as i32) {
                                self.x = self.x.clamp(floor.x0 as f32 + self.margin, floor.x1 as f32 - self.margin);
                                self.y = self.y.min(floor.y as f32 - 1.0);
                            }
                        }
                    }
                }
            }
            State::Idle(left) => {
                let Some(surface) = self.standing(surfaces).copied() else {
                    self.state = State::Fall(0.0);
                    return;
                };
                self.state = State::Idle(left - dt);
                if self.leaving {
                    // The nearer end of what it stands on, and past it.
                    let nearer = if self.x - (surface.x0 as f32) < surface.x1 as f32 - self.x { surface.x0 as f32 - self.margin * 6.0 } else { surface.x1 as f32 + self.margin * 6.0 };
                    self.state = State::Walk(nearer);
                } else if left <= dt {
                    let (from, to) = (surface.x0 as f32 + self.margin, surface.x1 as f32 - self.margin);
                    let roll = match self.restless {
                        Some(true) => 0.5,
                        Some(false) => 1.0,
                        None => self.chance(),
                    };
                    self.state = if roll < 0.12 && surface.owner != 0 {
                        // Off the end of a window, to see what is down there.
                        State::Walk(if self.chance() < 0.5 { surface.x0 as f32 - self.margin } else { surface.x1 as f32 + self.margin })
                    } else if roll < 0.7 && to > from {
                        // Somewhere along what it stands on, not so near that it only shuffles.
                        let target = from + (to - from) * self.chance();
                        if (target - self.x).abs() < self.margin { State::Idle(1.0 + 3.0 * self.chance()) } else { State::Walk(target) }
                    } else {
                        State::Idle(2.0 + 6.0 * self.chance())
                    };
                }
            }
            State::Walk(to) => {
                let Some(surface) = self.standing(surfaces).copied() else {
                    self.state = State::Fall(0.0);
                    return;
                };
                let way = (to - self.x).signum();
                walking = way;
                // Only as fast as it is into its stride and turned to go.
                let turned = (self.yaw * way / SIDEWAYS).clamp(0.0, 1.0);
                self.x += way * self.pace * self.gait * turned * dt;
                if (to - self.x) * way <= 0.0 {
                    self.x = to;
                    self.state = State::Idle(1.5 + 5.0 * self.chance());
                }
                if self.x < surface.x0 as f32 || self.x > surface.x1 as f32 {
                    self.state = State::Fall(0.0);
                }
            }
        }
        let ease = |value: f32, target: f32, rate: f32| if value < target { (value + rate * dt).min(target) } else { (value - rate * dt).max(target) };
        self.gait = ease(self.gait, if walking != 0.0 { 1.0 } else { 0.0 }, EASE);
        self.yaw = ease(self.yaw, walking * SIDEWAYS, TURN);
        if self.gait > 0.0 {
            self.stride += dt;
        } else {
            self.stride = 0.0;
        }
        if let Some(surface) = self.standing(surfaces) {
            self.offset = self.x - surface.left as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOR: Surface = Surface { y: 1000, x0: 0, x1: 1920, owner: 0, left: 0 };
    const WINDOW: Surface = Surface { y: 400, x0: 300, x1: 900, owner: 7, left: 300 };

    fn settle(actor: &mut Actor, surfaces: &[Surface]) {
        for _ in 0..600 {
            actor.step(1.0 / 60.0, surfaces);
            if !matches!(actor.state, State::Fall(_)) {
                return;
            }
        }
        panic!("still falling at {}", actor.y);
    }

    #[test]
    fn it_falls_to_the_highest_thing_under_it() {
        let mut over_window = Actor::new(500.0, 100.0, 80.0, 30.0, 1);
        settle(&mut over_window, &[FLOOR, WINDOW]);
        assert_eq!(over_window.y, 400.0);
        let mut beside_it = Actor::new(1200.0, 100.0, 80.0, 30.0, 1);
        settle(&mut beside_it, &[FLOOR, WINDOW]);
        assert_eq!(beside_it.y, 1000.0);
    }

    #[test]
    fn it_does_not_land_on_what_is_above_it() {
        let mut under_window = Actor::new(500.0, 600.0, 80.0, 30.0, 1);
        settle(&mut under_window, &[FLOOR, WINDOW]);
        assert_eq!(under_window.y, 1000.0);
    }

    #[test]
    fn it_rides_the_window_it_stands_on_and_falls_when_it_is_gone() {
        let mut actor = Actor::new(500.0, 100.0, 80.0, 30.0, 1);
        settle(&mut actor, &[FLOOR, WINDOW]);
        let moved = Surface { y: 250, x0: 700, x1: 1300, owner: 7, left: 700 };
        actor.resettle(&[FLOOR, moved]);
        assert_eq!((actor.x, actor.y), (900.0, 250.0));
        actor.resettle(&[FLOOR]);
        assert_eq!(actor.state, State::Fall(0.0));
        settle(&mut actor, &[FLOOR]);
        assert_eq!(actor.y, 1000.0);
    }

    #[test]
    fn it_walks_to_where_it_is_going_and_stands_there() {
        let mut actor = Actor::new(500.0, 999.0, 80.0, 30.0, 1);
        settle(&mut actor, &[FLOOR]);
        actor.state = State::Walk(700.0);
        let mut steps = 0;
        while matches!(actor.state, State::Walk(_)) {
            actor.step(1.0 / 30.0, &[FLOOR]);
            assert!(actor.yaw >= 0.0, "it turns the way it goes");
            steps += 1;
            assert!(steps < 600, "it never got there");
        }
        assert_eq!(actor.x, 700.0);
        // 200 pixels at 80 a second, and a little to turn and fall into stride.
        assert!((steps as f32 / 30.0 - 2.5).abs() < 0.8, "{steps} steps");
    }

    #[test]
    fn it_stays_on_what_it_stands_on_for_a_long_while_unless_it_means_to_leave() {
        let mut actor = Actor::new(500.0, 999.0, 80.0, 30.0, 42);
        for _ in 0..(30 * 600) {
            actor.step(1.0 / 30.0, &[FLOOR]);
            assert!(actor.x >= 0.0 && actor.x <= 1920.0 && actor.y <= 1000.0);
        }
    }

    #[test]
    fn leaving_it_walks_off_the_screen_and_does_not_come_back() {
        let mut actor = Actor::new(300.0, 999.0, 80.0, 30.0, 1);
        settle(&mut actor, &[FLOOR]);
        actor.leaving = true;
        for _ in 0..(30 * 30) {
            actor.step(1.0 / 30.0, &[FLOOR]);
        }
        assert!(actor.x < 0.0 && actor.y > 2000.0, "{} {}", actor.x, actor.y);
    }

    #[test]
    fn carried_it_goes_where_it_is_taken_and_falls_when_let_go() {
        let mut actor = Actor::new(500.0, 999.0, 80.0, 30.0, 1);
        settle(&mut actor, &[FLOOR]);
        actor.pick_up();
        actor.carry(800.0, 300.0);
        actor.step(1.0 / 30.0, &[FLOOR]);
        assert_eq!((actor.x, actor.y), (800.0, 300.0));
        actor.let_go();
        settle(&mut actor, &[FLOOR]);
        assert_eq!((actor.x, actor.y), (800.0, 1000.0));
    }
}
