package main

import (
	"context"
	"fmt"
	"os"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/sdk/host"
)

func main() {
	code, err := run(os.Args[1:])
	if err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "sailry-host: "+err.Error())
		code = 1
	}
	os.Exit(code)
}

func run(args []string) (int, error) {
	product := identity.OfficialDesktop()
	product.Version = link.ProductVersion
	product.DefaultProfile = core.Default
	return host.Run(args, host.Options{
		Identity: product,
		Assembly: hostassembly.Harness(),
		Acceptance: func(ctx context.Context, args []string) (int, error) {
			return runAcceptance(ctx, args, os.Stdout)
		},
	})
}
