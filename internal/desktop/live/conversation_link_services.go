package live

import (
	"net/url"
	"strings"
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) dispatchConversationLink(c *ui.Context, owner *Runtime, target shell.LinkTarget, actions shell.LinkActions) (bool, error) {
	root := r.localRuntime()
	var active atomic.Bool
	active.Store(true)
	defer active.Store(false)
	if owner != nil && owner.controller != nil {
		target.Node = owner.controller.Target()
		actions.OpenPackageDetails = func(c *ui.Context, name string) {
			if active.Load() {
				owner.openPackageDetails(c, name)
			}
		}
	}
	node := ""
	if target.Node != (protocol.NodeID{}) {
		node = target.Node.String()
	} else if cv := root.App.Data.Find(target.Session); cv != nil {
		node = cv.Node
	}
	if node != "" {
		actions.OpenKnowledge = func(c *ui.Context, source shell.Source) {
			if active.Load() {
				root.openKnowledge(c, node, source)
			}
		}
	}
	actions.ShowReference = func(label, subtitle, text string) {
		if active.Load() {
			text = strings.NewReplacer("\r\n", "\n", "\r", "\n").Replace(text)
			root.App.Messages.ShowReference(target.Session, label, subtitle, "    "+strings.ReplaceAll(text, "\n", "\n    "))
		}
	}
	scope := func() (connectionScope, bool) {
		if !active.Load() || owner == nil || owner.controller == nil {
			return connectionScope{}, false
		}
		current, ok := owner.session(target.Session)
		return connectionScope{project: current.Project, resource: current.Config.Resource}, ok
	}
	actions.SSHReference = func(id protocol.SshID) (shell.SSHReferenceEndpoint, bool) {
		if scope, ok := scope(); ok {
			return referenceSSHEndpoint(owner.snapshot(), scope, id)
		}
		return shell.SSHReferenceEndpoint{}, false
	}
	actions.DatabaseReference = func(id protocol.DatabaseID) (string, bool) {
		if scope, ok := scope(); ok {
			return referenceDatabaseName(owner.snapshot(), scope, id)
		}
		return "", false
	}
	return root.App.DispatchLink(c, target, actions)
}

func (r *Runtime) openExternalConversationURL(c *ui.Context, session, link string) error {
	parsed, err := url.Parse(link)
	if err != nil || parsed.Scheme == "" || !validLinkURL(parsed) {
		return unavailableLink()
	}
	target := shell.LinkTarget{Route: shell.LinkRoute{Kind: shell.LinkScheme, Name: parsed.Scheme}, Session: session, URL: link, External: true}
	handled, err := r.dispatchConversationLink(c, r, target, shell.LinkActions{})
	if !handled {
		return unavailableLink()
	}
	return err
}

func (r *Runtime) openLinkedPackage(c *ui.Context, session, name string) error {
	target := shell.LinkTarget{Route: shell.LinkRoute{Kind: shell.LinkReference, Name: "plugin"}, Session: session,
		Reference: protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "plugin", Data: name}}}
	handled, err := r.dispatchConversationLink(c, r, target, shell.LinkActions{})
	if !handled {
		return unavailableLink()
	}
	return err
}
