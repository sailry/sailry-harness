package hostctl

import (
	"context"
	"io"
	"os"
	"os/signal"
	"syscall"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
)

type Options struct {
	Identity   identity.Desktop
	Assembly   moduleapi.Assembly
	Acceptance func(context.Context, []string) (int, error)
	Stdin      io.Reader
	Stdout     io.Writer
	Stderr     io.Writer
}

// Run owns process signal handling and runs the Host CLI until shutdown.
func Run(args []string, config Options) (int, error) {
	computer.Trampoline()
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	return run(ctx, args, config, cancel)
}

// RunContext leaves cancellation and signal handling with the caller.
func RunContext(ctx context.Context, args []string, config Options) (int, error) {
	computer.Trampoline()
	return run(ctx, args, config, func() {})
}

func run(ctx context.Context, args []string, config Options, restoreSignals func()) (int, error) {
	if config.Stdin == nil {
		config.Stdin = os.Stdin
	}
	if config.Stdout == nil {
		config.Stdout = os.Stdout
	}
	if config.Stderr == nil {
		config.Stderr = os.Stderr
	}
	return RunCLI(ctx, args, config.Stdin, config.Stdout, config.Stderr, CLIHandlers{
		Identity: config.Identity,
		Service: func(ctx context.Context, args []string) (int, error) {
			return runHostService(ctx, args, config, restoreSignals)
		},
		Acceptance: config.Acceptance,
	})
}
