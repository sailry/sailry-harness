package admission_test

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLegacySecretRequestReplay(t *testing.T) {
	t.Parallel()
	for _, format := range []string{"raw", "digest"} {
		for _, status := range []string{"admitted", "completed", "unknown"} {
			t.Run(format+"/"+status, func(t *testing.T) {
				t.Parallel()
				ctx := t.Context()
				node, caller := protocol.NodeID{1}, protocol.NodeID{2}
				profile := filepath.Join(t.TempDir(), "profile")
				storage, err := store.Open(ctx, profile, node)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() {
					if storage != nil {
						_ = storage.Close()
					}
				})
				const secretText = "retained-request-secret-fixture"
				command := func(value string) protocol.Command {
					secret := protocol.NewSecret(value)
					payload := protocol.DefaultCommandSavePluginSettingsPayload()
					payload.Package.Name = "legacy-fixture"
					payload.Secrets["token"] = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: &secret}
					return protocol.Command{Kind: "save_plugin_settings", Data: &payload}
				}
				request := protocol.NewRequest(node, command(secretText))
				request.ID = protocol.NewRequestID()
				body, err := json.Marshal(request)
				if err != nil {
					t.Fatal(err)
				}
				original := body
				if format == "digest" {
					original = admission.Body(request.Command, body)
				}
				output := protocol.Output{Kind: "notifications_updated"}
				result, err := json.Marshal(protocol.Result[protocol.Output, protocol.Fault]{Ok: &output})
				if err != nil {
					t.Fatal(err)
				}
				if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					_, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,?4,?5)", caller[:], request.ID.String(), original, status, string(result))
					return err
				}); err != nil {
					t.Fatal(err)
				}
				if err := storage.Close(); err != nil {
					t.Fatal(err)
				}
				storage, err = store.Open(ctx, profile, node)
				if err != nil {
					t.Fatal(err)
				}
				var previous *link.Response
				if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					var err error
					previous, err = admission.Admit(ctx, db, caller, request, body)
					return err
				}); err != nil {
					t.Fatal(err)
				}
				if previous == nil {
					t.Fatal("retained request was admitted again")
				}
				if status == "completed" {
					if previous.Fault != nil || previous.Output.Kind != output.Kind {
						t.Fatal("retained completion did not replay")
					}
				} else if previous.Fault == nil || previous.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
					t.Fatal("retained in-flight request lost its unknown outcome")
				}
				changed := request
				changed.Command = command("different-fixture-secret")
				changedBody, err := json.Marshal(changed)
				if err != nil {
					t.Fatal(err)
				}
				err = storage.Node.Write(ctx, func(db *sql.Conn) error {
					_, err := admission.Admit(ctx, db, caller, changed, changedBody)
					return err
				})
				var conflict *protocol.Fault
				if !errors.As(err, &conflict) || conflict.Code != protocol.ErrorCodeConflict {
					t.Fatal("changed request content did not conflict")
				}
				var outcome protocol.RequestOutcome
				if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
					var retained []byte
					if err := db.QueryRowContext(ctx, "SELECT body FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&retained); err != nil {
						return err
					}
					if !bytes.Equal(retained, original) {
						return errors.New("replay changed the retained request bytes")
					}
					var count int
					if err := db.QueryRowContext(ctx, "SELECT count(*) FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&count); err != nil {
						return err
					}
					if count != 1 {
						return errors.New("replay duplicated the retained request")
					}
					var err error
					outcome, err = admission.Inspect(ctx, db, caller, request.ID, admission.Digest(body))
					return err
				}); err != nil {
					t.Fatal(err)
				}
				if outcome.Kind != status {
					t.Fatalf("inspection state: got %s, want %s", outcome.Kind, status)
				}
				inspection, err := json.Marshal(outcome)
				if err != nil {
					t.Fatal(err)
				}
				var publicReply any = previous.Fault
				if previous.Fault == nil {
					publicReply = previous.Output
				}
				reply, err := json.Marshal(publicReply)
				if err != nil {
					t.Fatal(err)
				}
				if bytes.Contains(inspection, []byte(secretText)) || bytes.Contains(reply, []byte(secretText)) || bytes.Contains([]byte(conflict.Error()), []byte(secretText)) {
					t.Fatal("retained secret appeared in the replay response")
				}
			})
		}
	}
}
