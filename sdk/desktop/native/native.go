// Package native connects product-owned pages to the shared Desktop workspace.
package native

import (
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Assembly = shell.Assembly
type Module = shell.Module
type Page = shell.Page
type NativePage = shell.NativePage
type Feature = shell.Feature
type App = shell.App
type Project = panel.Project

const (
	HeaderHeight = shell.HeaderHeight
	RailWidth    = shell.RailWidth
	NavWidth     = shell.NavWidth

	ConversationPage = shell.ConversationPage
	FilesPage        = shell.FilesPage
	GitPage          = shell.GitPage
	Changes          = panel.Changes
	UsagePageGit     = protocol.FeatureUsePageGit
)

func Assemble(modules ...Module) Assembly { return shell.Assemble(modules...) }

// NewPreview opens an isolated fixture window without models, tools or user data.
func NewPreview(assembly Assembly) *App { return shell.NewWithAssembly(preview.Load(), assembly) }
