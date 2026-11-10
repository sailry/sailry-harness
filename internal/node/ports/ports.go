package ports

import (
	"context"
	"net"
	"strconv"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Options struct {
	Now  func() time.Time
	Dial func(context.Context, string, string) (net.Conn, error)
}

type entry struct {
	caller  protocol.NodeID
	port    uint16
	expires time.Time
	active  bool
	ctx     context.Context
	cancel  context.CancelFunc
}

type Service struct {
	ctx     context.Context
	cancel  context.CancelFunc
	now     func() time.Time
	dial    func(context.Context, string, string) (net.Conn, error)
	mu      sync.Mutex
	entries map[protocol.StreamID]*entry
	closed  bool
	work    sync.WaitGroup
}

func New(ctx context.Context, options Options) *Service {
	ctx, cancel := context.WithCancel(ctx)
	if options.Now == nil {
		options.Now = time.Now
	}
	if options.Dial == nil {
		options.Dial = (&net.Dialer{}).DialContext
	}
	return &Service{ctx: ctx, cancel: cancel, now: options.Now, dial: options.Dial, entries: map[protocol.StreamID]*entry{}}
}

func (service *Service) Prepare(caller protocol.NodeID, port uint16) (protocol.StreamID, error) {
	if port == 0 {
		return protocol.StreamID{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "port must be nonzero"}
	}
	service.mu.Lock()
	defer service.mu.Unlock()
	if service.closed || service.ctx.Err() != nil {
		return protocol.StreamID{}, unavailable()
	}
	now := service.now()
	for id, item := range service.entries {
		if !item.active && !item.expires.After(now) {
			item.cancel()
			delete(service.entries, id)
		}
	}
	if len(service.entries) >= 32 {
		return protocol.StreamID{}, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "port stream capacity exhausted"}
	}
	id := protocol.NewStreamID()
	ctx, cancel := context.WithCancel(service.ctx)
	service.entries[id] = &entry{caller: caller, port: port, expires: now.Add(30 * time.Second), ctx: ctx, cancel: cancel}
	return id, nil
}

func (service *Service) Open(ctx context.Context, caller protocol.NodeID, id protocol.StreamID) (link.Stream, bool, error) {
	service.mu.Lock()
	item := service.entries[id]
	if item == nil {
		service.mu.Unlock()
		return nil, false, nil
	}
	if item.caller != caller {
		service.mu.Unlock()
		return nil, true, denied()
	}
	if service.closed || item.active || item.ctx.Err() != nil || !item.expires.After(service.now()) {
		service.mu.Unlock()
		return nil, true, unavailable()
	}
	item.active = true
	service.work.Add(1)
	service.mu.Unlock()
	release := func() {
		service.mu.Lock()
		delete(service.entries, id)
		service.mu.Unlock()
		item.cancel()
		service.work.Done()
	}
	dialContext, cancel := context.WithTimeout(ctx, 10*time.Second)
	stop := context.AfterFunc(item.ctx, cancel)
	connection, err := service.dial(dialContext, "tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(int(item.port))))
	stop()
	cancel()
	if err != nil {
		release()
		return nil, true, unavailable()
	}
	stream := &portStream{connection: connection, release: release}
	context.AfterFunc(item.ctx, func() { _ = stream.Close() })
	return stream, true, nil
}

func (service *Service) Cancel(caller protocol.NodeID, id protocol.StreamID) error {
	service.mu.Lock()
	item := service.entries[id]
	if item == nil {
		service.mu.Unlock()
		return nil
	}
	if item.caller != caller {
		service.mu.Unlock()
		return denied()
	}
	if !item.active {
		delete(service.entries, id)
	}
	service.mu.Unlock()
	item.cancel()
	return nil
}

func (service *Service) Close() error {
	service.mu.Lock()
	service.closed = true
	service.cancel()
	for id, item := range service.entries {
		if !item.active {
			delete(service.entries, id)
		}
	}
	service.mu.Unlock()
	service.work.Wait()
	return nil
}

type portStream struct {
	connection net.Conn
	release    func()
	close      sync.Once
	err        error
}

func (stream *portStream) Read(data []byte) (int, error)  { return stream.connection.Read(data) }
func (stream *portStream) Write(data []byte) (int, error) { return stream.connection.Write(data) }
func (stream *portStream) CloseWrite() error {
	return stream.connection.(interface{ CloseWrite() error }).CloseWrite()
}
func (stream *portStream) Close() error {
	stream.close.Do(func() {
		stream.err = stream.connection.Close()
		stream.release()
	})
	return stream.err
}

func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "execution port is unavailable"}
}
func denied() error {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "port stream belongs to another controller"}
}
