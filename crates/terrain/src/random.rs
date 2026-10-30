//! Installed mscorlib seeded subtractive random stream.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RandomState {
    #[serde(with = "random_values")]
    pub values: [i32; 56],
    pub next: usize,
    pub paired: usize,
}

#[derive(Clone, Debug)]
pub struct SourceRandom {
    values: [i32; 56],
    next: usize,
    paired: usize,
}
impl SourceRandom {
    pub fn new(seed: i32) -> Self {
        let mut values = [0_i32; 56];
        let magnitude = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };
        let mut last = 161803398_i32.wrapping_sub(magnitude);
        values[55] = last;
        let mut value = 1_i32;
        let mut index = 0;
        for _ in 1..55 {
            index = (index + 21) % 55;
            values[index] = value;
            value = last.wrapping_sub(value);
            if value < 0 {
                value = value.wrapping_add(i32::MAX);
            }
            last = values[index];
        }
        for _ in 1..5 {
            for index in 1..56 {
                values[index] = values[index].wrapping_sub(values[1 + (index + 30) % 55]);
                if values[index] < 0 {
                    values[index] = values[index].wrapping_add(i32::MAX);
                }
            }
        }
        Self {
            values,
            next: 0,
            paired: 21,
        }
    }
    fn internal_sample(&mut self) -> i32 {
        self.next = if self.next + 1 == 56 {
            1
        } else {
            self.next + 1
        };
        self.paired = if self.paired + 1 == 56 {
            1
        } else {
            self.paired + 1
        };
        let mut value = self.values[self.next].wrapping_sub(self.values[self.paired]);
        if value == i32::MAX {
            value -= 1;
        }
        if value < 0 {
            value = value.wrapping_add(i32::MAX);
        }
        self.values[self.next] = value;
        value
    }

    pub fn sample(&mut self) -> f64 {
        self.internal_sample() as f64 * 4.656612875245797E-10
    }

    /// The upper bound is exclusive. Equal bounds still advance the stream.
    pub fn next_range(&mut self, min: i32, max: i32) -> i32 {
        assert!(min <= max, "source random bounds are reversed");
        let range = max as i64 - min as i64;
        if range <= i32::MAX as i64 {
            return ((self.sample() * range as f64) as i32).wrapping_add(min);
        }
        let mut value = self.internal_sample();
        if self.internal_sample() % 2 == 0 {
            value = -value;
        }
        let sample = (value as f64 + 2147483646.0) / 4294967293.0;
        ((sample * range as f64) as i64 + min as i64) as i32
    }

    pub fn snapshot(&self) -> RandomState {
        RandomState {
            values: self.values,
            next: self.next,
            paired: self.paired,
        }
    }

    pub fn restore(state: RandomState) -> Self {
        assert!(
            state.next < 56 && state.paired < 56,
            "invalid source random indices"
        );
        Self {
            values: state.values,
            next: state.next,
            paired: state.paired,
        }
    }
}

mod random_values {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[i32; 56], s: S) -> Result<S::Ok, S::Error> {
        v.as_slice().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[i32; 56], D::Error> {
        Vec::<i32>::deserialize(d)?
            .try_into()
            .map_err(|_| serde::de::Error::custom("Invalid random state length"))
    }
}
