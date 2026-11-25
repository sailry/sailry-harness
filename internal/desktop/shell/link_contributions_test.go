package shell

import (
	"errors"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLinkContributionsRankOnlyAuthorizedSources(t *testing.T) {
	for _, test := range []struct {
		name      string
		hostFirst bool
	}{{"host first", true}, {"package first", false}} {
		t.Run(test.name, func(t *testing.T) {
			a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
			route := LinkRoute{Kind: LinkScheme, Name: protocol.PluginLinkScheme("fixture")}
			worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID().String()
			one := LinkScope{Node: protocol.NodeID{1}, Worktree: worktree, Session: session}
			two := LinkScope{Node: protocol.NodeID{2}, Worktree: worktree, Session: session}
			deny := errors.New("fixture denied")
			allowed := true
			var first, second, host, checked int
			var release func()
			registerHost := func() {
				var err error
				release, err = a.RegisterLinkHandler(route, func(*ui.Context, LinkTarget, LinkActions) error { host++; return nil })
				if err != nil {
					t.Fatal(err)
				}
			}
			if test.hostFirst {
				registerHost()
			}
			option := LinkContributionOptions{Priority: 10, Current: func(LinkTarget) bool { checked++; return allowed }}
			claim, err := a.RegisterLinkContribution(route, one, func(LinkTarget) error { first++; return deny }, option)
			if err != nil {
				t.Fatal(err)
			}
			if duplicate, err := a.RegisterLinkContribution(route, one, func(LinkTarget) error { t.Error("duplicate ran"); return nil }, option); duplicate != nil || !linkContributionFault(err, protocol.ErrorCodeRevisionConflict) {
				t.Fatalf("same scope and priority: %v, %v", duplicate, err)
			}
			other, err := a.RegisterLinkContribution(route, two, func(LinkTarget) error { second++; return nil }, LinkContributionOptions{Priority: 10})
			if err != nil {
				t.Fatal(err)
			}
			if !test.hostFirst {
				registerHost()
			}
			open := func(scope LinkScope, external bool) (bool, error) {
				return a.DispatchLink(nil, LinkTarget{Route: route, Node: scope.Node, Session: scope.Session, Worktree: scope.Worktree, URL: route.Name + ":/notes", External: external}, LinkActions{})
			}
			if handled, err := open(one, false); !handled || !errors.Is(err, deny) || first != 1 || second != 0 || host != 0 || checked != 1 {
				t.Fatalf("matched denial tried a lower priority: %t, %v, calls %d/%d/%d, checked %d", handled, err, first, second, host, checked)
			}
			if handled, err := open(two, false); !handled || err != nil || second != 1 || checked != 1 || host != 0 {
				t.Fatalf("disjoint scope did not select its candidate: %t, %v", handled, err)
			}
			allowed = false
			if handled, err := open(one, false); !handled || err != nil || host != 1 || first != 1 || checked != 2 {
				t.Fatalf("revoked scope entered ranking: %t, %v, calls %d/%d, checked %d", handled, err, host, first, checked)
			}
			allowed = true
			wrong := one
			wrong.Worktree = protocol.NewWorktreeID()
			if handled, err := open(wrong, false); !handled || err != nil || host != 2 || checked != 2 {
				t.Fatalf("wrong scope consulted the package: %t, %v", handled, err)
			}
			if handled, err := open(one, true); !handled || err != nil || host != 3 || checked != 2 {
				t.Fatalf("external choice consulted the package: %t, %v", handled, err)
			}
			high, err := a.RegisterLinkHandlerWithPriority(route, 20, func(*ui.Context, LinkTarget, LinkActions) error { host++; return nil })
			if err != nil {
				t.Fatal(err)
			}
			if handled, err := open(one, false); !handled || err != nil || host != 4 || first != 1 || !claim.Active() || !other.Active() {
				t.Fatalf("native override revoked another candidate: %t, %v", handled, err)
			}
			high()
			release()
			if handled, err := open(one, false); !handled || !errors.Is(err, deny) || first != 2 || host != 4 {
				t.Fatalf("release did not restore the authorized candidate: %t, %v", handled, err)
			}
			claim.Close()
			if handled, err := open(one, false); !handled || !linkContributionFault(err, protocol.ErrorCodePermissionDenied) || !other.Active() {
				t.Fatalf("scope release fell through or affected another Node: %t, %v", handled, err)
			}
			a.CloseWindowBindings()
			if other.Active() {
				t.Fatal("window close left a mounted candidate")
			}
		})
	}
}

func TestLinkContributionsCannotClaimReservedSchemes(t *testing.T) {
	a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
	scope := LinkScope{Node: protocol.NodeID{1}, Worktree: protocol.NewWorktreeID(), Session: protocol.NewSessionID().String()}
	for _, scheme := range []string{"https", "mailto", "sailry", "sailry-knowledge", "data"} {
		if claim, err := a.RegisterLinkContribution(LinkRoute{Kind: LinkScheme, Name: scheme}, scope, func(LinkTarget) error { return nil }, LinkContributionOptions{Priority: 100}); claim != nil || !linkContributionFault(err, protocol.ErrorCodeInvalidRequest) {
			t.Fatalf("reserved scheme %q: %v, %v", scheme, claim, err)
		}
	}
}

func TestLinkContributionsRejectInvalidURLsBeforeChoosingHandler(t *testing.T) {
	a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
	route := LinkRoute{Kind: LinkScheme, Name: protocol.PluginLinkScheme("fixture")}
	check := func(url string) {
		t.Helper()
		if handled, err := a.DispatchLink(nil, LinkTarget{Route: route, URL: url}, LinkActions{}); !handled || !linkContributionFault(err, protocol.ErrorCodeInvalidRequest) {
			t.Fatalf("invalid plugin URL %q: %t, %v", url, handled, err)
		}
	}
	check(route.Name + ":42")
	if _, err := a.RegisterLinkHandlerWithPriority(route, 10, func(*ui.Context, LinkTarget, LinkActions) error {
		t.Error("native override received an invalid plugin URL")
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	for _, url := range []string{route.Name + "://other/notes", route.Name + ":/../notes", "https://fixture.invalid/"} {
		check(url)
	}
}

func TestLinkContributionsRejectEqualPriorityInOverlappingScopes(t *testing.T) {
	route := LinkRoute{Kind: LinkScheme, Name: protocol.PluginLinkScheme("fixture")}
	scope := LinkScope{Node: protocol.NodeID{1}, Worktree: protocol.NewWorktreeID(), Session: protocol.NewSessionID().String()}
	target := LinkTarget{Route: route, Node: scope.Node, Worktree: scope.Worktree, Session: scope.Session, URL: route.Name + ":/notes"}
	var nativeCalls, packageCalls int
	native := func(*ui.Context, LinkTarget, LinkActions) error { nativeCalls++; return nil }
	contribution := func(LinkTarget) error { packageCalls++; return nil }
	app := func() *App {
		return NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
	}
	a := app()
	if _, err := a.RegisterLinkHandler(route, native); err != nil {
		t.Fatal(err)
	}
	if claim, err := a.RegisterLinkContribution(route, scope, contribution); claim != nil || !linkContributionFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("overlapping scope at priority zero: %v, %v", claim, err)
	}
	if handled, err := a.DispatchLink(nil, target, LinkActions{}); !handled || err != nil || nativeCalls != 1 || packageCalls != 0 {
		t.Fatalf("the native-first conflict changed dispatch: %t, %v, calls %d/%d", handled, err, nativeCalls, packageCalls)
	}
	b := app()
	claim, err := b.RegisterLinkContribution(route, scope, contribution)
	if err != nil {
		t.Fatal(err)
	}
	if release, err := b.RegisterLinkHandler(route, native); release != nil || !linkContributionFault(err, protocol.ErrorCodeRevisionConflict) || !claim.Active() || nativeCalls != 1 || packageCalls != 0 {
		t.Fatalf("package-first conflict changed the registry or invoked a handler: claimed %t, release %t, error %v", claim.Active(), release != nil, err)
	}
	if handled, err := b.DispatchLink(nil, target, LinkActions{}); !handled || err != nil || nativeCalls != 1 || packageCalls != 1 {
		t.Fatalf("the package-first conflict changed dispatch: %t, %v, calls %d/%d", handled, err, nativeCalls, packageCalls)
	}
}

func linkContributionFault(err error, code protocol.ErrorCode) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == code
}
