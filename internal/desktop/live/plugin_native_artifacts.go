package live

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeArtifact struct {
	revision *uint32
	cursor   string
	content  *protocol.ArtifactContent
	pending  bool
	err      error
}

type pluginNativeArtifactWatch struct {
	started sync.Once
	mu      sync.Mutex
	cursor  string
	changed chan struct{}
	err     error
}

func (state *pluginNativeState) artifactOperations() {
	operations := state.native.Operations
	operations["sailry/artifacts.artifactResource"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if state.resource == nil || state.resource.artifact == nil {
			return json.RawMessage("null"), nil
		}
		artifact, err := state.artifactSource(state.resource.artifact.ID)
		if err != nil {
			return nil, err
		}
		value := map[string]any{"id": artifact.ID, "session": state.resource.session}
		if state.resource.session != artifact.Session {
			value["revision"] = artifact.Revision
		}
		return pluginNativeJSON(value)
	}
	operations["sailry/artifacts.readArtifact"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		id, err := pluginNativeArgument[protocol.ArtifactID](arguments, 0)
		if err != nil {
			return nil, err
		}
		revision, err := pluginNativeRevision(arguments, 1)
		if err != nil {
			return nil, err
		}
		value, err := state.readArtifact(ctx, id, revision)
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(value)
	}
	operations["sailry/artifacts.listArtifactRevisions"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		id, err := pluginNativeArgument[protocol.ArtifactID](arguments, 0)
		if err != nil {
			return nil, err
		}
		artifact, err := state.artifactSource(id)
		if err != nil {
			return nil, err
		}
		before, err := pluginNativeRevision(arguments, 1)
		if err != nil {
			return nil, err
		}
		output, err := state.execute(ctx, protocol.Command{Kind: "list_artifact_revisions", Data: protocol.CommandListArtifactRevisionsPayload{
			ID: id, Before: before, Limit: 64, Session: &artifact.Session,
		}})
		if err != nil {
			return nil, err
		}
		if output.Kind != "artifact_revisions" {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "unexpected artifact revisions response")
		}
		return pluginNativeJSON(output.Data)
	}
	operations["sailry/artifacts.nextArtifactChange"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if state.resource == nil || state.resource.artifact == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "no artifact surface")
		}
		artifact, err := state.artifactSource(state.resource.artifact.ID)
		if err != nil {
			return nil, err
		}
		seen := ""
		if len(arguments) > 0 {
			seen, err = pluginNativeArgument[string](arguments, 0)
			if err != nil {
				return nil, err
			}
		}
		if _, err := state.execute(ctx, protocol.Command{Kind: "list_artifact_revisions", Data: protocol.CommandListArtifactRevisionsPayload{
			ID: artifact.ID, Limit: 1, Session: &artifact.Session,
		}}); err != nil {
			return nil, err
		}
		state.startArtifactWatch()
		watch := state.artifactWatch
		for {
			watch.mu.Lock()
			cursor, changed, failure := watch.cursor, watch.changed, watch.err
			watch.mu.Unlock()
			if failure != nil {
				return nil, failure
			}
			if cursor != "" && cursor != seen {
				return pluginNativeJSON(map[string]any{"cursor": cursor, "connected": state.runtime.controller.Connected()})
			}
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-state.native.Context.Done():
				return nil, state.native.Context.Err()
			case <-changed:
			}
		}
	}
}

func (state *pluginNativeState) artifactSource(id protocol.ArtifactID) (protocol.ArtifactSummary, error) {
	if state.native.Context.Err() != nil || state.resource == nil || state.resource.artifact == nil ||
		state.resource.artifact.ID != id || state.scope.Session == nil || state.resource.session != *state.scope.Session {
		return protocol.ArtifactSummary{}, pluginNativeFault(protocol.ErrorCodePermissionDenied, "artifact is outside the captured surface")
	}
	return *state.resource.artifact, nil
}

func (state *pluginNativeState) readArtifact(ctx context.Context, id protocol.ArtifactID, revision *uint32) (protocol.ArtifactContent, error) {
	artifact, err := state.artifactSource(id)
	if err != nil {
		return protocol.ArtifactContent{}, err
	}
	output, err := state.execute(ctx, protocol.Command{Kind: "read_artifact", Data: protocol.CommandReadArtifactPayload{ID: id, Revision: revision, Session: &artifact.Session}})
	if err != nil {
		return protocol.ArtifactContent{}, err
	}
	value, valid := detail[protocol.ArtifactContent](output.Data)
	if !valid || output.Kind != "artifact" || value.Artifact.ID != id || value.Artifact.Session != artifact.Session ||
		revision != nil && value.Artifact.Revision != *revision {
		return protocol.ArtifactContent{}, pluginNativeFault(protocol.ErrorCodeRevisionConflict, "artifact does not match its captured source")
	}
	return value, nil
}

