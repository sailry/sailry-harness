//go:build !darwin && !windows && !linux

package reader

import "errors"

type nativeSpeech struct{}

func newNativeSpeech() (*nativeSpeech, error) {
	return nil, errors.New("system speech is unavailable on this platform")
}
func (*nativeSpeech) speak(string) error {
	return errors.New("system speech is unavailable on this platform")
}
func (*nativeSpeech) stop() error             { return nil }
func (*nativeSpeech) speaking() (bool, error) { return false, nil }
func (*nativeSpeech) close() error            { return nil }
