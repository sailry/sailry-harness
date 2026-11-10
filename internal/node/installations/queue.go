package installations

import (
	"context"
	"encoding/json"
	"reflect"
	"sort"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/tasks"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Report func(protocol.InstallationPhase, *protocol.InstallationProgress)

type key struct {
	caller  protocol.NodeID
	request protocol.RequestID
}

type record struct {
	task     protocol.InstallationTask
	plugin   *protocol.PluginContext
	position uint64
}

type Queue struct {
	owner    *tasks.Supervisor
	node     protocol.NodeID
	closed   <-chan struct{}
	mu       sync.Mutex
	records  map[key]*record
	position uint64
	pending  int
	lanes    map[string]<-chan struct{}
	changed  chan struct{}
}

func New(owner *tasks.Supervisor, node protocol.NodeID, closed <-chan struct{}) *Queue {
	return &Queue{
		owner: owner, node: node, closed: closed, records: map[key]*record{},
		lanes: map[string]<-chan struct{}{}, changed: make(chan struct{}),
	}
}

func lane(resource protocol.InstallationResource) (string, error) {
	switch resource.Kind {
	case "plugin":
		var value protocol.InstallationResourcePluginPayload
		if err := decode(resource.Data, &value); err != nil || value.Name == "" {
			return "", invalid("installation package is required")
		}
		return "plugin:" + value.Name, nil
	case "model":
		var value protocol.InstallationResourceModelPayload
		if err := decode(resource.Data, &value); err != nil {
			return "", invalid("installation model is invalid")
		}
		return "model:" + string(value.Kind), nil
	default:
		return "", invalid("installation resource does not have a preparation lane")
	}
}

func (queue *Queue) Submit(caller protocol.NodeID, request protocol.Request, resource protocol.InstallationResource, observe bool, work func(context.Context, Report) link.Response) error {
	if request.Plugin != nil {
		var captured protocol.PluginContext
		if err := decode(request.Plugin, &captured); err != nil {
			return err
		}
		request.Plugin = &captured
	}
	name, err := lane(resource)
	if err != nil {
		return err
	}
	queue.mu.Lock()
	defer queue.mu.Unlock()
	if queue.pending >= 8 {
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "installation queue is busy"}
	}
	select {
	case <-queue.closed:
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "installation queue is stopped"}
	default:
	}
	previous := queue.lanes[name]
	release := make(chan struct{})
	item := key{caller, request.ID}
	start := make(chan struct{})
	err = queue.owner.Submit(func(ctx context.Context) {
		<-start
		defer close(release)
		if previous != nil {
			<-previous
		}
		report := Report(func(protocol.InstallationPhase, *protocol.InstallationProgress) {})
		if observe {
			report = func(phase protocol.InstallationPhase, progress *protocol.InstallationProgress) {
				queue.report(item, phase, progress)
			}
		}
		result := func() (result link.Response) {
			defer func() {
				if recover() != nil {
					result = link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "installation preparation failed; inspect the resource before retrying"}}
				}
			}()
			return work(ctx, report)
		}()
		queue.mu.Lock()
		queue.pending--
		if queue.lanes[name] == release {
			delete(queue.lanes, name)
		}
		queue.finish(item, result)
		queue.mu.Unlock()
	})
	if err != nil {
		return err
	}
	queue.pending++
	queue.lanes[name] = release
	if observe {
		queue.position++
		queue.records[item] = &record{
			task:   protocol.InstallationTask{Request: request.ID, Revision: 1, Resource: resource, State: protocol.InstallationState{Kind: "queued"}},
			plugin: request.Plugin, position: queue.position,
		}
		queue.publish()
	}
	close(start)
	return nil
}

func (queue *Queue) report(item key, phase protocol.InstallationPhase, progress *protocol.InstallationProgress) {
	if _, err := phase.MarshalJSON(); err != nil {
		return
	}
	if progress != nil {
		switch progress.Kind {
		case "percent":
			var value protocol.InstallationProgressPercentPayload
			if decode(progress.Data, &value) != nil || value.Percent > 100 {
				return
			}
		case "counts":
			var value protocol.InstallationProgressCountsPayload
			if decode(progress.Data, &value) != nil || value.Completed > value.Total {
				return
			}
		default:
			return
		}
	}
	queue.mu.Lock()
	defer queue.mu.Unlock()
	current := queue.records[item]
	if current == nil || terminal(current.task.State) {
		return
	}
	state := protocol.InstallationState{Kind: "running", Data: protocol.InstallationStateRunningPayload{Phase: phase, Progress: progress}}
	if reflect.DeepEqual(current.task.State, state) {
		return
	}
	current.task.Revision++
	current.task.State = state
	queue.publish()
}