func (state *pluginNativeState) startArtifactWatch() {
	state.artifactWatch.started.Do(func() {
		watch := state.artifactWatch
		if !state.runtime.async(func() {
			argument, _ := json.Marshal(state.resource.artifact.Session)
			observer, err := state.runtime.controller.Watch(state.native.Context, "watch_conversation", argument)
			if err == nil {
				defer observer.Close()
			}
			for err == nil {
				var body json.RawMessage
				body, err = observer.Next(state.native.Context)
				if err != nil {
					break
				}
				var value struct {
					Cursor    uint64 `json:"artifact_cursor"`
					Connected bool   `json:"connected"`
				}
				if err = json.Unmarshal(body, &value); err != nil {
					break
				}
				watch.mu.Lock()
				watch.cursor = fmt.Sprintf("%d:%t", value.Cursor, value.Connected)
				close(watch.changed)
				watch.changed = make(chan struct{})
				watch.mu.Unlock()
				state.native.Invalidate()
			}
			watch.mu.Lock()
			watch.err = err
			close(watch.changed)
			watch.changed = make(chan struct{})
			watch.mu.Unlock()
		}) {
			watch.mu.Lock()
			watch.err = pluginNativeFault(protocol.ErrorCodeUnavailable, "artifact observation is unavailable")
			close(watch.changed)
			watch.changed = make(chan struct{})
			watch.mu.Unlock()
		}
	})
}

func (state *pluginNativeState) renderArtifact(view *ui.Context, element plugins.NativeElement, _ func(), _ func(plugins.CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := pluginNativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	id, err := pluginNativeArgument[protocol.ArtifactID]([]json.RawMessage{props["artifact"]}, 0)
	if err != nil {
		return ui.Element{}, err
	}
	suspended := state.native.Suspended()
	if !suspended {
		if _, err := state.artifactSource(id); err != nil {
			return ui.Element{}, err
		}
	}
	if state.runtime.attachments == nil || state.runtime.attachments.native.DocumentView == nil {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "native artifact surface is unavailable")
	}
	revision, err := pluginNativeRevision([]json.RawMessage{props["revision"]}, 0)
	if err != nil {
		return ui.Element{}, err
	}
	mounted := state.artifactViews[identity]
	if !suspended {
		state.startArtifactWatch()
		state.artifactWatch.mu.Lock()
		cursor := state.artifactWatch.cursor
		state.artifactWatch.mu.Unlock()
		if mounted == nil || !pluginNativeRevisionEqual(mounted.revision, revision) || revision == nil && mounted.cursor != cursor {
			mounted = &pluginNativeArtifact{revision: revision, cursor: cursor, pending: true}
			state.artifactViews[identity] = mounted
			state.runtime.async(func() {
				content, err := state.readArtifact(state.native.Context, id, revision)
				state.post(func(*ui.Context) {
					if state.artifactViews[identity] == mounted {
						mounted.pending, mounted.err = false, err
						if err == nil {
							mounted.content = &content
						}
					}
				})
			})
		}
	} else if mounted == nil {
		return ui.Box(view).Size(0, 0), nil
	}
	if mounted.err != nil {
		return ui.Element{}, mounted.err
	}
	if mounted.pending || mounted.content == nil {
		return ui.Row(view).Fill().Center().Children(func() { ui.Spinner(view) }), nil
	}
	rendered := false
	region := ui.Column(view).Fill().MinWidth(0).MinHeight(0).Disabled(suspended).Children(func() {
		rendered = state.runtime.attachments.native.DocumentView(view, mounted.content.Artifact, mounted.content.Html)
	})
	if !rendered {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "native artifact surface is unavailable")
	}
	return region, nil
}

func pluginNativeRevision(arguments []json.RawMessage, index int) (*uint32, error) {
	if index >= len(arguments) || len(arguments[index]) == 0 || string(arguments[index]) == "null" {
		return nil, nil
	}
	value, err := pluginNativeArgument[uint32](arguments, index)
	if err != nil {
		return nil, err
	}
	return &value, nil
}

func pluginNativeRevisionEqual(first, second *uint32) bool {
	return first == nil && second == nil || first != nil && second != nil && *first == *second
}
