package mobile

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type connection struct {
	client     *client.Client
	controller *controller
}

func (owner *connection) invoke(ctx context.Context, runtime *Runtime, handle uint64, method string, data json.RawMessage) (any, error) {
	var values input
	if err := arguments(data, &values); err != nil {
		return nil, err
	}
	switch method {
	case "prepare":
		var command protocol.Command
		if err := json.Unmarshal([]byte(values.Command), &command); err != nil {
			return nil, invalid("invalid command")
		}
		return encode(owner.client.Prepare(command))
	case "execute", "resolve":
		// execute sends a request once and looks an unobserved result up;
		// resolve only asks what became of a request sent earlier. "released"
		// marks a failure that ends the request: nothing more can be learned
		// about it, so the caller drops it and builds a new one.
		var request protocol.Request
		var output protocol.Output
		var released bool
		var err error
		if err = json.Unmarshal([]byte(values.Request), &request); err != nil {
			err = invalid("invalid command request")
		} else if method == "execute" {
			output, err = owner.client.Settle(ctx, request)
			released = client.Released(err)
		} else {
			output, released, err = owner.resolve(ctx, request)
		}
		if err != nil {
			failed := result(nil, err).Error
			if errors.Is(err, context.Canceled) {
				failed = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "connection closed; retain the request identifier"}
			}
			if released {
				return encode(map[string]any{"Err": failed, "released": true})
			}
			return encode(map[string]any{"Err": failed})
		}
		return encode(map[string]any{"Ok": output})
	case "prepare_status":
		request, err := owner.client.PrepareStatusJSON(json.RawMessage(values.Scope), json.RawMessage(values.Entry))
		if err != nil {
			return nil, err
		}
		return encode(request)
	case "search_conversation":
		session, err := protocol.ParseSessionID(values.Session)
		if err != nil {
			return nil, invalid("invalid session identifier")
		}
		var query protocol.ConversationSearchQuery
		if err := json.Unmarshal([]byte(values.Query), &query); err != nil {
			return nil, invalid("invalid conversation search query")
		}
		found, err := owner.client.SearchConversation(ctx, session, query)
		if err != nil {
			return encode(map[string]any{"Err": result(nil, err).Error})
		}
		return encode(map[string]any{"Ok": found})
	case "forward_port", "forward_service":
		lifetime, cancel, detach := runtime.lifetime(ctx, handle)
		var forwarder *client.Forwarder
		var err error
		if method == "forward_port" {
			forwarder, err = owner.client.ForwardPort(lifetime, values.RemotePort, values.LocalPort)
		} else {
			forwarder, err = owner.client.ForwardService(lifetime, json.RawMessage(values.Source), values.LocalPort)
		}
		detach()
		if err != nil {
			cancel()
			return nil, err
		}
		return runtime.add(handle, "forwarding", &forwarding{forwarder: forwarder}, lifetime, cancel)
	case "upload_attachment", "download_attachment", "download_file":
		return owner.transfer(ctx, runtime, handle, method, values)
	case "watch", "watch_commands", "watch_connections", "watch_connector_login", "watch_conversation", "watch_login", "watch_status", "watch_terminal", "watch_usage":
		var argument json.RawMessage = json.RawMessage("null")
		switch method {
		case "watch_commands", "watch_conversation":
			argument, _ = json.Marshal(values.Session)
		case "watch_terminal":
			argument, _ = json.Marshal(values.Terminal)
		case "watch_status":
			argument = json.RawMessage(values.Scope)
		case "watch_usage":
			argument = json.RawMessage(values.Query)
		case "watch_login", "watch_connector_login":
			argument = json.RawMessage(values.Attempt)
		}
		lifetime, cancel, detach := runtime.lifetime(ctx, handle)
		observer, err := owner.client.Watch(lifetime, method, argument)
		detach()
		if err != nil {
			cancel()
			return nil, err
		}
		kind := map[string]string{
			"watch": "updates", "watch_commands": "command_updates",
			"watch_connections": "connections_updates", "watch_connector_login": "login_updates",
			"watch_conversation": "conversation_updates", "watch_login": "login_updates",
			"watch_status": "status_updates", "watch_terminal": "terminal_updates",
			"watch_usage": "usage_updates",
		}[method]
		return runtime.add(handle, kind, &observation{observer: observer, connection: owner, notices: method == "watch"}, lifetime, cancel)
	}
	return nil, invalid("unknown connection operation")
}

// resolve reports what became of a request whose result was not observed,
// without running it: its stored output or fault, a released failure when it
// never reached the Node or the Node can never tell, and OutcomeUnknown while
// it still runs or the Node cannot be asked.
func (owner *connection) resolve(ctx context.Context, request protocol.Request) (protocol.Output, bool, error) {
	if !request.Command.Durable() {
		// The Node keeps no outcome for it, so only a new request can answer.
		return protocol.Output{}, true, unavailable("the Node keeps no outcome for this request")
	}
	resolution, err := owner.client.Resolve(ctx, request)
	if err != nil {
		return protocol.Output{}, false, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node could not be asked; retain the request identifier"}
	}
	switch resolution.State {
	case client.RequestApplied:
		return resolution.Output, false, nil
	case client.RequestFailed:
		return protocol.Output{}, false, resolution.Fault
	case client.RequestAbsent:
		return protocol.Output{}, true, unavailable("the request did not reach the Node; nothing was changed")
	case client.RequestUnknown:
		return protocol.Output{}, true, resolution.Fault
	}
	return protocol.Output{}, false, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the request is still running; retain the request identifier"}
}

func (owner *connection) close() error {
	return owner.client.Close()
}

type observation struct {
	observer   *client.Observer
	connection *connection
	clients    []*client.Client
	notices    bool
}

func (owner *observation) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, data json.RawMessage) (any, error) {
	switch method {
	case "next":
		view, err := owner.observer.Next(ctx)
		if err != nil {
			return nil, err
		}
		if owner.notices && owner.connection != nil && owner.connection.controller != nil {
			var state struct {
				Snapshot *json.RawMessage `json:"snapshot"`
			}
			if err := json.Unmarshal(view, &state); err != nil {
				return nil, err
			}
			if state.Snapshot != nil {
				owner.connection.controller.inbox.Merge(owner.connection.client.Target(), owner.connection.client.Notices())
			}
		}
		return string(view), nil
	case "load_older", "load_through", "track_tools", "refresh":
		var values struct {
			Sequence  string `json:"sequence"`
			Selection string `json:"selection"`
		}
		if err := arguments(data, &values); err != nil {
			return nil, err
		}
		var argument json.RawMessage = json.RawMessage("null")
		if method == "load_through" {
			argument, _ = json.Marshal(values.Sequence)
		}
		if method == "track_tools" {
			argument = json.RawMessage(values.Selection)
		}
		return nil, owner.observer.Control(ctx, method, argument)
	}
	return nil, invalid("unknown observation operation")
}

func (owner *observation) close() error {
	err := owner.observer.Close()
	for _, shared := range owner.clients {
		err = errors.Join(err, shared.Close())
	}
	return err
}

type forwarding struct {
	forwarder *client.Forwarder
}

func (owner *forwarding) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, _ json.RawMessage) (any, error) {
	switch method {
	case "local_port":
		return owner.forwarder.LocalPort(), nil
	case "next":
		state, err := owner.forwarder.Next(ctx)
		if err != nil {
			return nil, err
		}
		return string(state), nil
	}
	return nil, invalid("unknown forwarding operation")
}

func (owner *forwarding) close() error {
	return owner.forwarder.Close()
}
