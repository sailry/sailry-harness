package testing

import (
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// HarnessAssembly returns the shipped Harness Desktop assembly for product integration tests.
func HarnessAssembly() shell.Assembly { return desktop.Harness() }

type PluginWorkspacePrefs = shell.PluginWorkspacePrefs
type NativeFileActions = live.NativeFileActions
type WorkspaceFileActions = live.WorkspaceNative
