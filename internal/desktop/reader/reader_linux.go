//go:build linux

package reader

import (
	"errors"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

type speechDispatcher struct {
	open   func(string, string, uintptr, int32) *speechCallbacks
	close  func(uintptr)
	say    func(uintptr, int32, string) int32
	cancel func(uintptr) int32
	notify func(uintptr, int32) int32
}

var loadDispatcher = sync.OnceValues(func() (*speechDispatcher, error) {
	handle, err := purego.Dlopen("libspeechd.so.2", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	api := &speechDispatcher{}
	for _, entry := range []struct {
		target any
		name   string
	}{{&api.open, "spd_open"}, {&api.close, "spd_close"}, {&api.say, "spd_say"}, {&api.cancel, "spd_cancel"}, {&api.notify, "spd_set_notification_on"}} {
		address, err := purego.Dlsym(handle, entry.name)
		if err != nil {
			return nil, err
		}
		purego.RegisterFunc(entry.target, address)
	}
	return api, nil
})

// The public ABI begins with six callback pointers; private connection layout
// is deliberately never read or copied. spd_open returns this system-owned
// connection as a typed pointer, so its address is never rebuilt from an integer.
type speechCallbacks struct{ begin, end, cancel, pause, resume, index uintptr }
type nativeSpeech struct {
	api        *speechDispatcher
	connection uintptr
	callback   uintptr
	mu         sync.Mutex
	active     int32
	pending    bool
	finished   map[uintptr]bool
}

func newNativeSpeech() (*nativeSpeech, error) {
	api, err := loadDispatcher()
	if err != nil {
		return nil, err
	}
	callbacks := api.open("Sailry", "reader", 0, 1)
	if callbacks == nil {
		return nil, errors.New("system speech connection failed")
	}
	connection := uintptr(unsafe.Pointer(callbacks))
	engine := &nativeSpeech{api: api, connection: connection, finished: map[uintptr]bool{}}
	engine.callback = purego.NewCallback(func(message, client uintptr, state int32) {
		engine.mu.Lock()
		defer engine.mu.Unlock()
		if engine.pending {
			engine.finished[message] = true
		} else if uintptr(engine.active) == message {
			engine.active = 0
		}
	})
	callbacks.end = engine.callback
	callbacks.cancel = engine.callback
	if api.notify(connection, 2|8) < 0 {
		api.close(connection)
		return nil, errors.New("system speech notifications failed")
	}
	return engine, nil
}
func (s *nativeSpeech) speak(text string) error {
	if err := s.stop(); err != nil {
		return err
	}
	s.mu.Lock()
	s.pending = true
	clear(s.finished)
	s.mu.Unlock()
	message := s.api.say(s.connection, 2, text)
	s.mu.Lock()
	s.pending = false
	s.active = message
	if message < 0 || s.finished[uintptr(message)] {
		s.active = 0
	}
	clear(s.finished)
	s.mu.Unlock()
	if message < 0 {
		return errors.New("system speech request failed")
	}
	return nil
}
func (s *nativeSpeech) stop() error {
	if s.api.cancel(s.connection) < 0 {
		return errors.New("system speech cancellation failed")
	}
	s.mu.Lock()
	s.active = 0
	s.pending = false
	clear(s.finished)
	s.mu.Unlock()
	return nil
}
func (s *nativeSpeech) speaking() (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.pending || s.active > 0, nil
}
func (s *nativeSpeech) close() error {
	err := s.stop()
	s.api.close(s.connection)
	s.connection = 0
	return err
}
