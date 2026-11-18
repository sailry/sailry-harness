package mobile

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandDetail struct {
	client    *client.Client
	reference client.HistoricalCommandReference
}

func (owner *connection) openCommandDetail(ctx context.Context, runtime *Runtime, handle uint64, query string) (Handle, error) {
	var ref client.HistoricalCommandReference
	if json.Unmarshal([]byte(query), &ref) != nil || ref.Detail.Session == (protocol.SessionID{}) || ref.Detail.Entry == "" ||
		ref.Detail.Revision == "" || ref.Command == (protocol.RequestID{}) || ref.Session == (protocol.SessionID{}) {
		return Handle{}, invalid("invalid background command detail reference")
	}
	lifetime, cancel, detach := runtime.lifetime(ctx, handle)
	detach()
	return runtime.add(handle, "command_detail", &commandDetail{owner.client, ref}, lifetime, cancel)
}

func (owner *commandDetail) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, _ json.RawMessage) (any, error) {
	if method != "read" {
		return nil, invalid("unknown command detail operation")
	}
	value, err := owner.client.ReadHistoricalCommand(ctx, owner.reference)
	if err != nil {
		return nil, err
	}
	return encode(value)
}

func (*commandDetail) close() error { return nil }
