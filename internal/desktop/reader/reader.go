// Package reader owns application-wide OS speech on the controller's UI thread.
package reader

import (
	"context"
	"strings"
	"time"
)

type Service struct {
	engine  *nativeSpeech
	current string
	monitor context.CancelFunc
	update  func(func())
	changed func()
	closed  bool
}

// New takes application-thread scheduling that remains usable without an open
// window. Speech is initialized lazily; creating the service emits no audio.
func New(update func(func()), changed func()) *Service {
	return &Service{update: update, changed: changed}
}
func (s *Service) Active(id string) bool { return !s.closed && s.current == id && id != "" }
func (s *Service) Toggle(id, markdown string) error {
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
	if s.engine == nil {
		engine, err := newNativeSpeech()
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
func (s *Service) Stop() error {
	if s.engine != nil {
		if err := s.engine.stop(); err != nil {
			return err
		}
	}
	if s.monitor != nil {
		s.monitor()
		s.monitor = nil
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
