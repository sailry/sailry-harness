package live

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type mountedPluginLink struct {
	id           string
	route        shell.LinkRoute
	priority     int
	registration *shell.LinkContribution
	rejected     bool
}

func (r *Runtime) bindMountedPluginLink(mounted *pluginMountedView) {
	if mounted.key.role != "contribution" || mounted.scope.Surface.Kind != "composer" || mounted.scope.Session == nil ||
		mounted.scope.Worktree == nil || mounted.registry == nil || mounted.host == nil || mounted.info.Extension == nil {
		return
	}
	scheme := protocol.PluginLinkScheme(mounted.scope.Package.Name)
	for _, declaration := range mounted.info.Extension.Ui {
		if declaration.LinkScheme != nil && *declaration.LinkScheme == scheme && r.pluginViewShows(mounted.info, declaration, "composer") {
			mounted.link = &mountedPluginLink{
				id: declaration.ID, route: shell.LinkRoute{Kind: shell.LinkScheme, Name: scheme}, priority: declaration.Priority.Int(),
			}
			return
		}
	}
}

func (mounted *pluginMountedView) releasePluginLink() {
	if link := mounted.link; link != nil && link.registration != nil {
		link.registration.Close()
		link.registration = nil
	}
}

func (r *Runtime) syncMountedPluginLink(mounted *pluginMountedView, visible bool) {
	link := mounted.link
	if link == nil || link.rejected {
		return
	}
	if link.registration != nil && !link.registration.Active() {
		r.rejectPluginLink(mounted, fmt.Errorf("conversation link route %q was claimed by a native handler", link.route.Name))
		return
	}
	if !visible || mounted.disconnected || mounted.mounted == nil || r.App.Preview || r.App.DemoOn() || !r.resourceVisible(mounted.key.scope.node.String()) {
		mounted.releasePluginLink()
		return
	}
	if link.registration != nil {
		return
	}
	key, exists := mounted.registry.Key(mounted.scope.Package, link.id)
	if !exists {
		return
	}
	scope := shell.LinkScope{Node: key.Node, Worktree: *mounted.scope.Worktree, Session: mounted.scope.Session.String()}
	registration, err := r.App.RegisterLinkContribution(link.route, scope, func(target shell.LinkTarget) error {
		_, path, _ := strings.Cut(target.URL, ":/")
		body, _ := json.Marshal(path)
		return mounted.registry.Invoke(key, protocol.PluginUiEventKindLink, body)
	}, shell.LinkContributionOptions{
		Priority: link.priority,
		Current: func(target shell.LinkTarget) bool {
			return r.mountedPluginLinkCurrent(mounted, key, target.Session)
		},
	})
	if err != nil {
		r.rejectPluginLink(mounted, err)
		return
	}
	link.registration = registration
}

func (r *Runtime) rejectPluginLink(mounted *pluginMountedView, err error) {
	mounted.link.rejected = true
	mounted.releasePluginLink()
	r.enqueue(func(view *ui.Context) {
		if view != nil && r.pluginViewCurrent(mounted) {
			kit.Error(view, i18n.T(view, "plugins_failed"), err.Error())
		}
	})
}

func (r *Runtime) mountedPluginLinkCurrent(mounted *pluginMountedView, key plugins.ContributionKey, session string) bool {
	owner, current := r.pluginContribution(key)
	return owner != nil && current == mounted && owner.seen && !mounted.disconnected && mounted.owner.controller.Connected() &&
		r.resourceVisible(key.Node.String()) && mounted.owner.ownsLinkConversation(session)
}
