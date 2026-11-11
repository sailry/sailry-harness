// Package reader owns application-wide read-aloud on the controller's UI
// thread: an assigned speech model's voice where one is usable, the OS
// speech otherwise.
package reader

import (
	"context"
	"errors"
	"log"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Synthesizer reads text aloud with a model and returns the audio's media
// type and bytes, as Client.SynthesizeSpeech does. A nil voice is the
// Node's default.
type Synthesizer func(ctx context.Context, text string, voice *string) (string, []byte, error)

// maxSpeechCharacters is the most text one synthesis request reads, the
// bound the Node's speech command enforces.
const maxSpeechCharacters = 4096

// voice is the OS speech as the service drives it.
type voice struct {
	speak    func(string) error
	stop     func() error
	speaking func() (bool, error)
	close    func() error
}

type Service struct {
	engine  *voice
	current string
	monitor context.CancelFunc
	update  func(func())
	changed func()
	closed  bool
	// synthesize is the model voice, nil for OS speech only; play plays one
	// clip to its end or until its context ends; open starts the OS speech.
	synthesize Synthesizer
	play       func(context.Context, []byte) error
	open       func() (*voice, error)
}

// New takes application-thread scheduling that remains usable without an open
// window. Speech is initialized lazily; creating the service emits no audio.
func New(update func(func()), changed func()) *Service {
	return &Service{update: update, changed: changed, play: playClip, open: systemVoice}
}

// SetSynthesizer makes read-aloud use a model's voice. Text the model does
// not read, because no speech model is assigned or the request or its
// playback failed, is read by the OS speech instead.
func (s *Service) SetSynthesizer(synthesize Synthesizer) { s.synthesize = synthesize }

func systemVoice() (*voice, error) {
	engine, err := newNativeSpeech()
	if err != nil {
		return nil, err
	}
	return &voice{speak: engine.speak, stop: engine.stop, speaking: engine.speaking, close: engine.close}, nil
}

func (s *Service) Active(id string) bool { return !s.closed && s.current == id && id != "" }

// Toggle reads markdown aloud as id, or stops id when it is being read.
// failed reports, on the UI thread, OS speech that could not start after
// the model voice gave way to it; an error returned is one found at once.
func (s *Service) Toggle(id, markdown string, failed func()) error {
	if s.closed {
		return context.Canceled
	}
	if s.Active(id) {
		return s.Stop()
	}
	text := Text(markdown)
	if strings.TrimSpace(text) == "" {
		return nil
	}
	if s.synthesize == nil {
		return s.speak(id, text)
	}
	if err := s.halt(); err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.monitor, s.current = cancel, id
	if s.changed != nil {
		s.changed()
	}
	go s.narrate(ctx, id, Chunks(text, maxSpeechCharacters), failed)
	return nil
}

// speak reads text with the OS speech and follows it to its end.
func (s *Service) speak(id, text string) error {
	if s.engine == nil {
		engine, err := s.open()
		if err != nil {
			return err
		}
		s.engine = engine
	}
	if err := s.engine.speak(text); err != nil {
		return err
	}
	if s.monitor != nil {
		s.monitor()
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.monitor = cancel
	s.current = id
	if s.changed != nil {
		s.changed()
	}
	go s.watch(ctx, id)
	return nil
}

// narrate reads the parts with the model voice, one clip after another,
// asking for the next while one plays. The first part the model does not
// read, and every part after it, goes to the OS speech: silently, since
// the text is still heard. Canceling ctx ends the request under way and
// the clip playing.
func (s *Service) narrate(ctx context.Context, id string, parts []string, failed func()) {
	type clip struct {
		audio []byte
		err   error
	}
	clips := make(chan clip, 1)
	go func() {
		defer close(clips)
		for _, part := range parts {
			_, audio, err := s.synthesize(ctx, part, nil)
			select {
			case clips <- clip{audio, err}:
			case <-ctx.Done():
				return
			}
			if err != nil {
				return
			}
		}
	}()
	for index := 0; ; index++ {
		next, more := <-clips
		if ctx.Err() != nil {
			return
		}
		if !more {
			s.update(func() { s.finish(ctx, id) })
			return
		}
		err := next.err
		if err == nil {
			err = s.play(ctx, next.audio)
		}
		if ctx.Err() != nil {
			return
		}
		if err != nil {
			// No assigned speech model is the ordinary case, not a failure.
			var fault *link.FaultError
			if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodeNotConfigured {
				log.Printf("read aloud: model voice failed, using system speech: %v", err)
			}
			rest := strings.Join(parts[index:], " ")
			s.update(func() {
				if s.closed || s.current != id || ctx.Err() != nil {
					return
				}
				if err := s.speak(id, rest); err != nil {
					log.Printf("read aloud: system speech failed: %v", err)
					s.finish(ctx, id)
					if failed != nil {
						failed()
					}
				}
			})
			return
		}
	}
}

// finish ends the reading ctx belongs to, unless another replaced it.
func (s *Service) finish(ctx context.Context, id string) {
	if s.closed || s.current != id || ctx.Err() != nil {
		return
	}
	s.monitor()
	s.monitor, s.current = nil, ""
	if s.changed != nil {
		s.changed()
	}
}

// halt stops what is being read: the OS speech, and the model voice's
// pending request and playback.
func (s *Service) halt() error {
	if s.engine != nil {
		if err := s.engine.stop(); err != nil {
			return err
		}
	}
	if s.monitor != nil {
		s.monitor()
		s.monitor = nil
	}
	return nil
}

func (s *Service) Stop() error {
	if err := s.halt(); err != nil {
		return err
	}
	s.current = ""
	if s.changed != nil {
		s.changed()
	}
	return nil
}
func (s *Service) Close() error {
	if s.closed {
		return nil
	}
	s.closed = true
	if s.monitor != nil {
		s.monitor()
		s.monitor = nil
	}
	s.current = ""
	if s.engine == nil {
		return nil
	}
	return s.engine.close()
}
func (s *Service) watch(ctx context.Context, id string) {
	for {
		timer := time.NewTimer(250 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
		done := make(chan bool, 1)
		s.update(func() {
			if s.closed || s.current != id || ctx.Err() != nil {
				done <- true
				return
			}
			speaking, err := s.engine.speaking()
			if err != nil || !speaking {
				s.current = ""
				s.monitor = nil
				if s.changed != nil {
					s.changed()
				}
				done <- true
				return
			}
			done <- false
		})
		select {
		case <-ctx.Done():
			return
		case stopped := <-done:
			if stopped {
				return
			}
		}
	}
}
