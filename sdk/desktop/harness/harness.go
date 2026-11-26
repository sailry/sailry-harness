// Package harness exposes the shipped Harness Desktop module set as an opt-in
// preset so minimal forks importing sdk/desktop do not pull in Harness modules.
package harness

import (
	harnessdesktop "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/sdk/desktop"
)

// Modules returns the ordered desktop modules shipped with Harness so
// downstream products can compose them without duplicating the Harness list.
func Modules() []desktop.Module { return harnessdesktop.HarnessModules() }

// Assembly returns the default Harness desktop assembly.
func Assembly() desktop.Assembly { return harnessdesktop.Harness() }
