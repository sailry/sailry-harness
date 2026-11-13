package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func providerCredentialFixtures(t *testing.T) {
	t.Run("rotated credentials retain their keys without exhausting capacity", func(t *testing.T) {
		_, node, storage := schemaFixture(t)
		ctx := t.Context()
		provider := providerFixture()
		var first protocol.CredentialRef
		for index := 0; index < 257; index++ {
			secret := protocol.NewSecret(fmt.Sprintf("fixture-key-%d", index))
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, ExpectedRevision: provider.Revision, Secret: &secret}}}, nil)
				if err == nil {
					provider = output.Data.(protocol.ConversationProvider)
				}
				return err
			}); err != nil {
				t.Fatalf("key rotation %d: %v", index, err)
			}
			if index == 0 {
				first = *provider.Credential
			}
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			key, err := resolveProviderKey(ctx, db, node, first, provider.ID)
			if err != nil || key == nil || key.Expose() != "fixture-key-0" {
				return fmt.Errorf("captured key was lost: %v", err)
			}
			credentials, err := providerCredentials(ctx, db)
			if err != nil || len(credentials) != 257 {
				return fmt.Errorf("retained credentials: %d, %v", len(credentials), err)
			}
			another, secret := providerFixture(), protocol.NewSecret("another-key")
			_, _, err = ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: another, Secret: &secret}}}, nil)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	})

	t.Run("capacity counts current providers and releases unused removed selections", func(t *testing.T) {
		profile, node, storage := schemaFixture(t)
		ctx := t.Context()
		providers := make([]protocol.ConversationProvider, 256)
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			secret := protocol.NewSecret("fixture-key")
			for index := range providers {
				provider := providerFixture()
				output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}}}, nil)
				if err != nil {
					return err
				}
				providers[index] = output.Data.(protocol.ConversationProvider)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		add := func() error {
			return storage.Node.Write(ctx, func(db *sql.Conn) error {
				provider, secret := providerFixture(), protocol.NewSecret("new-key")
				_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}}}, nil)
				return err
			})
		}
		if err := add(); !providerFixtureFault(err, protocol.ErrorCodeBusy) {
			t.Fatalf("a 257th selected credential was admitted: %v", err)
		}
		// Replacing a selected key does not need a new slot, even at capacity.
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			secret := protocol.NewSecret("rotated-key")
			output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: providers[0], ExpectedRevision: providers[0].Revision, Secret: &secret}}}, nil)
			if err == nil {
				providers[0] = output.Data.(protocol.ConversationProvider)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model, config.Credential = providers[0].ID, "fixture", providers[0].Credential
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			if _, _, err := createSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Config: &config}); err != nil {
				return err
			}
			_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "remove_provider", Data: &protocol.CommandRemoveProviderPayload{Provider: providers[0].ID, ExpectedRevision: providers[0].Revision}}}, nil)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if err := add(); !providerFixtureFault(err, protocol.ErrorCodeBusy) {
			t.Fatalf("a session's unlisted provider stopped counting: %v", err)
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "remove_provider", Data: &protocol.CommandRemoveProviderPayload{Provider: providers[1].ID, ExpectedRevision: providers[1].Revision}}}, nil)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if err := add(); err != nil {
			t.Fatalf("an unused removed selection did not release capacity: %v", err)
		}
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			key, err := resolveProviderKey(ctx, db, node, *providers[1].Credential, providers[1].ID)
			if err != nil || key == nil || key.Expose() != "fixture-key" {
				return fmt.Errorf("removal destroyed a retained key: %v", err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
}
