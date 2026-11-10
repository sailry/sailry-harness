package speech

import "math"

// Sampler converts a stream at another rate to SampleRate by averaging the
// input samples that fall into each output period. Upsampling repeats the
// average.
type Sampler struct {
	rate  uint32
	phase uint64
	sum   float32
	count uint32
}

// NewSampler converts from rate, in Hz.
func NewSampler(rate uint32) *Sampler { return &Sampler{rate: rate} }

// Push takes one input sample and emits the output samples it completes.
// A sample that is not finite counts as silence; others are clamped to ±1.
func (s *Sampler) Push(value float32, emit func(float32)) {
	if v := float64(value); math.IsNaN(v) || math.IsInf(v, 0) {
		value = 0
	}
	s.sum += min(max(value, -1), 1)
	s.count++
	s.phase += SampleRate
	if s.phase >= uint64(s.rate) {
		value := s.sum / float32(s.count)
		for s.phase >= uint64(s.rate) {
			emit(value)
			s.phase -= uint64(s.rate)
		}
		s.sum, s.count = 0, 0
	}
}
