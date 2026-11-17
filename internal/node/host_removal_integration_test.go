//go:build integration

package node

import (
	"context"
	"database/sql"
	"errors"
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
			} else {
				// Pairing alone is not a live execution connection. Notification
				// may reuse this authenticated connection, but may not dial one.
				shared := client.New(endpoint.Remote(address))
				_, err := shared.ReadResourceCatalog(ctx)
				_ = shared.Close()
				if err != nil {
					t.Fatal(err)
				}
				localFailure := errors.New("local save failed")
				if _, err := client.RemoveHost(ctx, endpoint, address, func(context.Context) error { return localFailure }); !errors.Is(err, localFailure) {
					t.Fatalf("local failure was hidden: %v", err)
				}
				if !endpoint.Direction(address.ID).AllowsOutbound() {
					t.Fatal("failed local save removed trust")
				}
				history, err := endpoint.HostRemovals(ctx)
				if err != nil || len(history) != 1 || history[0].State != "pending" {
					t.Fatalf("failed local save dispatched notification: %+v: %v", history, err)
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
			if offline {
				if attempt.State != "failed" {
					t.Fatalf("offline host was queued for notification: %+v", attempt)
				}
			} else {
				if attempt.State != "unknown" {
					t.Fatalf("live notification was not queued: %+v", attempt)
				}
				for {
					revoked := false
					err := host.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
						_, err := store.CallerUser(ctx, db, host.ID(), self.ID)
						var failure *protocol.Fault
						revoked = errors.As(err, &failure) && failure.Code == protocol.ErrorCodePermissionDenied
						if revoked {
							return nil
						}
						return err
					})
					if err != nil {
						t.Fatal(err)
					}
					history, err := endpoint.HostRemovals(ctx)
					if err != nil || len(history) != 1 {
						t.Fatalf("notification history: %+v: %v", history, err)
					}
					// Self-revocation can close the authenticated connection before
					// its completion arrives, so an unknown result remains honest.
					settled := history[0].State != "unknown" || history[0].Fault != nil
					if revoked && settled {
						break
					}
					select {
					case <-ctx.Done():
						t.Fatalf("remote notification did not finish: revoked=%t, history=%+v", revoked, history)
					case <-time.After(10 * time.Millisecond):
					}
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
