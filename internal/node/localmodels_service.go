package node

import (
	"context"
	"database/sql"
	"log/slog"
	"reflect"
	"sync"

	"github.com/egoist/mygo/netproxy"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/installations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// localModelControls owns the verified model registry and the Hub clients.
type localModelControls struct {
	registry *localmodels.Registry
	hubs     *localmodels.Hubs
	// configuring orders each durable configuration commit with its
	// application to the registry.
	configuring sync.Mutex
	followed    chan struct{}
}

func localModelCommand(kind string) bool {
	switch kind {
	case "read_local_model_settings", "save_local_model_settings", "read_local_models", "refresh_local_model",
		"register_local_model", "install_local_model", "delete_local_model", "cancel_local_model",
		"search_models", "read_model_versions", "resolve_model":
		return true
	}
	return false
}

// initLocalModels loads the saved catalog into a registry, verifies the
// installed files once and starts recording catalog changes.
func (node *Node) initLocalModels(ctx context.Context, options localmodels.Options) error {
	options.Profile = node.profile.Ownership.Path
	if options.Transport == nil {
		options.Transport = netproxy.Transport()
	}
	controls := &localModelControls{
		registry: localmodels.New(options),
		hubs:     &localmodels.Hubs{Transport: options.Transport},
		followed: make(chan struct{}),
	}
	var prepared *localmodels.Prepared
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		catalog, err := store.ReadLocalModelCatalog(ctx, db)
		if err != nil {
			return err
		}
		prepared, err = controls.registry.Prepare(catalog.Settings, catalog.Definitions)
		return err
	})
	if err == nil {
		controls.registry.Apply(prepared)
		_, err = controls.registry.List("")
	}
	if err != nil {
		controls.registry.Shutdown()
		return err
	}
	node.localModels = controls
	previous, changed := controls.registry.Updates()
	node.initKnowledge()
	go node.followLocalModels(previous, changed)
	return nil
}

// followLocalModels records every change of the model list until the Node
// stops. A failed write is retried with the complete list at the next change.
func (node *Node) followLocalModels(previous localmodels.Update, changed <-chan struct{}) {
	defer close(node.localModels.followed)
	recorded := previous.Models
	for {
		select {
		case <-node.closed:
			return
		case <-changed:
		}
		var latest localmodels.Update
		latest, changed = node.localModels.registry.Updates()
		node.reactKnowledge(previous, latest)
		previous = latest
		if reflect.DeepEqual(latest.Models, recorded) {
			continue
		}
		err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
			_, err := store.RecordEvents(node.ctx, db, node.ID(), []protocol.Event{{Kind: "local_models_changed", Data: protocol.EventLocalModelsChangedPayload{Models: latest.Models}}})
			return err
		})
		if err != nil {
			slog.Warn("local model catalog change could not be recorded", "code", nodeFault(err).(*protocol.Fault).Code)
			continue
		}
		recorded = latest.Models
		node.publish()
	}
}

func (node *Node) closeLocalModels() {
	<-node.localModels.followed
	node.localModels.registry.Shutdown()
}

func localModelController(request protocol.Request) error {
	if request.Plugin != nil {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "local model settings require a controller"}
	}
	return nil
}

func (node *Node) dispatchLocalModels(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if !accepted.Receipt.Durable {
		go func() {
			defer node.work.Done()
			defer close(completion)
			// Hub requests stop with their caller and with the Node.
			operation, cancel := context.WithCancel(ctx)
			defer cancel()
			defer context.AfterFunc(node.ctx, cancel)()
			completion <- response(node.readLocalModels(operation, request))
		}()
		return accepted, nil
	}
	var previous *link.Response
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	})
	if err != nil {
		node.work.Done()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		node.work.Done()
		return accepted, nil
	}
	if request.Command.Kind == "save_local_model_settings" || request.Command.Kind == "register_local_model" {
		go func() {
			defer node.work.Done()
			defer close(completion)
			completion <- node.configureLocalModels(caller, request)
		}()
		return accepted, nil
	}
	node.submitLocalModel(caller, request, completion)
	return accepted, nil
}

