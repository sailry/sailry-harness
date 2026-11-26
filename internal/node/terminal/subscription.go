package terminal

import (
	"context"
	"io"
	"sync"
	"time"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	terminalActiveInterval = time.Second / 60
	terminalWarmInterval   = 50 * time.Millisecond
	terminalCoolInterval   = 150 * time.Millisecond
	terminalLocalIdle      = 200 * time.Millisecond
	terminalConnIdle       = time.Second
)

func terminalPollInterval(emptyTicks int, hasConn bool) time.Duration {
	switch {
	case emptyTicks < 30:
		return terminalActiveInterval
	case emptyTicks < 45:
		return terminalWarmInterval
	case emptyTicks < 55:
		return terminalCoolInterval
	case hasConn:
		return terminalConnIdle
	default:
		return terminalLocalIdle
	}
}

type subscription struct {
	handle     *handle
	mirror     *myterminal.Mirror
	closed     chan struct{}
	finished   chan struct{}
	wake       chan struct{}
	final      []byte
	once       sync.Once
	next       sync.Mutex
	hasConn    bool
	initial    bool
	emptyTicks int
	sequence   uint64
	last       protocol.TerminalInfo
}

func (stream *subscription) signal() {
	select {
	case stream.wake <- struct{}{}:
	default:
	}
}

func (value *handle) wakeStreamsLocked() {
	for stream := range value.streams {
		stream.signal()
	}
}

func (value *handle) wakeStreams() {
	value.mu.Lock()
	value.wakeStreamsLocked()
	value.mu.Unlock()
}

func (service *Service) Subscribe(id protocol.TerminalID) (link.Subscription, error) {
	value, err := service.get(id)
	if err != nil {
		return nil, err
	}
	value.mu.Lock()
	defer value.mu.Unlock()
	if value.info.Status.Kind != "running" {
		return nil, fault(protocol.ErrorCodeNotFound, "terminal is not running on this Node")
	}
	mirror, err := value.terminal.NewMirror()
	if err != nil {
		return nil, err
	}
	stream := &subscription{
		handle:   value,
		mirror:   mirror,
		closed:   make(chan struct{}),
		finished: make(chan struct{}),
		wake:     make(chan struct{}, 1),
		hasConn:  value.hasConn,
		initial:  true,
	}
	value.streams[stream] = struct{}{}
	return stream, nil
}

// finishLocked preserves the last frame while releasing the stream's native
// mirror. The handle lock also serializes Frame and Close.
func (stream *subscription) finishLocked() {
	stream.final, _ = stream.mirror.Frame()
	stream.mirror.Close()
	stream.mirror = nil
	close(stream.finished)
}

func (service *Service) Snapshot(id protocol.TerminalID) (protocol.TerminalSnapshot, error) {
	stream, err := service.Subscribe(id)
	if err != nil {
		return protocol.TerminalSnapshot{}, err
	}
	defer stream.Close()
	update, err := stream.Next(service.ctx)
	if err != nil {
		return protocol.TerminalSnapshot{}, err
	}
	return update.Data.(protocol.TerminalSnapshot), nil
}

func (stream *subscription) Close() error {
	stream.once.Do(func() {
		close(stream.closed)
		stream.handle.mu.Lock()
		if stream.mirror != nil {
			stream.mirror.Close()
			stream.mirror = nil
		}
		stream.final = nil
		delete(stream.handle.streams, stream)
		stream.handle.mu.Unlock()
	})
	return nil
}

func (stream *subscription) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	var timer *time.Timer
	defer func() {
		if timer != nil {
			timer.Stop()
		}
	}()
	for {
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.closed:
			return protocol.Update{}, io.EOF
		case <-stream.handle.service.ctx.Done():
			return protocol.Update{}, io.EOF
		default:
		}
		select {
		case <-stream.wake:
			stream.emptyTicks = 0
		default:
		}
		value := stream.handle
		value.mu.Lock()
		frame := stream.final
		stream.final = nil
		held := false
		if stream.mirror != nil {
			frame, held = stream.mirror.Frame()
		}
		info, viewport := value.info, value.viewport
		value.mu.Unlock()
		changed := stream.last.Status.Kind != info.Status.Kind || stream.last.Revision != info.Revision || stringPointer(stream.last.Title) != stringPointer(info.Title)
		if stream.initial || len(frame) != 0 || changed {
			stream.emptyTicks = 0
			screen := protocol.DefaultTerminalScreen()
			screen.Columns = viewport.Columns
			screen.VT = &protocol.TerminalVT{Viewport: viewport, Bytes: frame}
			stream.last = info
			if stream.initial {
				stream.initial = false
				return protocol.Update{Kind: "terminal_snapshot", Data: protocol.TerminalSnapshot{Node: value.service.node, Info: info, Sequence: stream.sequence, Screen: screen}}, nil
			}
			stream.sequence++
			return protocol.Update{Kind: "terminal_frame", Data: protocol.TerminalFrame{Node: value.service.node, Info: info, Sequence: stream.sequence, Screen: protocol.TerminalScreenScreenUpdate{Kind: "replace", Data: protocol.TerminalScreenScreenUpdateReplacePayload{Screen: screen}}}}, nil
		}
		if info.Status.Kind != "running" {
			_ = stream.Close()
			return protocol.Update{}, io.EOF
		}
		if held {
			stream.emptyTicks = 0
		} else {
			stream.emptyTicks++
		}
		interval := terminalPollInterval(stream.emptyTicks, stream.hasConn)
		if timer == nil {
			timer = time.NewTimer(interval)
		} else {
			timer.Reset(interval)
		}
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.closed:
			return protocol.Update{}, io.EOF
		case <-stream.handle.service.ctx.Done():
			return protocol.Update{}, io.EOF
		case <-stream.finished:
		case <-stream.wake:
			stream.emptyTicks = 0
		case <-timer.C:
		}
	}
}

func stringPointer(value *string) string {
	if value != nil {
		return *value
	}
	return ""
}
