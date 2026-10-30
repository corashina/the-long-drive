//! Source computerscript: a text screen, a small program store and a piano mode.
pub mod screen;

use std::collections::BTreeMap;

/// Seconds between cursor blinks.
const BLINK: f64 = 0.35;
/// cmdlist and cmdrun wait after each line.
const LIST_WAIT: f64 = 0.05;
const RUN_WAIT: f64 = 0.01;
const BACKSPACE: u8 = 8;
const ENTER: [u8; 2] = *b"\r\n";

/// Arrow bits of `TypingFrame::arrows`.
pub mod arrows {
    pub const UP: u8 = 1;
    pub const DOWN: u8 = 2;
    pub const LEFT: u8 = 4;
    pub const RIGHT: u8 = 8;
}
/// Music mode key bits of `TypingFrame::pressed`.
pub mod music_keys {
    /// A: lower octave.
    pub const LOWER: u8 = 1;
    /// S: higher octave.
    pub const HIGHER: u8 = 2;
    /// X: leave music mode.
    pub const EXIT: u8 = 4;
}
/// Piano keys in source order (bit i of `TypingFrame::piano`): note and octave offset.
pub const PIANO: [(u8, u8); 17] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 0),
    (0, 1),
    (1, 1),
    (2, 1),
    (3, 1),
    (4, 1),
];

/// One frame of keyboard input while the player uses a computer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TypingFrame<'a> {
    /// Input.inputString: printable ASCII, 8 (Backspace) and 13 (Enter).
    pub text: &'a [u8],
    /// Input.anyKey: a key is held.
    pub any_key: bool,
    /// Held arrow keys (`arrows` bits).
    pub arrows: u8,
    /// Held piano keys (bit i is `PIANO[i]`).
    pub piano: u32,
    /// Music keys pressed this frame (`music_keys` bits).
    pub pressed: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobKind {
    List,
    Run,
}
/// A cmdlist or cmdrun coroutine. It stops when the program changes, as the source
/// SortedDictionary enumerator throws.
#[derive(Clone, Copy, Debug)]
struct Job {
    kind: JobKind,
    /// The next program key at or after this value.
    next: u32,
    revision: u32,
    due: f64,
}

/// A note semitone: note + octave * 12. Its pitch is 2^((semitone - 48) / 12).
pub fn note_pitch(semitone: u8) -> f32 {
    2f32.powf((f32::from(semitone) - 48.) / 12.)
}

#[derive(Clone, Debug)]
pub struct Computer {
    pub on: bool,
    /// Use(input) was called: the computer reads the keyboard while the player types.
    pub used: bool,
    x_size: i32,
    y_size: i32,
    grid: Box<[u8]>,
    cursor: [i32; 2],
    cursor_visible: bool,
    last_blink: f64,
    program: BTreeMap<u16, String>,
    revision: u32,
    jobs: Vec<Job>,
    pub music: bool,
    pub octave: u8,
    /// Playing note semitones (the source `sounds`).
    pub notes: u128,
    /// Notes of a replaced `sounds` dictionary. Nothing stops them.
    pub orphans: u128,
    /// The grid or the cursor changed since the last `take_changed`.
    changed: bool,
}