func terminal(state protocol.InstallationState) bool {
	return state.Kind == "failed" || state.Kind == "uncertain"
}

func (queue *Queue) finish(item key, result link.Response) {
	current := queue.records[item]
	if current == nil {
		return
	}
	if result.Fault == nil {
		delete(queue.records, item)
	} else {
		current.task.Revision++
		if result.Fault.Code == protocol.ErrorCodeOutcomeUnknown {
			current.task.State = protocol.InstallationState{Kind: "uncertain", Data: protocol.InstallationStateUncertainPayload{Error: *result.Fault}}
		} else {
			current.task.State = protocol.InstallationState{Kind: "failed", Data: protocol.InstallationStateFailedPayload{Error: *result.Fault}}
		}
	}
	queue.publish()
}

func (queue *Queue) publish() {
	close(queue.changed)
	queue.changed = make(chan struct{})
}

func visible(current *record, plugin *protocol.PluginContext) bool {
	return plugin == nil || reflect.DeepEqual(current.plugin, plugin) && (current.task.Resource.Kind == "model" || current.task.Resource.Kind == "language")
}

func (queue *Queue) Read(caller protocol.NodeID, requests []protocol.RequestID, plugin *protocol.PluginContext) ([]protocol.InstallationTask, error) {
	if err := validateRequests(requests); err != nil {
		return nil, err
	}
	queue.mu.Lock()
	defer queue.mu.Unlock()
	return queue.read(caller, requests, plugin, false)
}

func (queue *Queue) read(caller protocol.NodeID, requests []protocol.RequestID, plugin *protocol.PluginContext, all bool) ([]protocol.InstallationTask, error) {
	selected := map[protocol.RequestID]bool{}
	for _, request := range requests {
		selected[request] = true
	}
	var records []*record
	for item, current := range queue.records {
		if item.caller != caller || !all && !selected[item.request] {
			continue
		}
		if !visible(current, plugin) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "installation belongs to another plugin scope"}
		}
		records = append(records, current)
	}
	sort.Slice(records, func(left, right int) bool { return records[left].position < records[right].position })
	result := make([]protocol.InstallationTask, 0, len(records))
	for _, current := range records {
		result = append(result, current.task)
	}
	return result, nil
}

func (queue *Queue) Dismiss(caller protocol.NodeID, request protocol.RequestID, plugin *protocol.PluginContext) error {
	queue.mu.Lock()
	defer queue.mu.Unlock()
	item := key{caller, request}
	current := queue.records[item]
	if current == nil {
		return nil
	}
	if !visible(current, plugin) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "installation belongs to another plugin scope"}
	}
	if !terminal(current.task.State) {
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "installation is still active"}
	}
	delete(queue.records, item)
	queue.publish()
	return nil
}

func (queue *Queue) Subscribe(caller protocol.NodeID) link.Subscription {
	return &subscription{queue: queue, caller: caller, initial: true, closed: make(chan struct{})}
}

type subscription struct {
	queue    *Queue
	caller   protocol.NodeID
	requests []protocol.RequestID
	plugin   *protocol.PluginContext
	initial  bool
	next     sync.Mutex
	once     sync.Once
	closed   chan struct{}
	wake     <-chan struct{}
}

func (stream *subscription) Close() error {
	stream.once.Do(func() { close(stream.closed) })
	return nil
}

func (stream *subscription) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-stream.closed:
		return protocol.Update{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node service is unavailable"}
	case <-stream.queue.closed:
		return protocol.Update{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node service is unavailable"}
	default:
	}
	if !stream.initial {
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.closed:
			return protocol.Update{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node service is unavailable"}
		case <-stream.queue.closed:
			return protocol.Update{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node service is unavailable"}
		case <-stream.wake:
		}
	}
	stream.queue.mu.Lock()
	defer stream.queue.mu.Unlock()
	stream.initial = false
	stream.wake = stream.queue.changed
	requests := stream.requests
	if stream.plugin != nil {
		requests = make([]protocol.RequestID, 0, len(stream.requests))
		for _, request := range stream.requests {
			if current := stream.queue.records[key{caller: stream.caller, request: request}]; current != nil && visible(current, stream.plugin) {
				requests = append(requests, request)
			}
		}
	}
	tasks, err := stream.queue.read(stream.caller, requests, stream.plugin, stream.plugin == nil)
	return protocol.Update{Kind: "installations", Data: protocol.UpdateInstallationsPayload{Node: stream.queue.node, Tasks: tasks}}, err
}

func decode(input, output any) error {
	body, err := json.Marshal(input)
	if err == nil {
		err = json.Unmarshal(body, output)
	}
	return err
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
