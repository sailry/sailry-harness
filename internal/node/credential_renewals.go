package node

import (
	"context"
	"database/sql"
	"strconv"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// RuntimeCredentialsChanged wakes model steps only after their replacement
// credentials have committed. It also recalculates controller renewal timers.
func (node *Node) RuntimeCredentialsChanged() {
	node.authChanged()
}

func (node *Node) startCredentialRenewals() {
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		node.serveCredentialRenewals(node.ctx, node.dispatch.clock)
	}()
}

func (node *Node) serveCredentialRenewals(ctx context.Context, clock clockwork.Clock) {
	// Persisted pending, uncertain and rejected grants are excluded by the
	// store. Other due grants can retry after a connection failure, even when
	// no local model request arrives to drive renewal.
	type attempt struct {
		due   uint64
		after time.Time
	}
	attempted := map[string]attempt{}
	for ctx.Err() == nil {
		node.auth.mu.Lock()
		changed, closed := node.auth.changed, node.auth.closed
		node.auth.mu.Unlock()
		if closed {
			return
		}
		var candidates []store.CredentialRenewal
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			candidates, err = store.ReadCredentialRenewals(ctx, db, node.ID())
			return err
		})
		var next *time.Time
		pending := false
		if err != nil {
			retry := clock.Now().Add(time.Minute)
			next = &retry
		} else {
			seen := make(map[string]struct{}, len(candidates))
			for _, candidate := range candidates {
				key := candidate.User.ID.String() + ":provider:" + candidate.Credential.ID.String()
				if candidate.Connection != nil {
					key = candidate.User.ID.String() + ":connector:" + candidate.Connection.ID.String() + ":" + strconv.FormatUint(candidate.Connection.AuthRevision, 10)
				}
				seen[key] = struct{}{}
				due := time.UnixMilli(int64(candidate.DueMs))
				if previous, ok := attempted[key]; ok && previous.due == candidate.DueMs && previous.after.After(due) {
					due = previous.after
				}
				if due.After(clock.Now()) {
					if next == nil || due.Before(*next) {
						next = &due
					}
					continue
				}
				owner := store.WithUser(ctx, candidate.User)
				if candidate.Provider != nil {
					_ = node.renewProviderCredential(owner, *candidate.Provider, candidate.Credential, true)
				} else if candidate.Connection != nil {
					_, _ = node.connectorAccess(owner, *candidate.Connection, candidate.Declaration)
				}
				attempted[key] = attempt{due: candidate.DueMs, after: clock.Now().Add(time.Minute)}
				pending = true
			}
			for key := range attempted {
				if _, ok := seen[key]; !ok {
					delete(attempted, key)
				}
			}
		}
		if pending {
			continue
		}
		var timer clockwork.Timer
		var due <-chan time.Time
		if next != nil {
			timer = clock.NewTimer(max(next.Sub(clock.Now()), time.Millisecond))
			due = timer.Chan()
		}
		select {
		case <-ctx.Done():
		case <-changed:
		case <-due:
		}
		if timer != nil {
			timer.Stop()
		}
	}
}
