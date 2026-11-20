package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func configurationCommand(kind string) bool {
	return kind == "export_configuration" || kind == "import_configuration"
}

// dispatchConfiguration exports or merges a user's configuration document.
// An import is one durable request: its SQL changes commit together, and the
// personal instructions file, which lives outside SQL, is written after them.
func (node *Node) dispatchConfiguration(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if request.Plugin != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "configuration transfer requires a controller"}
	}
	var document store.ConfigurationDocument
	if input, ok := request.Command.Data.(*protocol.CommandImportConfigurationPayload); ok {
		var err error
		if document, err = store.ParseConfiguration(input.Document); err != nil {
			return link.Admission{}, err
		}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if !accepted.Receipt.Durable {
		go func() {
			defer node.work.Done()
			defer close(completion)
			completion <- response(node.exportConfiguration(ctx))
		}()
		return accepted, nil
	}
	var previous *link.Response
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	}); err != nil {
		node.work.Done()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		node.work.Done()
		return accepted, nil
	}
	job := commandJob{user: userFromContext(ctx), scope: ctx, caller: caller, request: request}
	go func() {
		defer node.work.Done()
		defer close(completion)
		completion <- node.importConfiguration(node.commandContext(job), caller, request, document)
	}()
	return accepted, nil
}

func (node *Node) exportConfiguration(ctx context.Context) (protocol.Output, error) {
	var document json.RawMessage
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		var instructions *string
		if user.Administrator {
			value, err := prompt.ReadPersonalInstructions(ctx, node.profile.Ownership.Path)
			if err != nil {
				return err
			}
			if value.Revision != nil {
				instructions = &value.Text
			}
		}
		value, err := store.ExportConfiguration(ctx, db, node.ID(), node.dispatch.clock.Now(), instructions)
		if err != nil {
			return err
		}
		document, err = json.Marshal(value)
		return err
	})
	return protocol.Output{Kind: "configuration_document", Data: protocol.ConfigurationDocument{Document: document}}, err
}

func (node *Node) importConfiguration(ctx context.Context, caller protocol.NodeID, request protocol.Request, document store.ConfigurationDocument) link.Response {
	preview := request.Command.Data.(*protocol.CommandImportConfigurationPayload).Preview
	var current protocol.PersonalInstructions
	var operationErr error
	if document.Instructions != nil {
		current, operationErr = prompt.ReadPersonalInstructions(ctx, node.profile.Ownership.Path)
	}
	targets := store.ConfigurationTargets{Plugins: map[string]map[string]string{}}
	if current.Revision != nil {
		targets.Instructions = &current.Text
	}
	for _, name := range document.PluginNames() {
		if operationErr != nil {
			break
		}
		var reference *protocol.PluginReference
		operationErr = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			info, err := store.ReadPlugin(ctx, db, name)
			if info != nil {
				value := plugins.PackageReference(*info)
				reference = &value
			}
			return err
		})
		if operationErr == nil && reference != nil {
			// A package whose destinations cannot be resolved keeps its
			// settings; the import reports them as skipped.
			if value, err := node.pluginMCPTargets(ctx, *reference); err == nil {
				targets.Plugins[name] = value
			}
		}
	}
	// Search settings saves are ordered with their refresh (workspaceLocks).
	node.locks.settings.Lock()
	defer node.locks.settings.Unlock()
	publication := node.agent.LockPublication()
	var result store.ConfigurationImport
	var changes []protocol.Event
	writeInstructions := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "SAVEPOINT sailry_configuration"); err != nil {
			return err
		}
		if operationErr == nil {
			result, operationErr = store.ImportConfiguration(ctx, db, node.ID(), document, targets)
		}
		changes, writeInstructions = result.Events, result.Instructions != nil && !preview
		if operationErr != nil || preview {
			changes, writeInstructions = nil, false
			if _, err := db.ExecContext(ctx, "ROLLBACK TO sailry_configuration"); err != nil {
				return err
			}
		}
		if _, err := db.ExecContext(ctx, "RELEASE sailry_configuration"); err != nil {
			return err
		}
		if _, err := store.RecordEvents(ctx, db, node.ID(), changes); err != nil {
			return err
		}
		if writeInstructions {
			// Settled after the file is written, so the stored result says
			// whether it was.
			return nil
		}
		return admission.Complete(ctx, db, caller, request.ID, response(configurationReport(preview, result), operationErr))
	})
	if err == nil {
		node.conversationEvents(changes)
		node.systemEvents(changes)
	}
	publication()
	if err != nil {
		return node.unapplied(caller, request.ID, err)
	}
	if operationErr == nil && !preview {
		node.configurationCommitted(ctx, changes, result.Python)
	}
	if !writeInstructions {
		return response(configurationReport(preview, result), operationErr)
	}
	if _, err := prompt.SavePersonalInstructions(ctx, node.profile.Ownership.Path, protocol.CommandSavePersonalInstructionsPayload{Text: *result.Instructions, ExpectedRevision: current.Revision}); err != nil {
		reason := nodeFault(err).(*protocol.Fault).Message
		for index := range result.Items {
			if result.Items[index].Area == store.PersonalInstructionsArea {
				result.Items[index].Action, result.Items[index].Reason = protocol.ConfigurationImportSkipped, &reason
			}
		}
	}
	return node.settle(caller, request.ID, response(configurationReport(preview, result), nil))
}

func configurationReport(preview bool, result store.ConfigurationImport) protocol.Output {
	return protocol.Output{Kind: "configuration_import", Data: protocol.ConfigurationImportReport{Preview: preview, Items: result.Items}}
}

// configurationCommitted applies committed changes the way their own
// commands do after a commit: package programs and authentication follow
// plugin changes, model clients follow provider changes, and Python follows
// its package index.
func (node *Node) configurationCommitted(ctx context.Context, changes []protocol.Event, python *protocol.PythonSettings) {
	plugin := false
	for _, change := range changes {
		plugin = plugin || change.Kind == "plugin_changed"
	}
	if plugin {
		node.pluginCommitted(changes, ctx, nil)
	} else {
		for _, change := range changes {
			if store.RuntimeCredentialEvent(change.Kind) {
				node.RuntimeCredentialsChanged()
				break
			}
		}
		node.wakeDispatch()
		node.publish()
		node.agent.Committed()
	}
	if python != nil && node.plugins.python != nil {
		_ = node.plugins.python.Configure(python.Index)
	}
}
