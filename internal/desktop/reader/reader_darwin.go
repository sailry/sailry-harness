//go:build darwin

package reader

import (
	"errors"
	"sync"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var loadSpeech = sync.OnceValues(func() (uintptr, error) {
	if _, err := purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
		return 0, err
	}
	return purego.Dlopen("/System/Library/Frameworks/AVFoundation.framework/AVFoundation", purego.RTLD_NOW|purego.RTLD_GLOBAL)
})
var (
	speechNew       = objc.RegisterName("new")
	speechRelease   = objc.RegisterName("release")
	speechDrain     = objc.RegisterName("drain")
	speechUTF8      = objc.RegisterName("stringWithUTF8String:")
	speechUtterance = objc.RegisterName("speechUtteranceWithString:")
	speechSpeak     = objc.RegisterName("speakUtterance:")
	speechStop      = objc.RegisterName("stopSpeakingAtBoundary:")
	speechSpeaking  = objc.RegisterName("isSpeaking")
)

type nativeSpeech struct{ synthesizer objc.ID }

func newNativeSpeech() (*nativeSpeech, error) {
	if _, err := loadSpeech(); err != nil {
		return nil, err
	}
	engine := objc.ID(objc.GetClass("AVSpeechSynthesizer")).Send(speechNew)
	if engine == 0 {
		return nil, errors.New("system speech initialization failed")
	}
	return &nativeSpeech{synthesizer: engine}, nil
}
func (s *nativeSpeech) speak(text string) error {
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(speechNew)
	defer pool.Send(speechDrain)
	s.synthesizer.Send(speechStop, int64(0))
	value := objc.ID(objc.GetClass("NSString")).Send(speechUTF8, text)
	utterance := objc.ID(objc.GetClass("AVSpeechUtterance")).Send(speechUtterance, value)
	if utterance == 0 {
		return errors.New("system speech text could not be prepared")
	}
	s.synthesizer.Send(speechSpeak, utterance)
	return nil
}
func (s *nativeSpeech) stop() error { s.synthesizer.Send(speechStop, int64(0)); return nil }
func (s *nativeSpeech) speaking() (bool, error) {
	return objc.Send[bool](s.synthesizer, speechSpeaking), nil
}
func (s *nativeSpeech) close() error {
	s.stop()
	s.synthesizer.Send(speechRelease)
	s.synthesizer = 0
	return nil
}
