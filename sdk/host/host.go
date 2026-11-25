// Package host runs a product-owned Host assembly through the shared Host CLI.
package host

import (
	"context"

	"github.com/sailry/sailry-harness/internal/hostctl"
)

type Options = hostctl.Options

// Run owns process signals and returns the CLI exit code after Host shutdown.
func Run(args []string, options Options) (int, error) {
	return hostctl.Run(args, options)
}

// RunContext uses the caller's context without installing signal handlers.
func RunContext(ctx context.Context, args []string, options Options) (int, error) {
	return hostctl.RunContext(ctx, args, options)
}
