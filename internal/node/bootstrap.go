package node

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// Profile holds the shared Desktop and Host storage lifecycle.
type Profile struct {
	Ownership *core.Profile
	Identity  link.Identity
	Store     *store.Store
}

func OpenProfile(ctx context.Context, path string) (*Profile, error) {
	ownership, err := core.AcquireProfile(path)
	if err != nil {
		return nil, err
	}
	identity, err := link.OpenIdentity(ownership.Path)
	if err != nil {
		_ = ownership.Close()
		return nil, err
	}
	database, err := store.Open(ctx, ownership.Path, identity.ID())
	if err != nil {
		_ = ownership.Close()
		return nil, err
	}
	return &Profile{Ownership: ownership, Identity: identity, Store: database}, nil
}

func (profile *Profile) Close() error {
	return errors.Join(profile.Store.Close(), profile.Ownership.Close())
}
