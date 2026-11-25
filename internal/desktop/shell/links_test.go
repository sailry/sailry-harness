package shell

import (
	"errors"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLinkRegistryOwnsExactRoutesPerWindow(t *testing.T) {
	route := LinkRoute{Kind: LinkScheme, Name: "destination"}
	refRoute := LinkRoute{Kind: LinkReference, Name: route.Name}
	denial := errors.New("rejected")
	var explicit, fallback, reference int
	assembly := Assemble(func(a *Assembly) {
		a.RegisterLinkHandler(route, func(*ui.Context, LinkTarget, LinkActions) error { explicit++; return denial })
	})
	window := func() *App { return NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly) }
	one, two := window(), window()
	if err := one.RegisterDefaultLinkHandler(route, func(*ui.Context, LinkTarget, LinkActions) error { fallback++; return nil }); err != nil {
		t.Fatal(err)
	}
	release, err := one.RegisterLinkHandler(refRoute, func(*ui.Context, LinkTarget, LinkActions) error { reference++; return nil })
	if err != nil {
		t.Fatal(err)
	}
	if handled, err := one.DispatchLink(nil, LinkTarget{Route: route}, LinkActions{}); !handled || !errors.Is(err, denial) || explicit != 1 || fallback != 0 {
		t.Fatalf("denial did not remain terminal: handled %v, error %v, calls %d/%d", handled, err, explicit, fallback)
	}
	if handled, err := one.DispatchLink(nil, LinkTarget{Route: refRoute}, LinkActions{}); !handled || err != nil || reference != 1 {
		t.Fatal("scheme and reference of the same name did not coexist")
	}
	release()
	if handled, _ := one.DispatchLink(nil, LinkTarget{Route: refRoute}, LinkActions{}); handled {
		t.Fatal("released route still handled links")
	}
	one.CloseWindowBindings()
	if handled, _ := one.DispatchLink(nil, LinkTarget{Route: route}, LinkActions{}); handled {
		t.Fatal("closed window retained a route")
	}
	if handled, _ := two.DispatchLink(nil, LinkTarget{Route: route}, LinkActions{}); !handled || explicit != 2 {
		t.Fatal("closing one window released another's route")
	}
}

func TestLinkRegistryRejectsDuplicateExplicitRoutes(t *testing.T) {
	defer func() {
		if recover() == nil {
			t.Fatal("ambiguous registration was not rejected")
		}
	}()
	register := func(a *Assembly) {
		a.RegisterLinkHandler(LinkRoute{Kind: LinkFile}, func(*ui.Context, LinkTarget, LinkActions) error { return nil })
	}
	Assemble(register, register)
}

func TestLinkRegistryReportsLiveDefaultConflict(t *testing.T) {
	app := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
	route := LinkRoute{Kind: LinkReference, Name: "fixture"}
	var calls int
	if err := app.RegisterDefaultLinkHandler(route, func(*ui.Context, LinkTarget, LinkActions) error { calls++; return nil }); err != nil {
		t.Fatal(err)
	}
	if err := app.RegisterDefaultLinkHandler(route, func(*ui.Context, LinkTarget, LinkActions) error {
		t.Fatal("the conflicting default ran")
		return nil
	}); !linkContributionFault(err, protocol.ErrorCodeRevisionConflict) || calls != 0 {
		t.Fatalf("default conflict invoked a handler or returned the wrong error: %v, calls %d", err, calls)
	}
	if release, err := app.RegisterLinkHandler(route, nil); release != nil || !linkContributionFault(err, protocol.ErrorCodeInvalidRequest) {
		t.Fatalf("invalid live handler: release %t, error %v", release != nil, err)
	}
	if handled, err := app.DispatchLink(nil, LinkTarget{Route: route}, LinkActions{}); !handled || err != nil || calls != 1 {
		t.Fatalf("the original default no longer owns the route: %t, %v, calls %d", handled, err, calls)
	}
}

func TestLinkRegistryPriorityAndReleaseAreIndependentOfRegistrationOrder(t *testing.T) {
	for _, order := range []struct {
		name       string
		priorities []int
	}{
		{"ascending", []int{-5, LinkDefaultPriority, 0, 10}},
		{"descending", []int{10, 0, LinkDefaultPriority, -5}},
	} {
		t.Run(order.name, func(t *testing.T) {
			app := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
			route := LinkRoute{Kind: LinkScheme, Name: "fixture"}
			var calls []int
			makeHandler := func(priority int) LinkHandler {
				return func(*ui.Context, LinkTarget, LinkActions) error { calls = append(calls, priority); return nil }
			}
			release := map[int]func(){}
			for _, priority := range order.priorities {
				handler := makeHandler(priority)
				var err error
				switch priority {
				case LinkDefaultPriority:
					err = app.RegisterDefaultLinkHandler(route, handler)
				case 0:
					release[priority], err = app.RegisterLinkHandler(route, handler)
				default:
					release[priority], err = app.RegisterLinkHandlerWithPriority(route, priority, handler)
				}
				if err != nil {
					t.Fatal(err)
				}
			}
			open := func() {
				t.Helper()
				if handled, err := app.DispatchLink(nil, LinkTarget{Route: route}, LinkActions{}); !handled || err != nil {
					t.Fatalf("dispatch handled %t, error %v", handled, err)
				}
			}
			open()
			release[10]()
			open()
			replacement, err := app.RegisterLinkHandlerWithPriority(route, 10, makeHandler(20))
			if err != nil {
				t.Fatal(err)
			}
			release[10]()
			release[0]()
			open()
			replacement()
			open()
			if want := []int{10, 0, 20, LinkDefaultPriority}; !slices.Equal(calls, want) {
				t.Fatalf("winner sequence %v, want %v", calls, want)
			}
		})
	}
}
