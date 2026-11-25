package native

import "github.com/sailry/sailry-harness/internal/desktop/shell"

type LinkKind = shell.LinkKind
type LinkRoute = shell.LinkRoute
type LinkTarget = shell.LinkTarget
type LinkFileDestination = shell.LinkFileDestination
type LinkActions = shell.LinkActions
type LinkHandler = shell.LinkHandler
type LinkScope = shell.LinkScope
type LinkContribution = shell.LinkContribution
type LinkContributionOptions = shell.LinkContributionOptions

const (
	LinkScheme          = shell.LinkScheme
	LinkReference       = shell.LinkReference
	LinkFile            = shell.LinkFile
	LinkDirectory       = shell.LinkDirectory
	LinkDefaultPriority = shell.LinkDefaultPriority
)
