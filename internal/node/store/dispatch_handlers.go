package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"sort"
	"strconv"
	"strings"

	"github.com/kaptinlin/jsonpointer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func saveDispatchHandler(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, handler protocol.DispatchHandler) (protocol.DispatchHandler, error) {
	for _, name := range []string{handler.Name, handler.Source.Package, handler.Source.Topic, handler.Queue} {
		if err := dispatchName(name); err != nil {
			return protocol.DispatchHandler{}, err
		}
	}
	if err := dispatchData(handler.Callback); err != nil {
		return protocol.DispatchHandler{}, err
	}
	if handler.Callback.Command == nil {
		return protocol.DispatchHandler{}, invalid("dispatch callback command is missing")
	}
	if handler.Callback.Command.Kind == "dispatch" {
		return protocol.DispatchHandler{}, invalid("dispatch callbacks cannot recursively schedule dispatch commands")
	}
	if len(handler.Callback.Bindings) > 32 {
		return protocol.DispatchHandler{}, invalid("too many event bindings")
	}
	command, err := dispatchDocument(handler.Callback.Command)
	if err != nil {
		return protocol.DispatchHandler{}, err
	}
	for target, source := range handler.Callback.Bindings {
		if !strings.HasPrefix(target, "/data/") || !strings.HasPrefix(source, "/") || len(source) > 512 || len(target) > 512 {
			return protocol.DispatchHandler{}, invalid("event binding must point to an existing command data field")
		}
		if _, err := jsonpointer.Value(command, target); err != nil {
			return protocol.DispatchHandler{}, invalid("event binding must point to an existing command data field")
		}
	}
	previous, err := readBody[protocol.DispatchHandler](ctx, db, "SELECT body FROM dispatch_handlers WHERE package=?1 AND name=?2", reference.Name, handler.Name)
	if err != nil {
		return protocol.DispatchHandler{}, err
	}
	var revision uint64
	if previous != nil {
		revision = previous.Revision
	}
	if err := checkRevision(revision, handler.Revision); err != nil {
		return protocol.DispatchHandler{}, err
	}
	if previous == nil {
		if err := dispatchCapacity(ctx, db, "dispatch_handlers", reference.Name); err != nil {
			return protocol.DispatchHandler{}, err
		}
	}
	handler.Revision, err = nextRevision(handler.Revision)
	if err != nil {
		return protocol.DispatchHandler{}, err
	}
	if err := dispatchBudget(ctx, db, "dispatch_handlers", reference.Name, "name", handler.Name, handler); err != nil {
		return protocol.DispatchHandler{}, err
	}
	body, err := encode(handler)
	if err != nil {
		return protocol.DispatchHandler{}, err
	}
	capture, err := encode(reference)
	if err != nil {
		return protocol.DispatchHandler{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO dispatch_handlers(package,name,source,topic,body,reference) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(package,name) DO UPDATE SET source=excluded.source,topic=excluded.topic,body=excluded.body,reference=excluded.reference",
		reference.Name, handler.Name, handler.Source.Package, handler.Source.Topic, body, capture)
	return handler, err
}

func dispatchRequest(node protocol.NodeID, job protocol.DispatchJob, callback protocol.DispatchCallback, reference *protocol.PluginReference) (protocol.Request, error) {
	event, err := dispatchDocument(job.Event)
	if err != nil {
		return protocol.Request{}, err
	}
	command, err := dispatchDocument(callback.Command)
	if err != nil {
		return protocol.Request{}, err
	}
	targets := make([]string, 0, len(callback.Bindings))
	for target := range callback.Bindings {
		targets = append(targets, target)
	}
	sort.Strings(targets)
	for _, target := range targets {
		source := callback.Bindings[target]
		value, err := jsonpointer.Value(event, source)
		if err != nil {
			return protocol.Request{}, invalid("event field required by callback is missing")
		}
		value, err = dispatchDocument(value)
		if err != nil {
			return protocol.Request{}, err
		}
		field, err := jsonpointer.ReferenceOf(command, target)
		if err != nil {
			return protocol.Request{}, invalid("callback command field is missing")
		}
		parent, ok := field.Parent()
		if !ok {
			return protocol.Request{}, invalid("callback command field is missing")
		}
		switch container := parent.(type) {
		case map[string]any:
			container[field.Token()] = value
		case []any:
			index, err := strconv.Atoi(field.Token())
			if err != nil {
				return protocol.Request{}, invalid("callback command field is missing")
			}
			container[index] = value
		default:
			return protocol.Request{}, invalid("callback command field is missing")
		}
	}
	body, err := encode(command)
	if err != nil {
		return protocol.Request{}, err
	}
	var action protocol.Command
	if err := json.Unmarshal(body, &action); err != nil {
		return protocol.Request{}, invalid("event data does not match the callback command")
	}
	request := protocol.NewRequest(node, action)
	request.ID = job.Request
	if reference != nil {
		authority := protocol.DefaultPluginContext()
		authority.Package, authority.Worktree, authority.Session = *reference, callback.Scope.Worktree, callback.Scope.Session
		request.Plugin = &authority
	}
	return request, nil
}

func dispatchDocument(value any) (any, error) {
	body, err := encode(value)
	if err != nil {
		return nil, err
	}
	var document any
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	err = decoder.Decode(&document)
	return document, err
}
