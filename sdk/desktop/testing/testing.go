// Package testing provides deterministic workspace fixtures for native page tests.
package testing

import (
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ResourceTarget = shell.ResourceTarget
type Data = viewmodel.Data
type Project = viewmodel.Project
type Workspace = viewmodel.Workspace
type FileNode = viewmodel.FileNode
type FileChange = viewmodel.FileChange
type Actions = panel.Actions
type GitState = panel.GitState

func NewProject(id string, workspace *Workspace, actions *Actions) *panel.Project {
	return panel.NewLiveProject(id, workspace, actions)
}

func NewLive(data *Data, assembly shell.Assembly) *shell.App {
	return shell.NewLiveWithAssembly(data, assembly)
}
