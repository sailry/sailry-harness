package node

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/admission"
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

	// The ledger keeps a digest, never the body, of every durable command
	// whose payload type can hold a protocol.Secret. The set is read from the
	// protocol declarations, through unions, so a new secret field fails here
	// until admission names its command.
	typed := secretCommandKinds(t)
	for _, kind := range []string{"put_credential", "save_provider", "save_ssh", "save_database", "save_backup_settings", "save_plugin_settings", "import_session", "dispatch"} {
		if !typed[kind] {
			t.Errorf("the protocol declarations no longer show a secret in %s: the derivation is broken", kind)
		}
	}
	var digested, free []string
	for _, kind := range protocol.CommandKinds() {
		durable := protocol.Command{Kind: kind}.Durable()
		switch {
		case admission.DigestOnly(kind) && !durable:
			t.Errorf("%s is not durable and has no ledger body to digest", kind)
		case typed[kind] && durable && !admission.DigestOnly(kind):
			t.Errorf("durable %s carries a secret-typed field and is stored raw in the ledger: name it in admission.DigestOnly", kind)
		case admission.DigestOnly(kind) && !typed[kind]:
			free = append(free, kind)
		}
		if admission.DigestOnly(kind) {
			digested = append(digested, kind)
			request := protocol.Request{Command: protocol.Command{Kind: kind}}
			if body := string(admission.Body(request.Command, []byte("SECRET body"))); !strings.HasPrefix(body, "credential-v1:") || len(body) != 78 || strings.Contains(body, "SECRET") {
				t.Errorf("%s ledger body = %q", kind, body)
			}
		}
	}
	// Its server definitions hold a free-form environment and headers rather
	// than a typed secret; it is digested as in the Rust engine.
	if want := []string{"save_plugin_mcp"}; !slices.Equal(free, want) {
		t.Errorf("digested without a secret-typed field: %v, want %v", free, want)
	}
	for kind := range typed {
		if !registered[kind] {
			t.Errorf("the protocol declarations name %s, which is not a registered command", kind)
		}
	}
	if body := admission.Body(protocol.Command{Kind: "rename_session"}, []byte("plain")); string(body) != "plain" {
		t.Errorf("a command without a secret is stored as %q", body)
	}
	t.Logf("%d commands are kept by digest: %v", len(digested), digested)

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
		"import_session": {&protocol.SessionImport{Source: protocol.NodeID{1}, Worktree: &worktree, Provider: provider, Secret: &secret,
			RoleCredentials: []protocol.RoleProviderSecret{{Provider: owner, Secret: secret}}}, "worktree=" + worktree.String()},
		"create_session_at": {&protocol.CommandCreateSessionAtPayload{Target: protocol.NodeID{1}, Worktree: &worktree}, "worktree=" + worktree.String()},
		"unit":              {nil, ""},
	} {
		got := strings.TrimSpace(fmt.Sprintln(commandIdentifiers(test.data)))
		if strings.Contains(got, canary) || got != "["+test.want+"]" {
			t.Errorf("%s identifiers = %s, want [%s]", name, got, test.want)
		}
	}
}

