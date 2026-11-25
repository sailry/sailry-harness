package live

import (
	"context"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/view"
)

type windowViewHost struct {
	runtime *Runtime
	mounts  map[*nativeWindowResource]struct{}
}

func (h *windowViewHost) services() view.Services {
	r, controller := h.runtime, h.runtime.controller
	return view.Services{
		Node: controller.Target(), Connected: func() bool { return h.open() && controller.Connected() },
		Next: h.next, Post: h.post, Wake: r.wake,
		Async: func(work func()) bool { return work != nil && h.open() && r.async(work) },
		Prepare: func(command protocol.Command) protocol.Request {
			if !h.open() {
				return protocol.Request{}
			}
			return controller.Prepare(command)
		},
		Execute: func(ctx context.Context, request protocol.Request) (protocol.Output, error) {
			return windowRequest(h, ctx, request, controller.Execute)
		},
		Settle: func(ctx context.Context, request protocol.Request) (protocol.Output, error) {
			return windowRequest(h, ctx, request, controller.Settle)
		},
		Resolve: h.resolve,
		Outcome: func(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
			return windowRequest(h, ctx, request, controller.Outcome)
		},
		TerminalLaunch: func(c *ui.Context) any {
			return protocol.SshTerminalLaunch{Viewport: terminalViewport, Appearance: pluginNativeAppearance(theme.Of(c))}
		},
		MountTerminal: h.mountTerminal, MountConversation: h.mountConversation,
		PickFiles: h.pickFiles, PickFolder: h.pickFolder, PickDestination: h.pickDestination, PickSecret: h.pickSecret,
		WriteClipboard: h.writeClipboard, Preview: h.preview,
		OpenSource: h.openSource, OpenDestination: h.openDestination,
		Upload: h.upload, Download: h.download, ReleaseStream: h.releaseStream,
	}
}

func (h *windowViewHost) open() bool {
	r := h.runtime
	r.mu.Lock()
	defer r.mu.Unlock()
	return !r.closed && r.ctx.Err() == nil
}

func (h *windowViewHost) post(apply func(*ui.Context)) bool {
	r := h.runtime
	r.mu.Lock()
	if apply == nil || r.closed || r.ctx.Err() != nil {
		r.mu.Unlock()
		return false
	}
	r.queue = append(r.queue, func(c *ui.Context) {
		if h.open() {
			apply(c)
		}
	})
	r.mu.Unlock()
	r.wake()
	return true
}

func (h *windowViewHost) next(ctx context.Context, seen string) (view.Change, error) {
	controller := h.runtime.controller
	changed := make(chan struct{}, 1)
	unsubscribe := controller.OnChange(func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	defer unsubscribe()
	for {
		if err := ctx.Err(); err != nil {
			return view.Change{}, err
		}
		if !h.open() {
			return view.Change{}, context.Canceled
		}
		connected := controller.Connected()
		cursor := strconv.FormatUint(controller.Revision(), 10) + ":" + strconv.FormatBool(connected)
		if cursor != seen {
			return view.Change{Cursor: cursor, Connected: connected}, nil
		}
		select {
		case <-ctx.Done():
			return view.Change{}, ctx.Err()
		case <-h.runtime.ctx.Done():
			return view.Change{}, h.runtime.ctx.Err()
		case <-changed:
		}
	}
}

func (h *windowViewHost) operationContext(ctx context.Context) (context.Context, func()) {
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(h.runtime.ctx, cancel)
	if !h.open() {
		cancel()
	}
	return ctx, func() { stop(); cancel() }
}

func windowRequest[Value any](h *windowViewHost, ctx context.Context, request protocol.Request, run func(context.Context, protocol.Request) (Value, error)) (Value, error) {
	var zero Value
	// A compiled contribution uses the window's client, not a package scope
	// supplied by a template. A request must stay on the captured Node.
	if request.Target != h.runtime.controller.Target() || request.Version != protocol.VERSION {
		return zero, fileActionFault(protocol.ErrorCodeWrongTarget, "request belongs to another Node or protocol version")
	}
	if request.Plugin != nil || request.ID == (protocol.RequestID{}) {
		return zero, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid native window request")
	}
	ctx, done := h.operationContext(ctx)
	defer done()
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	return run(ctx, request)
}

func (h *windowViewHost) resolve(ctx context.Context, request protocol.Request) (view.Receipt, error) {
	result, err := windowRequest(h, ctx, request, h.runtime.controller.Resolve)
	if err != nil {
		return view.Receipt{}, err
	}
	var state string
	switch result.State {
	case client.RequestPending:
		state = "pending"
	case client.RequestApplied:
		state = "applied"
	case client.RequestFailed:
		state = "failed"
	case client.RequestAbsent:
		state = "absent"
	case client.RequestUnknown:
		state = "unknown"
	case client.RequestReplayable:
		state = "replayable"
	default:
		return view.Receipt{}, fileActionFault(protocol.ErrorCodeInternal, "invalid native window receipt")
	}
	return view.Receipt{State: state, Output: result.Output, Fault: result.Fault}, nil
}

func (h *windowViewHost) trackMount(resource nativeWindowResource) func() {
	if h.mounts == nil {
		h.mounts = make(map[*nativeWindowResource]struct{})
		h.runtime.nativeWindowResources = append(h.runtime.nativeWindowResources, nativeWindowResource{
			afterFrame: func() {
				for mount := range h.mounts {
					if mount.afterFrame != nil {
						mount.afterFrame()
					}
				}
			},
			close: func() {
				for mount := range h.mounts {
					mount.close()
				}
				clear(h.mounts)
			},
		})
	}
	h.mounts[&resource] = struct{}{}
	return func() {
		if _, exists := h.mounts[&resource]; exists {
			delete(h.mounts, &resource)
			resource.close()
		}
	}
}
