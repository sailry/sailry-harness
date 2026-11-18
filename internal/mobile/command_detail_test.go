package mobile

import (
	"context"
	"encoding/json"
	"strconv"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandDetailTransport struct {
	noticeTransport
	page     protocol.ToolDetailPage
	requests chan protocol.Request
}

func (transport commandDetailTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	transport.requests <- request
	switch request.Command.Kind {
	case "read_tool_detail":
		completion := make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{Kind: "tool_detail", Data: transport.page}}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	case "read_command":
		<-ctx.Done()
		return link.Admission{}, ctx.Err()
	}
	return link.Admission{}, invalid("unexpected process operation")
}

func TestMobileHistoricalCommandDetailRelease(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ref := client.HistoricalCommandReference{
			Detail:  protocol.ToolDetailReference{Session: protocol.SessionID{1}, Turn: protocol.TurnID{2}, Entry: "launch-result", Revision: strings.Repeat("a", 64)},
			Command: protocol.RequestID{3}, Session: protocol.SessionID{4},
		}
		encode := func(value any) []byte {
			t.Helper()
			data, err := json.Marshal(value)
			if err != nil {
				t.Fatal(err)
			}
			return data
		}
		detail := protocol.DefaultToolDetail()
		detail.Result = new(json.RawMessage(encode(map[string]any{"kind": "command_started", "data": map[string]any{"session": ref.Session, "id": ref.Command}})))
		body := encode(detail)
		transport := commandDetailTransport{
			noticeTransport: noticeTransport{node: protocol.NodeID{1}},
			page:            protocol.ToolDetailPage{Reference: ref.Detail, Text: string(body), TotalBytes: uint64(len(body))},
			requests:        make(chan protocol.Request, 4),
		}
		answers := make(chan struct {
			id uint64
			envelope
		}, 4)
		runtime := New(func(id uint64, data []byte) {
			var value envelope
			if err := json.Unmarshal(data, &value); err != nil {
				t.Errorf("decode bridge result: %v", err)
			}
			answers <- struct {
				id uint64
				envelope
			}{id, value}
		})
		defer runtime.Close(5)
		owner := &connection{client: client.New(transport)}
		lifetime, cancel := context.WithCancel(runtime.ctx)
		parent, err := runtime.add(0, "connection", owner, lifetime, cancel)
		if err != nil {
			t.Fatal(err)
		}
		number := func(handle string) uint64 {
			t.Helper()
			value, err := strconv.ParseUint(handle, 10, 64)
			if err != nil {
				t.Fatal(err)
			}
			return value
		}
		if !runtime.Invoke(number(parent.Handle), "open_command_detail", encode(map[string]string{"query": string(encode(ref))}), 1) {
			t.Fatal("detail open was not accepted")
		}
		opened := <-answers
		var handle Handle
		if opened.id != 1 || !opened.OK || json.Unmarshal(encode(opened.Value), &handle) != nil || handle.Kind != "command_detail" {
			t.Fatalf("detail did not open a scoped handle: %+v", opened)
		}
		if len(transport.requests) != 0 {
			t.Fatal("opening a handle sent a command before its content was read")
		}
		id := number(handle.Handle)
		if !runtime.Invoke(id, "read", nil, 2) {
			t.Fatal("detail read was not accepted")
		}
		canonical, process := <-transport.requests, <-transport.requests
		query, ok := process.Command.Data.(protocol.CommandReadCommandPayload)
		if canonical.Command.Kind != "read_tool_detail" || process.Command.Kind != "read_command" || !ok || query.Session != ref.Session || query.ID != ref.Command {
			t.Fatalf("historical detail lost the original binding: %v; %+v", canonical.Command.Kind, process.Command)
		}
		if !runtime.Invoke(id, "dispose", nil, 3) {
			t.Fatal("release was not accepted")
		}
		read, released := <-answers, <-answers
		if read.id != 2 || read.OK || read.Error == nil || read.Error.Code != protocol.ErrorCodeCancelled || released.id != 3 || !released.OK {
			t.Fatalf("release did not cancel and wait for the process read: %+v; %+v", read, released)
		}
		if len(transport.requests) != 0 {
			t.Fatal("releasing a detail sent another request")
		}
	})
}
