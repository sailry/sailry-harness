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

type subscription struct {
	handle   *handle
	mirror   *myterminal.Mirror
	ticker   *time.Ticker
	closed   chan struct{}
	once     sync.Once
	next     sync.Mutex
	initial  bool
	sequence uint64
	last     protocol.TerminalInfo
}

func (service *Service) Subscribe(id protocol.TerminalID) (link.Subscription, error) {
	value, err := service.get(id)
	if err != nil {
		return nil, err
	}
	value.mu.Lock()
	mirror, err := value.terminal.NewMirror()
	value.mu.Unlock()
	if err != nil {
		return nil, err
	}
	return &subscription{handle: value, mirror: mirror, ticker: time.NewTicker(time.Second / 60), closed: make(chan struct{}), initial: true}, nil
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
		stream.ticker.Stop()
		stream.handle.mu.Lock()
		stream.mirror.Close()
		stream.handle.mu.Unlock()
	})
	return nil
}

func (stream *subscription) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
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
		value := stream.handle
		value.mu.Lock()
		frame, _ := stream.mirror.Frame()
		info, viewport := value.info, value.viewport
		value.mu.Unlock()
		changed := stream.last.Status.Kind != info.Status.Kind || stream.last.Revision != info.Revision || stringPointer(stream.last.Title) != stringPointer(info.Title)
		if stream.initial || len(frame) != 0 || changed {
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
		if info.Status.Kind == "closed" || info.Status.Kind == "stopped" {
			return protocol.Update{}, io.EOF
		}
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.closed:
			return protocol.Update{}, io.EOF
		case <-stream.handle.service.ctx.Done():
			return protocol.Update{}, io.EOF
		case <-stream.ticker.C:
		}
	}
}

func stringPointer(value *string) string {
	if value != nil {
		return *value
	}
	return ""
}