func (node *Node) readLocalModels(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	controls := node.localModels
	mirror := controls.registry.Settings().Mirror
	switch request.Command.Kind {
	case "read_local_model_settings":
		if err := localModelController(request); err != nil {
			return protocol.Output{}, err
		}
		var settings protocol.ModelsLocalSettings
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			settings, err = store.ReadLocalModelSettings(ctx, db)
			return err
		})
		return protocol.Output{Kind: "local_model_settings", Data: settings}, err
	case "read_local_models":
		input := request.Command.Data.(*protocol.CommandReadLocalModelsPayload)
		var kind protocol.ModelsKind
		if input.Kind != nil {
			kind = *input.Kind
		}
		models, err := controls.registry.List(kind)
		if err != nil {
			return protocol.Output{}, err
		}
		return protocol.Output{Kind: "local_models", Data: controls.registry.Capabilities().Catalog(models)}, nil
	case "cancel_local_model":
		input := request.Command.Data.(*protocol.CommandCancelLocalModelPayload)
		if err := localModelController(request); err != nil {
			return protocol.Output{}, err
		}
		if _, err := controls.registry.Definition(input.ID); err != nil {
			return protocol.Output{}, err
		}
		controls.registry.Cancel(input.ID)
		return protocol.Output{Kind: "local_model", Data: protocol.OutputLocalModelPayload{ID: input.ID}}, nil
	case "search_models":
		page, err := controls.hubs.Search(ctx, request.Command.Data.(*protocol.ModelsSearch), mirror)
		return protocol.Output{Kind: "model_search", Data: page}, err
	case "read_model_versions":
		input := request.Command.Data.(*protocol.CommandReadModelVersionsPayload)
		versions, err := controls.hubs.Versions(ctx, input.Hub, input.Repository, mirror)
		return protocol.Output{Kind: "model_versions", Data: versions}, err
	case "resolve_model":
		input := request.Command.Data.(*protocol.CommandResolveModelPayload)
		definition, err := controls.hubs.Resolve(ctx, input.Kind, input.Source, mirror)
		return protocol.Output{Kind: "model_definition", Data: definition}, err
	case "refresh_local_model":
		input := request.Command.Data.(*protocol.CommandRefreshLocalModelPayload)
		previous, err := controls.registry.Definition(input.ID)
		if err != nil {
			return protocol.Output{}, err
		}
		if localmodels.Kind(previous.Format) != protocol.ModelsKindOcr {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "only OCR model definitions can be refreshed"}
		}
		definition, err := controls.hubs.RefreshOCR(ctx, previous, mirror)
		return protocol.Output{Kind: "model_definition", Data: definition}, err
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "local model read expected"}
}

// configureLocalModels commits a settings or registration change together
// with the registry's acceptance of the resulting catalog, then applies it.
func (node *Node) configureLocalModels(caller protocol.NodeID, request protocol.Request) link.Response {
	controls := node.localModels
	controls.configuring.Lock()
	defer controls.configuring.Unlock()
	var output protocol.Output
	var operationErr error
	var prepared *localmodels.Prepared
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(node.ctx, "SAVEPOINT sailry_local_models"); err != nil {
			return err
		}
		var events []protocol.Event
		output, events, prepared, operationErr = node.executeLocalModelConfiguration(db, caller, request)
		if operationErr != nil {
			events, prepared = nil, nil
			if _, err := db.ExecContext(node.ctx, "ROLLBACK TO sailry_local_models"); err != nil {
				return err
			}
		}
		if _, err := db.ExecContext(node.ctx, "RELEASE sailry_local_models"); err != nil {
			return err
		}
		if _, err := store.RecordEvents(node.ctx, db, node.ID(), events); err != nil {
			return err
		}
		return admission.Complete(node.ctx, db, caller, request.ID, response(output, operationErr))
	})
	if err != nil {
		return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "local model configuration result could not be recorded; read the settings before retrying"})
	}
	if prepared != nil {
		controls.registry.Apply(prepared)
		node.publish()
	}
	return response(output, operationErr)
}

