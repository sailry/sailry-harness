use crate::SAMPLE_RATE;

pub struct Sampler {
    rate: u32,
    phase: u64,
    sum: f32,
    count: u32,
}
impl Sampler {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            phase: 0,
            sum: 0.,
            count: 0,
        }
    }
    pub fn push(&mut self, value: f32, mut emit: impl FnMut(f32)) {
        self.sum += if value.is_finite() {
            value.clamp(-1., 1.)
        } else {
            0.
        };
        self.count += 1;
        self.phase += u64::from(SAMPLE_RATE);
        if self.phase >= u64::from(self.rate) {
            let value = self.sum / self.count as f32;
            while self.phase >= u64::from(self.rate) {
                emit(value);
                self.phase -= u64::from(self.rate);
            }
            self.sum = 0.;
            self.count = 0;
        }
    }
}
