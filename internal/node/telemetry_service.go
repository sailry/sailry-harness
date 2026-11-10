package node

import (
	"context"
	"database/sql"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// newTelemetry reads the process's reporting environment. Usage counters
// name a model only when a built-in preset lists it.
func newTelemetry(options telemetry.Options) *telemetry.Control {
	if options.Models == nil {
		options.Models = map[string][]string{}
		for _, preset := range providers.Bundled().Builtin {
			switch models := preset.Models.Data.(type) {
			case protocol.ConversationPresetsModelsStaticPayload:
				options.Models[preset.ID] = models.IDs
			case *protocol.ConversationPresetsModelsStaticPayload:
				options.Models[preset.ID] = models.IDs
			}
		}
	}
	return telemetry.New(options)
}

// initTelemetry gives the process its saved consent and anonymous identity,
// then lets the process's log report failed operations.
func (node *Node) initTelemetry(ctx context.Context) error {
	err := node.applyTelemetry(ctx)
	if err == nil {
		node.attachLogging()
	}
	return err
}

// applyTelemetry applies current storage, never a replayed consent policy.
func (node *Node) applyTelemetry(ctx context.Context) error {
	var settings protocol.TelemetrySettings
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		settings, err = store.ReadTelemetry(ctx, db)
		return err
	})
	if err == nil {
		node.telemetry.Configure(settings)
	}
	return err
}

// dispatchDiagnostics sends the report a controller asked for. It is not
// durable: a lost result is never replayed as a second report.
func (node *Node) dispatchDiagnostics(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Plugin != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "telemetry settings require a controller"}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		operation, cancel := context.WithCancel(ctx)
		defer cancel()
		defer context.AfterFunc(node.ctx, cancel)()
		completion <- response(protocol.Output{Kind: "diagnostics_sent"}, node.telemetry.Diagnostics(operation))
	}()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

// searchObservation counts a web search by its driver and outcome only.
func searchObservation(plan web.SearchPlan, page protocol.SearchPage, err error, elapsed time.Duration) telemetry.Observation {
	var driver *protocol.SearchDriver
	if selection := plan.Settings.Driver; selection != nil && selection.Kind == "driver" {
		switch value := selection.Data.(type) {
		case protocol.SearchDriver:
			driver = &value
		case *protocol.SearchDriver:
			driver = value
		}
	}
	if err == nil {
		return telemetry.Search(&page.Driver, "success", nil, elapsed)
	}
	code, result := nodeFault(err).(*protocol.Fault).Code, "error"
	if code == protocol.ErrorCodeCancelled {
		result = "cancelled"
	}
	return telemetry.Search(driver, result, &code, elapsed)
}
