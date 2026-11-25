package moduleapi

import (
	"context"
	"errors"
	"time"
)

// GitOptions is the existing injected Git execution boundary. Only a
// workspace provider that implements Git consumes it.
type GitOptions struct {
	Run         GitRunner
	Now         func() time.Time
	Environment []string
	Profile     string
	// Lock covers local Git effects, not waits on a remote transfer.
	Lock func(string) func()
}

type GitRunner func(context.Context, string, []string, []byte) (GitCommandResult, error)

type GitCommandResult struct {
	Stdout, Stderr []byte
	ExitCode       int
	Truncated      bool
	InvalidUTF8    bool
}

type WorkspaceOptions struct {
	Context     context.Context
	Profile     string
	Environment []string
	Lock        func(string) func()
	Git         GitOptions
}

type WorkspaceBinder func(*WorkspaceRuntime) (WorkspaceBindings, error)

// WorkspaceRuntime owns the services selected during one Node's startup.
type WorkspaceRuntime struct {
	options WorkspaceOptions
	opened  map[any]any
	closers []func() error
}

func NewWorkspaceRuntime(options WorkspaceOptions) *WorkspaceRuntime {
	return &WorkspaceRuntime{options: options, opened: make(map[any]any)}
}

// WorkspaceFactory identifies one shared service. Modules that select the
// same factory instance bind only their own handlers to that service.
type WorkspaceFactory[T any] struct {
	Open func(WorkspaceOptions) (T, func() error, error)
}

type workspaceInstance[T any] struct{ service T }

func (f *WorkspaceFactory[T]) Bind(bind func(T) WorkspaceBindings) WorkspaceBinder {
	return func(runtime *WorkspaceRuntime) (WorkspaceBindings, error) {
		if runtime.opened == nil {
			return WorkspaceBindings{}, errors.New("workspace runtime is closed")
		}
		if instance, ok := runtime.opened[f]; ok {
			return bind(instance.(workspaceInstance[T]).service), nil
		}
		service, closeService, err := f.Open(runtime.options)
		if err != nil {
			if closeService != nil {
				err = errors.Join(err, closeService())
			}
			return WorkspaceBindings{}, err
		}
		runtime.opened[f] = workspaceInstance[T]{service: service}
		if closeService != nil {
			runtime.closers = append(runtime.closers, closeService)
		}
		return bind(service), nil
	}
}

func (r *WorkspaceRuntime) Close() error {
	if r == nil {
		return nil
	}
	var err error
	for i := len(r.closers) - 1; i >= 0; i-- {
		err = errors.Join(err, r.closers[i]())
	}
	r.opened, r.closers = nil, nil
	return err
}
