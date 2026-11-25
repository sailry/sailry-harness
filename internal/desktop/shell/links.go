package shell

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type LinkKind string

const (
	LinkScheme    LinkKind = "scheme"
	LinkReference LinkKind = "reference"
	LinkFile      LinkKind = "file"
	LinkDirectory LinkKind = "directory"

	LinkDefaultPriority = -1
)

// LinkRoute claims a URL scheme, a reference kind, or a validated workspace
// file or directory. Name is empty for workspace paths.
type LinkRoute struct {
	Kind LinkKind
	Name string
}

// LinkTarget carries the source location. Path is relative to Worktree ("."
// for its root); Line is one-based or zero if unspecified.
type LinkTarget struct {
	Route     LinkRoute
	Node      protocol.NodeID
	Session   string
	Worktree  protocol.WorktreeID
	Turn      protocol.TurnID
	URL       string
	Reference protocol.ConversationReference
	Path      string
	Line      int
	External  bool
	// ConversationWorkspace reports whether this worktree can be shown in the
	// source conversation's panel. It does not select a destination.
	ConversationWorkspace bool
}

// LinkFileDestination names exactly one registered workspace page or panel.
type LinkFileDestination struct {
	Page  Page
	Panel panel.Kind
}

// LinkActions act only on the captured target, during the handler call.
type LinkActions struct {
	LinkServices
	OpenWorkspaceFile      func(LinkFileDestination) error
	ShowWorkspaceDirectory func(Page, func(*panel.Project, string)) error
	OpenSession            func() error
}

// LinkHandler owns the attempted open. An error never selects another handler.
type LinkHandler func(*ui.Context, LinkTarget, LinkActions) error

type linkHandlers map[LinkRoute][]*linkCandidate

type linkCandidate struct {
	priority int
	scope    LinkScope
	current  func(LinkTarget) bool
	handle   LinkHandler
}

func (registry *linkHandlers) register(route LinkRoute, candidate linkCandidate) (*LinkContribution, error) {
	valid := false
	switch route.Kind {
	case LinkScheme, LinkReference:
		valid = route.Name != "" && route.Name == strings.ToLower(strings.TrimSpace(route.Name))
	case LinkFile, LinkDirectory:
		valid = route.Name == ""
	}
	if !valid || candidate.handle == nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("invalid link route %q/%q", route.Kind, route.Name)}
	}
	for _, entry := range (*registry)[route] {
		if entry.priority == candidate.priority && entry.scope.overlaps(candidate.scope) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict,
				Message: fmt.Sprintf("link route %q/%q is already registered at priority %d in an overlapping scope", route.Kind, route.Name, candidate.priority)}
		}
	}
	if *registry == nil {
		*registry = make(linkHandlers)
	}
	(*registry)[route] = append((*registry)[route], &candidate)
	return &LinkContribution{registry: registry, route: route, candidate: &candidate}, nil
}

// RegisterLinkHandler declares a native handler at priority zero.
func (a *Assembly) RegisterLinkHandler(route LinkRoute, handler LinkHandler) {
	a.RegisterLinkHandlerWithPriority(route, 0, handler)
}

// RegisterLinkHandlerWithPriority declares a handler for every live window.
// An overlapping claim at the same priority is a configuration error.
func (a *Assembly) RegisterLinkHandlerWithPriority(route LinkRoute, priority int, handler LinkHandler) {
	if _, err := a.linkHandlers.register(route, linkCandidate{priority: priority, handle: handler}); err != nil {
		panic("shell: " + err.Error())
	}
}

// RegisterLinkHandler binds a native handler at priority zero to this window.
func (a *App) RegisterLinkHandler(route LinkRoute, handler LinkHandler) (release func(), err error) {
	return a.RegisterLinkHandlerWithPriority(route, 0, handler)
}

// RegisterLinkHandlerWithPriority binds a candidate to this window. On error
// it leaves existing candidates unchanged and returns no release function.
func (a *App) RegisterLinkHandlerWithPriority(route LinkRoute, priority int, handler LinkHandler) (release func(), err error) {
	registration, err := a.linkHandlers.register(route, linkCandidate{priority: priority, handle: handler})
	if err != nil {
		return nil, err
	}
	return registration.Close, nil
}

// RegisterDefaultLinkHandler registers the host's behavior at priority -1.
func (a *App) RegisterDefaultLinkHandler(route LinkRoute, handler LinkHandler) error {
	_, err := a.RegisterLinkHandlerWithPriority(route, LinkDefaultPriority, handler)
	return err
}

func (a *App) DispatchLink(c *ui.Context, target LinkTarget, actions LinkActions) (bool, error) {
	if a.Preview || a.DemoOn() {
		return false, nil
	}
	if target.Route.Kind == LinkScheme && protocol.ValidPluginLinkScheme(target.Route.Name) && !validContributionURL(target) {
		return true, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin conversation link"}
	}
	candidates := a.linkHandlers[target.Route]
	if len(candidates) == 0 {
		return false, nil
	}
	var winner *linkCandidate
	for _, candidate := range candidates {
		if !candidate.eligible(target) {
			continue
		}
		if winner == nil || candidate.priority > winner.priority {
			winner = candidate
		}
	}
	if winner == nil {
		return true, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "conversation link is outside its mounted plugin scope"}
	}
	return true, winner.handle(c, target, actions)
}

func (a *App) WorkspacePageEnabled(page Page) bool {
	definition := a.nativePage(page)
	return definition != nil && definition.Resource != ""
}

func (a *App) installLinkHandlers(assembly Assembly) {
	if a.Preview {
		return
	}
	handlers := make(linkHandlers, len(assembly.linkHandlers))
	for route, candidates := range assembly.linkHandlers {
		handlers[route] = slices.Clone(candidates)
	}
	assembly.linkHandlers = handlers
	for _, scheme := range []string{"http", "https", "mailto", "tel"} {
		assembly.RegisterLinkHandlerWithPriority(LinkRoute{Kind: LinkScheme, Name: scheme}, LinkDefaultPriority, func(c *ui.Context, target LinkTarget, _ LinkActions) error {
			c.OpenURL(target.URL)
			return nil
		})
	}
	assembly.RegisterLinkHandlerWithPriority(LinkRoute{Kind: LinkScheme, Name: "sailry"}, LinkDefaultPriority, func(_ *ui.Context, _ LinkTarget, actions LinkActions) error {
		if actions.OpenSession == nil {
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "session link is unavailable"}
		}
		return actions.OpenSession()
	})
	a.linkHandlers = handlers
}
