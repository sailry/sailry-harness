package client

import (
	"context"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ProviderCredential reads the active Node's binding for one provider under the
// projection lock without copying model catalogs or unrelated session data.
func (client *Client) ProviderCredential(id protocol.ProviderID) (protocol.Authentication, uint64, protocol.CredentialID, bool) {
	client.mu.RLock()
	defer client.mu.RUnlock()
	if client.connected && client.project.snapshot != nil {
		for i := range client.project.snapshot.Providers {
			p := &client.project.snapshot.Providers[i]
			if p.ID == id && p.Credential != nil {
				return p.Authentication, p.Revision, p.Credential.ID, true
			}
		}
	}
	return "", 0, protocol.CredentialID{}, false
}

// RefreshProviders reconciles a confirmed configuration change. Older Nodes
// only expose this directory through their complete snapshot.
func (client *Client) RefreshProviders(ctx context.Context) error {
	client.mu.RLock()
	generation := client.project.generation
	supported := client.project.snapshot != nil && client.project.snapshot.ProviderDirectory
	client.mu.RUnlock()
	if !supported {
		return client.Refresh(ctx)
	}
	output, err := client.read(ctx, protocol.Command{Kind: "read_provider_directory"})
	if err != nil {
		return err
	}
	directory, err := payload[protocol.ProviderDirectory](output.Data)
	if err != nil || output.Kind != "provider_directory" || directory.Node != client.Target() {
		return fault(protocol.ErrorCodeInvalidRequest, "invalid provider directory response")
	}
	client.mu.Lock()
	changed, err := client.project.applyProviderDirectory(generation, directory)
	if changed {
		client.revision++
	}
	client.mu.Unlock()
	if changed {
		client.changed()
	}
	return err
}

func (projection *Projection) applyProviderDirectory(generation uint64, directory protocol.ProviderDirectory) (bool, error) {
	snapshot := projection.snapshot
	if projection.generation != generation || projection.recovery || snapshot == nil || directory.Cursor < max(snapshot.Cursor, projection.providerCursor) {
		return false, nil
	}
	providersChanged := !reflect.DeepEqual(snapshot.Providers, directory.Providers)
	quotasChanged := !reflect.DeepEqual(snapshot.ProviderQuotas, directory.Quotas)
	if providersChanged {
		if err := projection.changes.observe(protocol.Event{Kind: "provider_changed"}, directory.Cursor, nil, false); err != nil {
			return false, err
		}
		snapshot.Providers = copyValue(directory.Providers)
		projection.providers++
	}
	if quotasChanged {
		snapshot.ProviderQuotas = copyValue(directory.Quotas)
	}
	projection.providerCursor = directory.Cursor
	return providersChanged || quotasChanged, nil
}
