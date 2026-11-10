// Package power holds one serialized native idle-sleep request controlled by
// the Node's power setting.
package power

import (
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Backend is the system's idle-sleep request. Acquire and Release return the
// system's error code, 0 for success.
type Backend interface {
	Supported() bool
	Held() bool
	Acquire() int64
	Release() int64
}

type unsupported struct{}

// Unsupported is the backend of a system without an idle-sleep request.
func Unsupported() Backend { return unsupported{} }

func (unsupported) Supported() bool { return false }
func (unsupported) Held() bool      { return false }
func (unsupported) Acquire() int64  { return 0 }
func (unsupported) Release() int64  { return 0 }

// Status is what the service holds now.
type Status struct {
	Enabled   bool
	Acquired  bool
	Supported bool
	Closed    bool
	Error     int64 // the first system error, 0 for none
}

// Service keeps the system awake while the setting is on and the Node runs.
type Service struct {
	mu            sync.Mutex
	settings      protocol.PowerSettings
	status        Status
	backend       Backend
	releaseFailed bool
	failed        func(int64)
}

// New creates the service over the system's request; failed receives the
// first system error.
func New(failed func(int64)) *Service { return WithBackend(native(), failed) }

// WithBackend creates the service over backend.
func WithBackend(backend Backend, failed func(int64)) *Service {
	return &Service{settings: protocol.DefaultPowerSettings(), backend: backend, failed: failed,
		status: Status{Enabled: true, Supported: backend.Supported()}}
}

// Apply takes the saved settings. Replayed durable results and delayed
// readers cannot restore an older policy.
func (s *Service) Apply(settings protocol.PowerSettings) {
	s.update(func() {
		if settings.Revision >= s.settings.Revision {
			s.status.Enabled = settings.PreventSleep
			s.settings = settings
		}
	})
}

// Status reports what the service holds.
func (s *Service) Status() Status {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.status
}

// Close releases the request for good.
func (s *Service) Close() { s.update(func() { s.status.Closed = true }) }

func (s *Service) update(change func()) {
	s.mu.Lock()
	change()
	needed := s.status.Enabled && !s.status.Closed
	var code int64
	switch {
	case !needed && s.status.Acquired && !s.releaseFailed:
		code = s.backend.Release()
		s.releaseFailed = code != 0
	case needed && !s.status.Acquired && s.status.Supported && s.status.Error == 0:
		code = s.backend.Acquire()
	}
	s.status.Acquired = s.backend.Held()
	first := code != 0 && s.status.Error == 0
	if first {
		s.status.Error = code
	}
	s.mu.Unlock()
	// Observers never run under the service lock.
	if first && s.failed != nil {
		s.failed(code)
	}
}