impl Computer {
    pub fn new(x_size: u16, y_size: u16) -> Self {
        Self {
            on: false,
            used: false,
            x_size: i32::from(x_size),
            y_size: i32::from(y_size),
            grid: vec![b' '; usize::from(x_size) * usize::from(y_size)].into_boxed_slice(),
            cursor: [0; 2],
            cursor_visible: true,
            last_blink: 0.,
            program: BTreeMap::new(),
            revision: 0,
            jobs: Vec::new(),
            music: false,
            octave: 0,
            notes: 0,
            orphans: 0,
            changed: false,
        }
    }
    pub fn grid(&self) -> &[u8] {
        &self.grid
    }
    pub fn cursor(&self) -> [i32; 2] {
        self.cursor
    }
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    pub fn turn(&mut self) {
        if self.on {
            self.turn_off();
        } else {
            self.turn_on();
        }
    }
    pub fn turn_off(&mut self) {
        self.notes = 0;
        self.on = false;
        self.grid.fill(b' ');
        self.cursor = [0; 2];
        self.changed = true;
    }
    pub fn turn_on(&mut self) {
        self.on = true;
        self.music = false;
        self.write_ln(b"Ready.");
    }
    /// Update: the cursor blink and the running jobs. Returns true when the screen changed.
    pub fn update(&mut self, time: f64) -> bool {
        if time - self.last_blink >= BLINK && !self.music {
            self.last_blink = time;
            self.cursor_visible = !self.cursor_visible;
            self.changed = true;
        }
        let mut i = 0;
        while i < self.jobs.len() {
            if self.step_job(i, time) {
                i += 1;
            } else {
                self.jobs.swap_remove(i);
            }
        }
        self.changed
    }
    /// One cmdlist or cmdrun step. Returns false when the job ends.
    fn step_job(&mut self, index: usize, time: f64) -> bool {
        let job = self.jobs[index];
        if time < job.due {
            return true;
        }
        if job.revision != self.revision {
            return false;
        }
        // Keys have at most four digits, so `next` stays below u16::MAX.
        let next = self
            .program
            .range(job.next as u16..)
            .next()
            .map(|(k, v)| (*k, v.clone()));
        let Some((key, line)) = next else {
            self.write_ln2(b"Ready.");
            return false;
        };
        let wait = match job.kind {
            JobKind::List => {
                let text = format!("{key} {line}");
                self.write_ln(text.as_bytes());
                LIST_WAIT
            }
            JobKind::Run => {
                self.execute(line.as_bytes());
                RUN_WAIT
            }
        };
        let job = &mut self.jobs[index];
        job.next = u32::from(key) + 1;
        job.due = time + wait;
        true
    }
    /// The source Update while the player uses this computer.
    pub fn input(&mut self, frame: TypingFrame, time: f64) {
        if self.music {
            self.notes = 0;
            for (i, (note, octave)) in PIANO.iter().enumerate() {
                if frame.piano & 1 << i != 0 {
                    self.notes |= 1u128 << (note + (self.octave + octave) * 12);
                }
            }
            if frame.pressed & music_keys::LOWER != 0 {
                self.change_octave(-1);
            }
            if frame.pressed & music_keys::HIGHER != 0 {
                self.change_octave(1);
            }
            if frame.pressed & music_keys::EXIT != 0 {
                self.music = false;
                self.write_ln2(b"Ready.");
            }
        } else {
            self.read(frame, time);
        }
    }
    fn change_octave(&mut self, step: i8) {
        self.notes = 0;
        self.octave = (self.octave as i8 + step).clamp(0, 9) as u8;
        let text = format!("Octave: {}", self.octave);
        self.write_ln(text.as_bytes());
    }
    /// InputRead. A write outside the grid ends the frame, as the source index throws.
    fn read(&mut self, frame: TypingFrame, time: f64) {
        if !frame.text.is_empty() || frame.any_key {
            for &c in frame.text {
                match c {
                    BACKSPACE => {
                        self.cursor[0] -= 1;
                        self.cursor_limits();
                        let i = self.index();
                        self.grid[i] = c;
                    }
                    c if ENTER.contains(&c) => {
                        if !self.hit_enter(time) {
                            self.cursor_limits();
                            return;
                        }
                    }
                    c => {
                        let i = self.cursor[0] + self.cursor[1] * self.x_size;
                        if i < 0 || i as usize >= self.grid.len() {
                            self.cursor_limits();
                            return;
                        }
                        self.grid[i as usize] = c;
                        self.cursor[0] += 1;
                    }
                }
                self.cursor_limits();
            }
            self.cursor_limits();
            self.changed = true;
        }
        let step = if frame.arrows & arrows::UP != 0 {
            [0, -1]
        } else if frame.arrows & arrows::DOWN != 0 {
            [0, 1]
        } else if frame.arrows & arrows::LEFT != 0 {
            [-1, 0]
        } else if frame.arrows & arrows::RIGHT != 0 {
            [1, 0]
        } else {
            return;
        };
        self.cursor[0] += step[0];
        self.cursor[1] += step[1];
        self.cursor_visible = true;
        self.changed = true;
    }
    fn index(&self) -> usize {
        (self.cursor[0] + self.cursor[1] * self.x_size) as usize
    }
    /// HitEnter. Returns false when the cursor row lies outside the grid.
    fn hit_enter(&mut self, time: f64) -> bool {
        let start = self.cursor[1] * self.x_size;
        if start < 0 || self.cursor[1] >= self.y_size {
            return false;
        }
        let start = start as usize;
        let line: Vec<u8> = self.grid[start..start + self.x_size as usize - 1]
            .iter()
            .map(u8::to_ascii_lowercase)
            .collect();
        let line = trim_spaces(&line);
        self.cursor[1] += 1;
        self.cursor[0] = 0;
        if line.is_empty() {
            return true;
        }
        if line[0].is_ascii_digit() {
            self.write_program(line);
            return true;
        }
        match keyword(line) {
            Err(()) => self.write_program(line),
            Ok(0) => self.start_job(JobKind::Run, time),
            Ok(1) => self.start_job(JobKind::List, time),
            Ok(2) => {
                self.orphans |= self.notes;
                self.notes = 0;
                self.music = true;
                self.octave = 4;
                for text in [
                    "Started music mode",
                    "Play: qwerty...",
                    "Higher octave: S",
                    "Lower octave: A",
                    "Exit music mode: X",
                    "Octave: 4",
                ] {
                    self.write_ln(text.as_bytes());
                }
            }
            Ok(_) => {}
        }
        true
    }
    /// StartCoroutine runs the first step at once.
    fn start_job(&mut self, kind: JobKind, time: f64) {
        self.jobs.push(Job {
            kind,
            next: 0,
            revision: self.revision,
            due: time,
        });
        let i = self.jobs.len() - 1;
        if !self.step_job(i, time) {
            self.jobs.swap_remove(i);
        }
    }
    fn write_program(&mut self, line: &[u8]) {
        let digits = line
            .iter()
            .take(4)
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 {
            self.write_ln2(b"?Syntax  Error");
            return;
        }
        let key = line[..digits]
            .iter()
            .fold(0u16, |v, c| v * 10 + u16::from(c - b'0'));
        if self.program.remove(&key).is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
        let text = &line[digits..];
        if !text.is_empty() {
            self.program
                .insert(key, String::from_utf8_lossy(trim_spaces(text)).into_owned());
            self.revision = self.revision.wrapping_add(1);
        }
    }
    /// execute: only lines that contain "print" write their text after the fifth byte.
    fn execute(&mut self, line: &[u8]) {
        if line.windows(5).any(|w| w == b"print") {
            self.write_ln(&line[5..]);
        }
    }
    pub fn write_ln2(&mut self, text: &[u8]) {
        self.cursor[0] = 0;
        self.cursor[1] += 1;
        self.cursor_limits();
        self.write_ln(text);
    }
    pub fn write_ln(&mut self, text: &[u8]) {
        self.write(text);
        self.cursor[0] = 0;
        self.cursor[1] += 1;
    }
    pub fn write(&mut self, text: &[u8]) {
        for &c in text {
            self.cursor_limits();
            let i = self.index();
            self.grid[i] = c;
            self.cursor[0] += 1;
        }
        self.cursor_limits();
        self.changed = true;
    }
    /// CursorLimits. A wrap at the right edge always scrolls, as in the source.
    fn cursor_limits(&mut self) {
        if self.cursor[0] < 0 {
            if self.cursor[1] > 0 {
                self.cursor = [self.x_size - 1, self.cursor[1] - 1];
            } else {
                self.cursor[0] = 0;
            }
        } else if self.cursor[0] > self.x_size - 1 {
            self.cursor = [0, self.cursor[1] + 1];
            self.scroll_down();
        }
        if self.cursor[1] > self.y_size - 1 {
            self.cursor[1] = self.y_size - 1;
            self.scroll_down();
        } else if self.cursor[1] < 0 {
            self.cursor[1] = 0;
        }
    }
    fn scroll_down(&mut self) {
        let x = self.x_size as usize;
        self.grid.copy_within(x.., 0);
        let len = self.grid.len();
        self.grid[len - x..].fill(b' ');
    }
}

fn trim_spaces(text: &[u8]) -> &[u8] {
    let start = text.iter().position(|c| *c != b' ').unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|c| *c != b' ')
        .map_or(start, |i| i + 1);
    &text[start..end]
}

/// Enum.Parse(typeof(keywords), line): a number, or names joined with commas.
fn keyword(line: &[u8]) -> Result<i64, ()> {
    let text = std::str::from_utf8(line).map_err(|_| ())?.trim();
    let first = *text.as_bytes().first().ok_or(())?;
    if first.is_ascii_digit() || first == b'-' || first == b'+' {
        return text.parse::<i32>().map(i64::from).map_err(|_| ());
    }
    text.split(',').try_fold(0, |value, name| {
        Ok(value
            | match name.trim() {
                "run" => 0,
                "list" => 1,
                "music" => 2,
                _ => return Err(()),
            })
    })
}
