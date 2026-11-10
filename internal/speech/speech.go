// Package speech is offline speech recognition on the controller: the
// sherpa-onnx C API bound through purego, a SenseVoice recognizer and the
// resampler that brings recorder audio to the model's rate. It has no Node,
// transport or UI dependency, and audio never leaves the process.
package speech

import (
	"errors"
	"math"
)

const (
	// SampleRate is the rate the recognizer decodes, in Hz.
	SampleRate = 16_000
	// MaxSamples bounds one recording to five minutes.
	MaxSamples = SampleRate * 300
)

var (
	// ErrUnavailable reports that the packaged speech runtime is absent or
	// is not the pinned build.
	ErrUnavailable = errors.New("speech runtime is unavailable")
	// ErrInvalidModel reports model files the recognizer cannot load.
	ErrInvalidModel = errors.New("speech model validation failed")
	// ErrRecognition reports a failed decode.
	ErrRecognition = errors.New("speech recognition failed")
	// ErrInvalidAudio reports audio outside the accepted bounds.
	ErrInvalidAudio = errors.New("invalid speech audio")
)

// Language is the language the recognizer assumes.
type Language int

const (
	Auto Language = iota
	Chinese
	English
)

// Languages are the values of the dictation.language preference, in the
// order of the constants.
var Languages = [...]string{"auto", "chinese", "english"}

// ParseLanguage maps a dictation.language preference; anything else is Auto.
func ParseLanguage(preference string) Language {
	for language, name := range Languages {
		if name == preference {
			return Language(language)
		}
	}
	return Auto
}

// Code is the SenseVoice language code.
func (l Language) Code() string {
	switch l {
	case Chinese:
		return "zh"
	case English:
		return "en"
	}
	return "auto"
}

// Normalize accepts mono PCM from a platform recorder and returns it at
// SampleRate. The rate and the duration are bounded and every sample must
// be finite.
func Normalize(samples []float32, rate uint32) ([]float32, error) {
	if rate < 8_000 || rate > 192_000 || len(samples) > int(rate)*300 || !finite(samples) {
		return nil, ErrInvalidAudio
	}
	if rate == SampleRate {
		return samples, nil
	}
	sampler := NewSampler(rate)
	audio := make([]float32, 0, len(samples)*SampleRate/int(rate))
	for _, sample := range samples {
		sampler.Push(sample, func(value float32) { audio = append(audio, value) })
	}
	return audio, nil
}

func finite(samples []float32) bool {
	for _, sample := range samples {
		if value := float64(sample); math.IsNaN(value) || math.IsInf(value, 0) {
			return false
		}
	}
	return true
}

// epsilon is the single-precision machine epsilon: samples within it are
// silence.
const epsilon = 1.1920929e-07

// Silent reports audio the recognizer returns no text for: shorter than a
// fifth of a second, or without any sample above the noise floor.
func Silent(samples []float32) bool {
	if len(samples) < SampleRate/5 {
		return true
	}
	for _, sample := range samples {
		if sample > epsilon || sample < -epsilon {
			return false
		}
	}
	return true
}
