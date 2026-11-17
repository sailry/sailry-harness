//go:build integration

package node

import (
	"context"
	"database/sql"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Re-pairing keeps the controller's existing host relationship. Only the host
// revoked access, so ordinary peer discovery cannot recreate the follower.
func TestConfigurationFollowRePairRestoresResourceDirectoryIntegration(t *testing.T) {
	for _, restart := range []bool{false, true} {
		name := "terminated_observation"
		if restart {
			name = "rejected_initial_observation"
		}
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
			defer cancel()
			scope := link.NetworkScope{Bind: "127.0.0.1:0"}
			environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
			start := func(path string) *Node {
				t.Helper()
				node, err := StartWithOptions(ctx, path, Options{Network: &scope, Environment: &environment, MCP: fixtureMCPOptions()})
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() {
					if err := node.Stop(context.Background()); err != nil {
						t.Error(err)
					}
				})
				return node
			}
			controllerPath := filepath.Join(t.TempDir(), "controller")
			controller, host := start(controllerPath), start(filepath.Join(t.TempDir(), "host"))
			pair := func() {
				t.Helper()
				offer, err := host.Link().Invite(ctx)
				if err != nil {
					t.Fatal(err)
				}
				defer offer.Close()
				if _, err := controller.Link().Pair(ctx, offer.Ticket()); err != nil {
					t.Fatal(err)
				}
				if err := offer.Paired(ctx); err != nil {
					t.Fatal(err)
				}
			}
			wait := func(label string, ready func() bool) {
				t.Helper()
				ticker := time.NewTicker(20 * time.Millisecond)
				defer ticker.Stop()
				for !ready() {
					select {
					case <-ctx.Done():
						t.Logf("relationship directions: controller=%q host=%q", controller.Link().Direction(host.ID()), host.Link().Direction(controller.ID()))
						if observer := controller.controllerClients()[host.ID()]; observer != nil {
							t.Logf("shared observer: connected=%t error=%v", observer.Connected(), observer.Err())
						} else {
							t.Log("shared observer is missing")
						}
						t.Fatalf("%s: %v", label, context.Cause(ctx))
					case <-ticker.C:
					}
				}
			}
			project := func(name string) protocol.Project {
				t.Helper()
				return commandFixture(t, host, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: name, Path: t.TempDir()}}).Data.(protocol.Project)
			}
			cached := func(projects ...protocol.Project) bool {
				t.Helper()
				var sources []protocol.ResourceSource
				if err := controller.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
					var err error
					sources, err = store.ReadResourceCatalogSources(ctx, db, controller.ID())
					return err
				}); err != nil {
					t.Fatal(err)
				}
				if len(projects) == 0 {
					return !slices.ContainsFunc(sources, func(source protocol.ResourceSource) bool { return source.Node == host.ID() })
				}
				for _, source := range sources {
					if source.Node != host.ID() || source.Fault != nil || len(source.Projects) != len(projects) {
						continue
					}
					for _, want := range projects {
						if !slices.ContainsFunc(source.Projects, func(got protocol.Project) bool { return got.ID == want.ID }) {
							return false
						}
					}
					return true
				}
				return false
			}
			first := project("Before revocation")
			pair()
			wait("initial shared directory", func() bool { return cached(first) })
			if restart {
				// Keep authentication while removing execution access. A wholly
				// revoked peer is rejected before the initial wire subscription,
				// where transport closure can surface as Unavailable instead.
				if err := host.Link().SetDirection(ctx, controller.ID(), link.DirectionHost); err != nil {
					t.Fatal(err)
				}
				if direction := host.Link().Direction(controller.ID()); direction != link.DirectionHost || direction.AllowsInbound() {
					t.Fatalf("execution access was not removed: %q", direction)
				}
			} else {
				output := commandFixture(t, host, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device",
					Data: protocol.RevokeDevice{Device: controller.ID(), ConfirmLast: true}}})
				if output.Kind != "device_revoked" {
					t.Fatalf("revoke controller: %+v", output)
				}
			}
			denied := func() bool {
				observer := controller.controllerClients()[host.ID()]
				if observer == nil || observer.Connected() {
					return false
				}
				fault := resourceFault(observer.Err())
				return fault != nil && fault.Code == protocol.ErrorCodePermissionDenied
			}
			wait("revoked observation", denied)
			wait("revoked directory cleared", func() bool { return cached() })
			if restart {
				if err := controller.Stop(context.Background()); err != nil {
					t.Fatal(err)
				}
				controller = start(controllerPath)
				wait("initial observation denied", denied)
			}
			if !controller.Link().Direction(host.ID()).AllowsOutbound() {
				t.Fatal("fixture removed the controller's retained host relationship")
			}
			observer := controller.controllerClients()[host.ID()]
			second := project("Created while revoked")
			pair()
			wait("same shared observer recovered", func() bool {
				return observer == controller.controllerClients()[host.ID()] && observer.Connected() && observer.Err() == nil && cached(first, second)
			})
			third := project("Created after re-pairing")
			wait("recovered observer follows new events", func() bool { return cached(first, second, third) })
			// The normal local Client sees the same authoritative directory; it
			// does not create or refresh a separate direct-host observer.
			local := client.New(link.NewLocal(controller.ID(), controller.ID(), controller))
			defer local.Close()
			catalog, err := local.ReadResourceCatalog(ctx)
			if err != nil || !slices.ContainsFunc(catalog.Sources, func(source protocol.ResourceSource) bool {
				return source.Node == host.ID() && source.Fault == nil && len(source.Projects) == 3
			}) {
				t.Fatalf("re-paired directory remains unavailable: %+v: %v", catalog, err)
			}
		})
	}
}
