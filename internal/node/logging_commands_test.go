package node

import (
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Every registered command is recorded in full unless this package names it
// as one that changes nothing, and a record names typed identifiers only.
func TestCommandLogClassification(t *testing.T) {
	t.Parallel()
	registered, counts := map[string]bool{}, map[observation]int{}
	durable := 0
	for _, kind := range protocol.CommandKinds() {
		registered[kind] = true
		command := protocol.Command{Kind: kind}
		class, listed := transientCommands[kind]
		switch {
		case command.Durable() && listed:
			t.Errorf("%s has durable admission and may not be classified as %d: it is always recorded", kind, class)
		case command.Durable():
			durable++
			if commandObservation(command) != changing {
				t.Errorf("durable %s is not recorded", kind)
			}
		case !listed:
			t.Errorf("%s is neither durable nor classified in transientCommands: name it as changing, a read, a transfer or interactive", kind)
		default:
			counts[class]++
			if commandObservation(command) != class {
				t.Errorf("%s is observed as %d, classified %d", kind, commandObservation(command), class)
			}
		}
	}
	for kind := range transientCommands {
		if !registered[kind] {
			t.Errorf("transientCommands names %s, which is not a registered command", kind)
		}
	}
	if total := durable + counts[changing] + counts[traced] + counts[slow] + counts[readOnly] + counts[transfer] + counts[interactive]; total != len(registered) || durable == 0 || counts[changing] == 0 {
		t.Errorf("classified %d of %d commands", total, len(registered))
	}
	t.Logf("%d commands: %d durable and %d transient recorded as changes, %d traced reads, %d slow reads, %d read-only, %d transfers, %d interactive",
		len(registered), durable, counts[changing], counts[traced], counts[slow], counts[readOnly], counts[transfer], counts[interactive])

	// A schedule's reads are the one payload-dependent admission.
	if class := commandObservation(protocol.Command{Kind: "dispatch", Data: protocol.CommandDispatchPayload{Action: protocol.DispatchCommand{Kind: "list_jobs"}}}); class != readOnly {
		t.Errorf("a schedule read is observed as %d", class)
	}
	if class := commandObservation(protocol.Command{Kind: "unregistered_command"}); class != changing {
		t.Errorf("an unknown command is observed as %d, want a full record", class)
	}

	// Identifiers are read by type: no text, path, endpoint or secret of a
	// payload can become a field.
	const canary = "SECRET"
	secret := protocol.NewSecret(canary + "-credential")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Endpoint = protocol.NewProviderID(), canary+" name", "https://"+canary+".test/v1"
	session, turn, worktree, owner := protocol.NewSessionID(), protocol.NewTurnID(), protocol.NewWorktreeID(), protocol.NewProviderID()
	backup := protocol.DefaultBackupUpdate()
	backup.Passphrase = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: &secret}
	for name, test := range map[string]struct {
		data any
		want string
	}{
		"save_provider": {protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}, ""},
		"put_provider":  {&protocol.CommandPutProviderPayload{Provider: provider}, ""},
		"put_credential": {&protocol.CommandPutCredentialPayload{ID: protocol.NewCredentialID(), Provider: owner, Secret: secret},
			"provider=" + owner.String()},
		"save_database": {protocol.CommandSaveDatabasePayload{Profile: protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: canary}, Password: &secret}, ""},
		"save_ssh": {protocol.CommandSaveSshPayload{Profile: protocol.SshProfile{ID: protocol.NewSshID(), Host: canary + ".test", Username: canary},
			Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: secret}}}, ""},
		"save_backup_settings": {&backup, ""},
		"submit_turn": {protocol.CommandSubmitTurnPayload{Session: session, Message: protocol.ConversationInput{Text: canary + " prompt"}},
			"session=" + session.String()},
		"guide_turn": {&protocol.CommandGuideTurnPayload{Recipient: turn, Message: protocol.ConversationInput{Text: canary}}, "turn=" + turn.String()},
		"write_file": {&protocol.CommandWriteFilePayload{Worktree: worktree, Path: canary + "/notes.txt", Text: canary + " contents"},
			"worktree=" + worktree.String()},
		"request_provider_http": {&protocol.CommandRequestProviderHttpPayload{Provider: owner, Request: canary, Input: map[string]string{canary: canary}},
			"provider=" + owner.String()},
		"unit": {nil, ""},
	} {
		got := strings.TrimSpace(fmt.Sprintln(commandIdentifiers(test.data)))
		if strings.Contains(got, canary) || got != "["+test.want+"]" {
			t.Errorf("%s identifiers = %s, want [%s]", name, got, test.want)
		}
	}
}
