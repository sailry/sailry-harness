// Package desktop starts a native Desktop with a product-selected assembly.
package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/application"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

type Options = application.Options
type Assembly = shell.Assembly
type Module = shell.Module

// Assemble collects only the contributions selected by the caller.
func Assemble(modules ...Module) Assembly { return shell.Assemble(modules...) }

// Run owns the native application lifecycle and must run on the main goroutine.
func Run(args []string, options Options) error { return application.Run(args, options) }
