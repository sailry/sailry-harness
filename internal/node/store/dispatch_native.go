package store

import (
	"context"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type nativeDispatchKey struct{}

// WithNativeDispatch binds the namespaces owned by assembled native Node
// modules (such as scheduled-tasks and reminders) so existing dispatch
// schedules, handlers and jobs keep running when the legacy plugins are retired.
func WithNativeDispatch(ctx context.Context, namespaces []string) context.Context {
	if len(namespaces) == 0 {
		return ctx
	}
	return context.WithValue(ctx, nativeDispatchKey{}, slices.Clone(namespaces))
}

func hasNativeDispatchNamespace(ctx context.Context, name string) bool {
	if name == "" {
		return false
	}
	namespaces, _ := ctx.Value(nativeDispatchKey{}).([]string)
	return slices.Contains(namespaces, name)
}

func nativeDispatchStorageInfo(ctx context.Context, name string) (protocol.PluginInfo, bool) {
	if !hasNativeDispatchNamespace(ctx, name) {
		return protocol.PluginInfo{}, false
	}
	return NativeStorageInfo(name)
}

func dispatchNativeHandler(handler protocol.DispatchHandler) error {
	if handler.Name != "profile-backup" || handler.Source.Package != dispatchNativeOwner ||
		handler.Source.Topic != "profile-backup" || handler.Queue != "profile-backup" ||
		handler.Callback.Scope.Worktree != nil || handler.Callback.Scope.Session != nil ||
		handler.Callback.Completion != protocol.DispatchCompletionCommand ||
		len(handler.Callback.Bindings) != 0 || handler.Callback.Command == nil ||
		handler.Callback.Command.Kind != "export_backup" {
		return fault(protocol.ErrorCodePermissionDenied, "native callback is not a registered backup")
	}
	return nil
}

func dispatchNativeCallback(owner string, job protocol.DispatchJob, callback protocol.DispatchCallback) error {
	if owner != dispatchNativeOwner || job.Handler != "profile-backup" || job.Queue != "profile-backup" {
		return fault(protocol.ErrorCodePermissionDenied, "callback has no native owner")
	}
	return dispatchNativeHandler(protocol.DispatchHandler{
		Name: job.Handler, Enabled: true, Source: job.Event.Source, Queue: job.Queue, Callback: callback,
	})
}
