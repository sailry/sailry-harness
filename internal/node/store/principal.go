package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type userContextKey struct{}

// WithUser carries a Node-verified principal through admitted asynchronous work.
// A peer never supplies this value in its request body.
func WithUser(ctx context.Context, user protocol.User) context.Context {
	return context.WithValue(admission.WithUser(ctx, user.ID), userContextKey{}, user)
}

// CurrentUser defaults only trusted internal maintenance to the local user.
// Public Node entry points resolve CallerUser before they call a service.
func CurrentUser(ctx context.Context, db *sql.Conn) (protocol.User, error) {
	if user, ok := ctx.Value(userContextKey{}).(protocol.User); ok {
		return ResolveUser(ctx, db, user.ID)
	}
	return LocalUser(ctx, db)
}

// UserFromContext returns only the principal already verified by Node.
func UserFromContext(ctx context.Context) (protocol.User, bool) {
	user, ok := ctx.Value(userContextKey{}).(protocol.User)
	return user, ok
}
