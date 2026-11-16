package store

import (
	"context"
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConnectorCaptureSurvivesUserMerge(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		source, err := CreateUser(ctx, db, "Source")
		if err != nil {
			return err
		}
		target, err := CreateUser(ctx, db, "Target")
		if err != nil {
			return err
		}
		third, err := CreateUser(ctx, db, "Third")
		if err != nil {
			return err
		}
		sourceCtx, targetCtx := WithUser(ctx, source), WithUser(ctx, target)
		info := pluginTestInfo("connector-merge", 'a')
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		token := "token"
		for _, name := range []string{"conflict", "moved", "target-only"} {
			declaration := protocol.DefaultPluginConnectorDeclaration()
			declaration.ID, declaration.Display.Label, declaration.Accounts = name, name, protocol.PluginConnectorAccountsSingle
			declaration.Origins = []string{"https://api.example.test"}
			declaration.Fields = []protocol.PluginConnectorField{{Name: token, Title: "Token", Secret: true}}
			declaration.Auth = []protocol.PluginConnectorAuth{{Kind: "secret", Data: protocol.PluginConnectorAuthSecretPayload{Fields: []string{token}}}}
			declaration.Inject = []protocol.PluginConnectorInjection{{Header: &token, Field: &token}}
			extension.Connectors = append(extension.Connectors, declaration)
		}
		info.Extension = &extension
		var reference protocol.PluginReference
		for _, owner := range []context.Context{sourceCtx, targetCtx, WithUser(ctx, third)} {
			installed, err := InstallPlugin(owner, db, info, 0, true)
			if err != nil {
				return err
			}
			reference = plugins.PackageReference(installed)
		}
		connect := func(owner context.Context, name, secret string) (protocol.PluginConnectorID, error) {
			capture, err := PrepareConnectorLogin(owner, db, protocol.NewRequestID(), protocol.CommandStartConnectorLoginPayload{Package: reference, Connector: name, Method: protocol.PluginConnectorKindSecret, Input: protocol.PluginConnectorInput{Secrets: map[string]protocol.Secret{token: protocol.NewSecret(secret)}}})
			if err != nil {
				return protocol.PluginConnectorID{}, err
			}
			if err := StartConnectorLogin(owner, db, capture); err != nil {
				return protocol.PluginConnectorID{}, err
			}
			value, err := CompleteConnectorLogin(owner, db, capture, nil, nil, false)
			return value.ID, err
		}
		sourceConflict, err := connect(sourceCtx, "conflict", "source-secret")
		if err != nil {
			return err
		}
		moved, err := connect(sourceCtx, "moved", "moved-secret")
		if err != nil {
			return err
		}
		if _, err := connect(targetCtx, "conflict", "target-secret"); err != nil {
			return err
		}
		if _, err := connect(targetCtx, "target-only", "target-only-secret"); err != nil {
			return err
		}
		if _, err := MergeIdentityUsers(ctx, db, source.ID, target.ID); err != nil {
			return err
		}
		captured := WithCapturedUser(targetCtx, source.ID)
		for name, want := range map[string]string{"conflict": "source-secret", "moved": "moved-secret"} {
			record, _, err := ConnectorAuthorization(captured, db, reference, name, nil, nil)
			if err != nil || record.Secrets[token].Expose() != want {
				return fmt.Errorf("captured connector %s changed identity: %v", name, err)
			}
		}
		if _, _, err := ConnectorAuthorization(captured, db, reference, "target-only", nil, nil); err == nil {
			return fmt.Errorf("capture consumed a target-only connector")
		}
		if _, err := ReadConnectorRecord(targetCtx, db, sourceConflict); err == nil {
			return fmt.Errorf("public target read a source conflict archive")
		}
		record, err := ReadConnectorRecord(captured, db, moved)
		if err != nil {
			return err
		}
		record.Revision++
		record.Fields = map[string]string{"note": "retained callback"}
		if err := saveConnectorRecord(captured, db, record); err != nil {
			return err
		}
		current, err := ReadConnectorRecord(targetCtx, db, moved)
		if err != nil || current.Fields["note"] != "retained callback" {
			return fmt.Errorf("captured update lost the moved live row: %v", err)
		}
		if _, err := MergeIdentityUsers(ctx, db, target.ID, third.ID); err != nil {
			return err
		}
		chained := WithCapturedUser(WithUser(ctx, third), target.ID)
		if _, err := ReadConnectorRecord(chained, db, moved); err != nil {
			return fmt.Errorf("second merge lost intermediate admitted ownership: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
