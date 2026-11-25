package node

import (
	"context"
	"errors"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
)

func TestWorkspaceFactoryLifecycle(t *testing.T) {
	type service struct{ profile string }
	var opened, bound, closed []*service
	factory := moduleapi.WorkspaceFactory[*service]{Open: func(options moduleapi.WorkspaceOptions) (*service, func() error, error) {
		if options.Context == nil || options.Lock == nil || options.Profile == "" {
			t.Fatal("factory did not receive the Node's workspace settings")
		}
		value := &service{profile: options.Profile}
		opened = append(opened, value)
		return value, func() error {
			closed = append(closed, value)
			return nil
		}, nil
	}}
	bind := func(value *service) moduleapi.WorkspaceBindings {
		bound = append(bound, value)
		return moduleapi.WorkspaceBindings{}
	}
	git := serviceTestWorkspaceModule{serviceTestModule: "fixture-repository", services: moduleapi.WorkspaceServices{Git: true, Bind: factory.Bind(bind)}}
	worktrees := serviceTestWorkspaceModule{serviceTestModule: "fixture-worktrees", services: moduleapi.WorkspaceServices{Worktrees: true, Bind: factory.Bind(bind)}}
	first := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{git, worktrees}})
	second := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{worktrees}})
	if len(opened) != 2 || opened[0].profile == opened[1].profile || !slices.Equal(bound, []*service{opened[0], opened[0], opened[1]}) {
		t.Fatalf("factory opens = %v, bindings = %v", opened, bound)
	}
	for _, node := range []*Node{first, second, first} {
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
	}
	if !slices.Equal(opened, closed) {
		t.Fatalf("shared services were not closed exactly once: %v", closed)
	}

	failure := errors.New("fixture startup failed")
	var failedClosed bool
	badFactory := moduleapi.WorkspaceFactory[*service]{Open: func(moduleapi.WorkspaceOptions) (*service, func() error, error) {
		return nil, func() error { failedClosed = true; return nil }, failure
	}}
	bad := serviceTestWorkspaceModule{serviceTestModule: "fixture-failure", services: moduleapi.WorkspaceServices{Bind: badFactory.Bind(bind)}}
	profile := filepath.Join(t.TempDir(), "profile")
	node, err := StartWithOptions(t.Context(), profile, Options{Assembly: moduleapi.Assembly{Modules: []moduleapi.Module{git, bad}}})
	if node != nil || !errors.Is(err, failure) {
		t.Fatalf("factory startup failure = %v, %v", node, err)
	}
	if len(opened) != 3 || !slices.Equal(opened, closed) || !failedClosed || len(bound) != 4 {
		t.Fatalf("failed startup did not close its services: opens %d, successful bindings %d, closes %d, partial close %t", len(opened), len(bound), len(closed), failedClosed)
	}
}
