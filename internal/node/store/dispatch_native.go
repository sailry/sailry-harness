package store

import "github.com/sailry/sailry-harness/internal/protocol"

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
