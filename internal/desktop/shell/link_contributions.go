package shell

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type LinkScope struct {
	Node     protocol.NodeID
	Worktree protocol.WorktreeID
	Session  string
}

func (scope LinkScope) overlaps(other LinkScope) bool {
	return scope == other || scope == (LinkScope{}) || other == (LinkScope{})
}

func validContributionURL(target LinkTarget) bool {
	scheme, suffix, _ := strings.Cut(target.URL, ":")
	path, rooted := strings.CutPrefix(suffix, "/")
	return rooted && strings.EqualFold(scheme, target.Route.Name) && protocol.ValidPluginLinkPath(path)
}

func (candidate *linkCandidate) eligible(target LinkTarget) bool {
	if candidate.scope == (LinkScope{}) {
		return true
	}
	return !target.External && candidate.scope == (LinkScope{Node: target.Node, Worktree: target.Worktree, Session: target.Session}) &&
		(candidate.current == nil || candidate.current(target))
}

// LinkContributionOptions ranks a scoped candidate. Current is the host's
// side-effect-free authorization check, evaluated before ranking.
type LinkContributionOptions struct {
	Priority int
	Current  func(LinkTarget) bool
}

// LinkContribution is a window-local claim, including while another wins.
type LinkContribution struct {
	registry  *linkHandlers
	route     LinkRoute
	candidate *linkCandidate
}

func (claim *LinkContribution) Active() bool {
	return claim.registry != nil && slices.Contains((*claim.registry)[claim.route], claim.candidate)
}

func (claim *LinkContribution) Close() {
	if claim.registry != nil {
		entries := (*claim.registry)[claim.route]
		if index := slices.Index(entries, claim.candidate); index >= 0 {
			if len(entries) == 1 {
				delete(*claim.registry, claim.route)
			} else {
				(*claim.registry)[claim.route] = slices.Delete(entries, index, index+1)
			}
		}
	}
	*claim = LinkContribution{}
}

// RegisterLinkContribution uses the same route as RegisterLinkHandler, but only
// admits package schemes and requires an exact source scope at dispatch.
func (a *App) RegisterLinkContribution(route LinkRoute, scope LinkScope, handler func(LinkTarget) error, options ...LinkContributionOptions) (*LinkContribution, error) {
	if route.Kind != LinkScheme || !protocol.ValidPluginLinkScheme(route.Name) || handler == nil ||
		scope.Node == (protocol.NodeID{}) || scope.Worktree == (protocol.WorktreeID{}) || scope.Session == "" || len(options) > 1 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin conversation link registration"}
	}
	var option LinkContributionOptions
	if len(options) == 1 {
		option = options[0]
	}
	return a.linkHandlers.register(route, linkCandidate{
		priority: option.Priority, scope: scope, current: option.Current,
		handle: func(_ *ui.Context, target LinkTarget, _ LinkActions) error { return handler(target) },
	})
}
