//go:build integration

package node

import (
	"context"
	"database/sql"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostRemovalNotifiesRemoteAndAlwaysForgetsLocally(t *testing.T) {
	for _, offline := range []bool{false, true} {
		name := "online"
		if offline {
			name = "offline"
		}
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 35*time.Second)
			defer cancel()
			scope := link.NetworkScope{Bind: "127.0.0.1:0"}
			host, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "host"), Options{Network: &scope, MCP: fixtureMCPOptions()})
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if err := host.Stop(context.Background()); err != nil {
					t.Error(err)
				}
			}()
			profile := filepath.Join(t.TempDir(), "controller")
			endpoint, err := link.OpenController(ctx, profile, scope)
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = endpoint.Close() }()
			offer, err := host.Link().Invite(ctx)
			if err != nil {
				t.Fatal(err)
			}
			defer offer.Close()
			address, err := endpoint.Pair(ctx, offer.Ticket())
			if err != nil {
				t.Fatal(err)
			}
			if err := offer.Paired(ctx); err != nil {
				t.Fatal(err)
			}
			self, err := endpoint.Address(ctx)
			if err != nil {
				t.Fatal(err)
			}
			if offline {
				if err := host.Stop(ctx); err != nil {
					t.Fatal(err)
				}
			}
			localCalled := false
			attempt, err := client.RemoveHost(ctx, endpoint, address, func(local context.Context) error {
				localCalled = true
				return endpoint.SetTrust(local, address.ID, false)
			})
			if err != nil || !localCalled || endpoint.Direction(address.ID).Valid() {
				t.Fatalf("local removal failed with offline=%t: %v", offline, err)
			}
			if attempt.Request.ID == (protocol.RequestID{}) || attempt.Request.Target != host.ID() {
				t.Fatal("original remote removal request was not captured")
			}
			if !offline {
				err := host.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
					_, err := store.CallerUser(ctx, db, host.ID(), self.ID)
					if err == nil {
						t.Error("remote retained its removed controller")
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			}
			if err := endpoint.Close(); err != nil {
				t.Fatal(err)
			}
			endpoint, err = link.OpenController(ctx, profile, scope)
			if err != nil {
				t.Fatal(err)
			}
			history, err := endpoint.HostRemovals(ctx)
			if err != nil || len(history) != 1 || history[0].Request.ID != attempt.Request.ID {
				t.Fatalf("reopen lost original self-removal: %+v %v", history, err)
			}
		})
	}
}