func (node *Node) executeLocalModelConfiguration(db *sql.Conn, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, *localmodels.Prepared, error) {
	registry := node.localModels.registry
	if err := node.validatePluginRequestDB(node.ctx, db, caller, request); err != nil {
		return protocol.Output{}, nil, nil, err
	}
	var output protocol.Output
	var events []protocol.Event
	var err error
	if request.Command.Kind == "save_local_model_settings" {
		if err = localModelController(request); err == nil {
			output, events, err = store.SaveLocalModelSettings(node.ctx, db, *request.Command.Data.(*protocol.ModelsLocalSettings))
		}
	} else {
		input := request.Command.Data.(*protocol.CommandRegisterLocalModelPayload)
		output, events, err = store.RegisterLocalModel(node.ctx, db, registry.Capabilities(), input.Definition)
	}
	if err != nil {
		return protocol.Output{}, nil, nil, err
	}
	catalog, err := store.ReadLocalModelCatalog(node.ctx, db)
	if err != nil {
		return protocol.Output{}, nil, nil, err
	}
	prepared, err := registry.Prepare(catalog.Settings, catalog.Definitions)
	if err != nil {
		return protocol.Output{}, nil, nil, err
	}
	return output, events, prepared, nil
}

// submitLocalModel orders an installation or removal in the model's
// installation lane. Only installations are observable.
func (node *Node) submitLocalModel(caller protocol.NodeID, request protocol.Request, completion chan link.Response) {
	registry := node.localModels.registry
	recording := context.WithoutCancel(node.ctx)
	settle := func(result link.Response) link.Response {
		err := node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
			return admission.Complete(recording, db, caller, request.ID, result)
		})
		if err != nil {
			result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "local model operation result could not be recorded; read the local models before retrying"})
		}
		return result
	}
	var id string
	removing := request.Command.Kind == "delete_local_model"
	if removing {
		id = request.Command.Data.(*protocol.CommandDeleteLocalModelPayload).ID
	} else {
		id = request.Command.Data.(*protocol.CommandInstallLocalModelPayload).ID
	}
	var definition *protocol.ModelsDefinition
	err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		var err error
		definition, err = store.LocalModelDefinition(node.ctx, db, id)
		return err
	})
	if err == nil {
		resource := protocol.InstallationResource{Kind: "model", Data: protocol.InstallationResourceModelPayload{Kind: localmodels.Kind(definition.Format), ID: id}}
		err = node.plugins.installations.Submit(caller, request, resource, !removing, func(owner context.Context, report installations.Report) (result link.Response) {
			defer node.work.Done()
			defer func() {
				if recover() != nil {
					result = settle(response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "local model operation did not settle; read the local models before retrying"}))
				}
				completion <- result
				close(completion)
			}()
			report(protocol.InstallationPhaseInspecting, nil)
			return settle(response(protocol.Output{Kind: "local_model", Data: protocol.OutputLocalModelPayload{ID: id}}, node.changeLocalModel(owner, caller, request, id, report)))
		})
		if err == nil {
			if removing {
				// Ordered removal cancels an installation before waiting for its completion.
				registry.Cancel(id)
			}
			return
		}
	}
	result := settle(response(protocol.Output{}, err))
	node.work.Done()
	completion <- result
	close(completion)
}

func (node *Node) changeLocalModel(ctx context.Context, caller protocol.NodeID, request protocol.Request, id string, report installations.Report) error {
	registry := node.localModels.registry
	if request.Command.Kind == "install_local_model" {
		return registry.Install(ctx, id, localmodels.Report(report))
	}
	// A plugin removes a model only as the approved tool call that captured it.
	if request.Plugin != nil {
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			if err := node.validatePluginRequestDB(ctx, db, caller, request); err != nil {
				return err
			}
			return store.AuthorizeLocalModelRemoval(ctx, db, request)
		})
		if err != nil {
			return err
		}
	}
	return registry.Remove(id)
}