// secretCommandKinds reads the protocol declarations and returns the commands
// whose payload type reaches protocol.Secret through fields, containers,
// generic arguments and union variants.
func secretCommandKinds(t *testing.T) map[string]bool {
	t.Helper()
	packages, err := parser.ParseDir(token.NewFileSet(), "../protocol", func(info fs.FileInfo) bool {
		return !strings.HasSuffix(info.Name(), "_test.go")
	}, parser.SkipObjectResolution)
	if err != nil || packages["protocol"] == nil {
		t.Fatalf("protocol declarations: %v", err)
	}
	types := map[string]ast.Expr{}       // declared type -> its definition
	factories := map[string][]ast.Expr{} // variant table -> the types it constructs
	codecs := map[string][]ast.Node{}    // declared type -> its JSON method bodies
	commands := map[string]ast.Expr{}    // command kind -> payload type
	constructed := func(node ast.Node) (found []ast.Expr) {
		ast.Inspect(node, func(node ast.Node) bool {
			if call, ok := node.(*ast.CallExpr); ok && len(call.Args) == 1 {
				if name, ok := call.Fun.(*ast.Ident); ok && name.Name == "new" {
					found = append(found, call.Args[0])
				}
			}
			return true
		})
		return found
	}
	command := func(key, value ast.Expr) {
		kind, ok := key.(*ast.BasicLit)
		if !ok {
			return
		}
		// A command without a payload has no factory.
		commands[strings.Trim(kind.Value, `"`)] = ast.NewIdent("nil")
		if built := constructed(value); len(built) == 1 {
			commands[strings.Trim(kind.Value, `"`)] = built[0]
		}
	}
	for _, file := range packages["protocol"].Files {
		for _, declaration := range file.Decls {
			switch declaration := declaration.(type) {
			case *ast.GenDecl:
				for _, spec := range declaration.Specs {
					switch spec := spec.(type) {
					case *ast.TypeSpec:
						types[spec.Name.Name] = spec.Type
					case *ast.ValueSpec:
						for index, name := range spec.Names {
							if index >= len(spec.Values) {
								continue
							}
							factories[name.Name] = constructed(spec.Values[index])
							if literal, ok := spec.Values[index].(*ast.CompositeLit); ok && name.Name == "commandVariants" {
								for _, element := range literal.Elts {
									if pair, ok := element.(*ast.KeyValueExpr); ok {
										command(pair.Key, pair.Value)
									}
								}
							}
						}
					}
				}
			case *ast.FuncDecl:
				if declaration.Body == nil {
					continue
				}
				// A variant registered after the table's declaration.
				ast.Inspect(declaration.Body, func(node ast.Node) bool {
					assignment, ok := node.(*ast.AssignStmt)
					if !ok || len(assignment.Lhs) != 1 || len(assignment.Rhs) != 1 {
						return true
					}
					if index, ok := assignment.Lhs[0].(*ast.IndexExpr); ok {
						if table, ok := index.X.(*ast.Ident); ok {
							factories[table.Name] = append(factories[table.Name], constructed(assignment.Rhs[0])...)
							if table.Name == "commandVariants" {
								command(index.Index, assignment.Rhs[0])
							}
						}
					}
					return true
				})
				if declaration.Recv == nil || len(declaration.Recv.List) != 1 {
					continue
				}
				receiver := declaration.Recv.List[0].Type
				if pointer, ok := receiver.(*ast.StarExpr); ok {
					receiver = pointer.X
				}
				if name, ok := receiver.(*ast.Ident); ok && slices.Contains([]string{"MarshalJSON", "UnmarshalJSON", "DecodeJSON"}, declaration.Name.Name) {
					codecs[name.Name] = append(codecs[name.Name], declaration.Body)
				}
			}
		}
	}
	if len(commands) != len(protocol.CommandKinds()) {
		t.Fatalf("read %d command payload types for %d registered commands", len(commands), len(protocol.CommandKinds()))
	}
	known := map[string]bool{}
	var holds func(ast.Expr) bool
	named := func(name string) bool {
		if name == "Secret" {
			return true
		}
		if result, visited := known[name]; visited {
			return result // a type being visited holds nothing more by recursion
		}
		definition, declared := types[name]
		if !declared {
			return false
		}
		known[name] = false
		result := holds(definition)
		for _, body := range codecs[name] {
			variants := constructed(body)
			ast.Inspect(body, func(node ast.Node) bool {
				if table, ok := node.(*ast.Ident); ok {
					variants = append(variants, factories[table.Name]...)
				}
				return true
			})
			for _, variant := range variants {
				result = result || holds(variant)
			}
		}
		known[name] = result
		return result
	}
	holds = func(expression ast.Expr) bool {
		switch expression := expression.(type) {
		case *ast.Ident:
			return named(expression.Name)
		case *ast.StarExpr:
			return holds(expression.X)
		case *ast.ArrayType:
			return holds(expression.Elt)
		case *ast.MapType:
			return holds(expression.Key) || holds(expression.Value)
		case *ast.IndexExpr:
			return holds(expression.X) || holds(expression.Index)
		case *ast.IndexListExpr:
			return holds(expression.X) || slices.ContainsFunc(expression.Indices, holds)
		case *ast.StructType:
			return slices.ContainsFunc(expression.Fields.List, func(field *ast.Field) bool { return holds(field.Type) })
		default:
			return false
		}
	}
	// A cycle is resolved from its entry point, so each command starts afresh.
	carrying := map[string]bool{}
	for kind, payload := range commands {
		clear(known)
		if holds(payload) {
			carrying[kind] = true
		}
	}
	return carrying
}
