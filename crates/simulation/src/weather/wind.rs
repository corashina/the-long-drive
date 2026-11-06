//! One global wind on the fixed step. A new feature: the source has only sandstorm gusts.
use super::{CellSample, StormRandom, WeatherKind};
use std::f32::consts::{PI, TAU};

/// Wind speed in meters per second of each kind, in `WeatherKind` order.
const SPEEDS: [f32; 5] = [2., 4., 6., 12., 1.];
/// Largest direction change in radians per second: 9 degrees per minute.
const TURN_RATE: f32 = 9. * PI / 180. / 60.;
/// Seconds between the keys of the turn noise.
const TURN_PERIOD: f32 = 40.;
/// Seconds for the speed to close 63 % of the gap to its target.
const SPEED_SECONDS: f32 = 15.;
/// Seconds between the keys of the low and the high gust octave.
const GUST_PERIODS: [f32; 2] = [6., 1.5];
/// Cells drift at this share of the wind speed.
const DRIFT_SHARE: f32 = 0.6;

/// Smoothed value noise: random keys in -1..1 at a fixed period.
#[derive(Debug)]
struct Noise {
    from: f32,
    to: f32,
    /// 0..1 from `from` to `to`.
    t: f32,
    period: f32,
}

impl Noise {
    fn new(period: f32, random: &mut StormRandom) -> Self {
        Self {
            from: random.range(-1., 1.),
            to: random.range(-1., 1.),
            t: 0.,
            period,
        }
    }
    fn advance(&mut self, dt: f32, random: &mut StormRandom) -> f32 {
        self.t += dt / self.period;
        while self.t >= 1. {
            self.t -= 1.;
            self.from = self.to;
            self.to = random.range(-1., 1.);
        }
        let s = self.t * self.t * (3. - 2. * self.t);
        self.from + (self.to - self.from) * s
    }
}

#[derive(Debug)]
pub struct Wind {
    /// Unit direction the wind blows toward, world `x`, `z`.
    pub dir: [f32; 2],
    /// Meters per second without the gust.
    pub speed: f32,
    /// Speed factor, 0.5 to 2.
    pub gust: f32,
    angle: f32,
    turn: Noise,
    gusts: [Noise; 2],
    random: StormRandom,
}

impl Wind {
    pub fn new(seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x7769_6e64_0001);
        let angle = random.range(0., TAU);
        let turn = Noise::new(TURN_PERIOD, &mut random);
        let gusts = GUST_PERIODS.map(|period| Noise::new(period, &mut random));
        Self {
            dir: [angle.cos(), angle.sin()],
            speed: SPEEDS[WeatherKind::Clear as usize],
            gust: 1.,
            angle,
            turn,
            gusts,
            random,
        }
    }

    /// Cell drift in meters per second along world `x`, `z`.
    pub fn drift(&self) -> [f32; 2] {
        [
            self.dir[0] * self.speed * DRIFT_SHARE,
            self.dir[1] * self.speed * DRIFT_SHARE,
        ]
    }

    /// Wind velocity with the gust, in meters per second along world `x`, `z`.
    pub fn velocity(&self) -> [f32; 2] {
        let speed = self.speed * self.gust;
        [self.dir[0] * speed, self.dir[1] * speed]
    }

    /// Turns the direction, moves the speed toward the kind strength of `sample` and gives a new gust.
    /// `heading` replaces the random turn: a guest follows the drift of the host.
    pub fn advance(&mut self, dt: f32, sample: &CellSample, heading: Option<[f32; 2]>) {
        let turn = self.turn.advance(dt, &mut self.random);
        match heading.filter(|h| *h != [0.; 2]) {
            Some(h) => self.angle = h[1].atan2(h[0]),
            None => self.angle = (self.angle + turn * TURN_RATE * dt).rem_euclid(TAU),
        }
        self.dir = [self.angle.cos(), self.angle.sin()];
        // Clear also gets the share that no cell covers.
        let covered: f32 = sample.weights[1..].iter().sum();
        let mut total = 0.;
        let mut weighted = 0.;
        for (i, &speed) in SPEEDS.iter().enumerate() {
            let w = if i == 0 {
                sample.weights[0].max(1. - covered)
            } else {
                sample.weights[i]
            };
            total += w;
            weighted += w * speed;
        }
        let target = weighted / total.max(1e-6);
        self.speed += (target - self.speed) * (dt / SPEED_SECONDS).min(1.);
        let [low, high] = &mut self.gusts;
        let noise =
            0.65 * low.advance(dt, &mut self.random) + 0.35 * high.advance(dt, &mut self.random);
        self.gust = noise.exp2();
    }
}
