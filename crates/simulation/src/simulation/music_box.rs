//! Source `musicboxscript`: program parsing and crank playback.

/// One `noteinprogram`. A rest has no pitch.
struct Note {
    time: f32,
    pitch: Option<f32>,
    after: bool,
}

pub struct MusicBox {
    notes: Vec<Note>,
    time: f32,
    last: [f32; 2],
    sens: f32,
}

impl MusicBox {
    /// `musicboxscript.Start` with `defprogram`. Returns None for an empty program.
    pub fn new<'a>(program: impl Iterator<Item = &'a str>, pause: f32, sens: f32) -> Option<Self> {
        let mut notes = Vec::new();
        for line in program {
            let line: Vec<char> = line.chars().collect();
            let mut so_far = 0.;
            let mut read = 0;
            while read < line.len() {
                while read < line.len() && line[read] == ' ' {
                    read += 1;
                }
                let start = read;
                while read < line.len() && line[read] != ' ' && line[read] != '|' {
                    read += 1;
                }
                let mut token = &line[start..read];
                read += 1;
                let mut next = pause;
                if let [rest @ .., last] = token
                    && !rest.is_empty()
                {
                    if let Some(d @ 2..=9) = last.to_digit(10) {
                        next *= d as f32;
                    }
                    token = rest;
                }
                next += so_far;
                notes.push(Note {
                    time: so_far,
                    pitch: note_pitch(token),
                    after: false,
                });
                so_far = next;
            }
        }
        (!notes.is_empty()).then_some(Self {
            notes,
            time: 0.,
            last: [0.; 2],
            sens,
        })
    }
    /// One `Update` with the crank look input (source X right, Y up). Calls `play` with
    /// the base pitch of each passed note and returns `playSpeed`.
    pub fn step(&mut self, look: [f32; 2], dt: f32, mut play: impl FnMut(f32)) -> f32 {
        let speed = signed_angle(self.last, look) * self.sens;
        self.last = look;
        if speed == 0. {
            return 0.;
        }
        self.time += dt * speed;
        for n in &self.notes {
            if (self.time > n.time) != n.after
                && let Some(pitch) = n.pitch
            {
                play(pitch);
            }
        }
        let end = self.notes[self.notes.len() - 1].time;
        if self.time > end {
            self.time = 0.;
        } else if self.time < 0. {
            self.time = end;
        }
        for n in &mut self.notes {
            n.after = self.time > n.time;
        }
        speed
    }
    pub fn heap_bytes(&self) -> usize {
        self.notes.capacity() * std::mem::size_of::<Note>()
    }
}

/// `PlayNote(string)`. The source reads `input[1]` twice for accidentals and octave.
fn note_pitch(token: &[char]) -> Option<f32> {
    let mut note = match token.first() {
        Some('0') => return None,
        Some('D' | 'd') => 2,
        Some('E' | 'e') => 4,
        Some('F' | 'f') => 5,
        Some('G' | 'g') => 7,
        Some('A' | 'a') => 9,
        Some('B' | 'b' | 'H' | 'h') => 11,
        _ => 0,
    };
    let mut octave = 4;
    for i in 1..3 {
        if i < token.len() {
            match token[1] {
                'b' => note -= 1,
                '#' => note += 1,
                c => {
                    if let Some(d) = c.to_digit(10) {
                        octave = d as i32;
                    }
                }
            }
        }
    }
    Some(2_f32.powf((note + (octave - 4) * 12) as f32 / 12.))
}

/// `Vector2.SignedAngle` in degrees.
fn signed_angle(from: [f32; 2], to: [f32; 2]) -> f32 {
    let d = ((from[0] * from[0] + from[1] * from[1]) * (to[0] * to[0] + to[1] * to[1])).sqrt();
    if d < 1e-15 {
        return 0.;
    }
    let angle = ((from[0] * to[0] + from[1] * to[1]) / d)
        .clamp(-1., 1.)
        .acos()
        .to_degrees();
    if from[0] * to[1] - from[1] * to[0] >= 0. {
        angle
    } else {
        -angle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_parse_follows_source() {
        let b = MusicBox::new(["009  g43 c#43 0 a52|h"].into_iter(), 0.1, 1.).unwrap();
        let times = [0., 0.9, 1.2, 1.5, 1.6, 1.8];
        assert_eq!(b.notes.len(), times.len());
        for (n, t) in b.notes.iter().zip(times) {
            assert!((n.time - t).abs() < 1e-5);
        }
        // "c#4" reads '#' twice. A one-char token keeps its last char.
        let p = |semitones: i32| Some(2_f32.powf(semitones as f32 / 12.));
        let pitches: Vec<_> = b.notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [None, p(7), p(2), None, p(21), p(11)]);
    }

    #[test]
    fn crank_plays_crossed_notes_both_ways() {
        let mut b = MusicBox::new(["c2 d2 e2"].into_iter(), 0.1, -1.).unwrap();
        let mut played = Vec::new();
        // Clockwise quarter turns give -90 degrees; sens -1 plays forward 0.09 s each.
        assert_eq!(b.step([0., 1.], 0.001, |p| played.push(p)), 0.);
        for look in [[1., 0.], [0., -1.], [-1., 0.]] {
            assert_eq!(b.step(look, 0.001, |p| played.push(p)), 90.);
        }
        let d = 2_f32.powf(2. / 12.);
        assert_eq!(played, [1., d]);
        assert_eq!(b.step([0., -1.], 0.001, |p| played.push(p)), -90.);
        assert_eq!(played, [1., d, d]);
    }
}
