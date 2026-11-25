package live

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type legacyFileScope struct {
	name    string
	session protocol.SessionID
}

// The worktree owns the editor drafts and admitted operations. A UI mount
// only subscribes to these; remounting never reinstalls a package or retries a write.
type legacyFileWorkspace struct {
	documents        []*legacyDocument
	clipboard        *legacyClipboard
	transfers        []*legacyTransfer
	transferRevision uint64
	reveal           legacyDocumentReveal
}

type legacyFileHost struct {
	state            *pluginNativeState
	project          *workspaceProject
	worktree         protocol.WorktreeID
	workspace        *legacyFileWorkspace
	documents        legacyDocumentView
	transfers        legacyFileFeed
	transferRevision uint64
}

func (state *pluginNativeState) legacyFileOperations() {
	if state.scope.Surface.Kind != "workspace" || state.scope.Worktree == nil {
		return
	}
	project, err := state.fileProject()
	if err != nil {
		return
	}
	key := legacyFileScope{name: state.scope.Package.Name}
	if state.scope.Session != nil {
		key.session = *state.scope.Session
	}
	if project.legacyFiles == nil {
		project.legacyFiles = map[legacyFileScope]*legacyFileWorkspace{}
	}
	workspace := project.legacyFiles[key]
	if workspace == nil {
		workspace = &legacyFileWorkspace{}
		project.legacyFiles[key] = workspace
	}
	host := &legacyFileHost{state: state, project: project, worktree: *state.scope.Worktree, workspace: workspace}
	host.documentOperations()
	host.transferOperations()
	state.native.Components = append(state.native.Components, plugins.NativeComponent{Module: "sailry/documents", Export: "DocumentSurface", Render: host.renderDocument})
	frame := state.native.Frame
	state.native.Frame = func(view *ui.Context) {
		if frame != nil {
			frame(view)
		}
		host.publishDocuments(view)
		host.publishTransfers()
	}
}

func (host *legacyFileHost) check(write bool) error {
	state := host.state
	project, err := state.fileProject()
	if err != nil {
		return err
	}
	if project != host.project || !state.runtime.controller.Connected() {
		return pluginNativeFault(protocol.ErrorCodeUnavailable, "captured file workspace is unavailable")
	}
	if !write {
		return nil
	}
	info, available := state.packageInfo()
	if !available || !slices.Contains(info.Extension.Actions, protocol.PluginActionWriteFiles) {
		return pluginNativeFault(protocol.ErrorCodePermissionDenied, "plugin did not declare file writes")
	}
	if state.scope.Session != nil {
		if session, exists := state.runtime.sessionWithID(*state.scope.Session); !exists || session.Worktree != project.owner.ID || session.Delegation != nil {
			return pluginNativeFault(protocol.ErrorCodePermissionDenied, "captured session cannot edit files")
		}
	}
	return nil
}

func (host *legacyFileHost) request(command protocol.Command) protocol.Request {
	request := host.state.runtime.controller.Prepare(command)
	scope := host.state.scope
	request.Plugin = &scope
	return request
}

func legacyFilePath(name string) bool {
	return validWorkspaceRelative(name) && !strings.ContainsRune(name, '\x00')
}

func legacyFileFault(err error) *protocol.Fault {
	if err == nil {
		return nil
	}
	code := faultCode(err)
	switch {
	case code != "":
	case errors.Is(err, context.Canceled):
		code = protocol.ErrorCodeCancelled
	default:
		code = protocol.ErrorCodeUnavailable
	}
	return &protocol.Fault{Code: code, Message: faultMessage(err)}
}

type legacyFileFeed struct {
	mu      sync.Mutex
	cursor  uint64
	body    json.RawMessage
	changed chan struct{}
}

func (feed *legacyFileFeed) publish(value func(string) any) {
	feed.mu.Lock()
	defer feed.mu.Unlock()
	feed.cursor++
	feed.body, _ = pluginNativeJSON(value(strconv.FormatUint(feed.cursor, 10)))
	if feed.changed != nil {
		close(feed.changed)
	}
	feed.changed = make(chan struct{})
}

func (feed *legacyFileFeed) read() json.RawMessage {
	feed.mu.Lock()
	defer feed.mu.Unlock()
	return feed.body
}

func (feed *legacyFileFeed) next(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	seen, err := pluginNativeArgument[string](args, 0)
	if err != nil {
		return nil, err
	}
	for {
		feed.mu.Lock()
		cursor, changed, body := feed.cursor, feed.changed, feed.body
		feed.mu.Unlock()
		if body != nil && strconv.FormatUint(cursor, 10) != seen {
			return body, nil
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-changed:
		}
	}
}
