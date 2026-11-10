package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// LocalModelCatalog is the durable local model configuration: the settings
// and every pinned or registered definition by identity.
type LocalModelCatalog struct {
	Settings    protocol.ModelsLocalSettings
	Definitions map[string]*protocol.ModelsDefinition
}

// ReadLocalModelSettings reads the saved choices and mirror.
func ReadLocalModelSettings(ctx context.Context, db *sql.Conn) (protocol.ModelsLocalSettings, error) {
	settings, err := readBody[protocol.ModelsLocalSettings](ctx, db, "SELECT body FROM local_model_settings WHERE singleton=1")
	if err != nil {
		return protocol.ModelsLocalSettings{}, err
	}
	if settings == nil {
		return protocol.ModelsLocalSettings{}, errors.New("local model settings are missing")
	}
	localmodels.FillDefaults(settings)
	return *settings, localmodels.ValidateSettings(settings)
}

// ReadLocalModelCatalog reads the settings and joins registered definitions
// with the pinned builtins.
func ReadLocalModelCatalog(ctx context.Context, db *sql.Conn) (LocalModelCatalog, error) {
	var catalog LocalModelCatalog
	settings, err := ReadLocalModelSettings(ctx, db)
	if err != nil {
		return catalog, err
	}
	rows, err := db.QueryContext(ctx, "SELECT id,kind,digest,body FROM local_models ORDER BY id")
	if err != nil {
		return catalog, err
	}
	defer rows.Close()
	var records []localmodels.Registration
	for rows.Next() {
		var id, kind, digest string
		var body []byte
		if err := rows.Scan(&id, &kind, &digest, &body); err != nil {
			return catalog, err
		}
		record := localmodels.Registration{ID: id}
		if err := json.Unmarshal(body, &record.Definition); err != nil {
			return catalog, err
		}
		if digest != localmodels.ID(&record.Definition) || kind != string(localmodels.Kind(record.Definition.Format)) {
			return catalog, errors.New("local model metadata does not match its definition")
		}
		records = append(records, record)
	}
	if err := rows.Err(); err != nil {
		return catalog, err
	}
	definitions, err := localmodels.Definitions(records)
	if err != nil {
		return catalog, err
	}
	for _, kind := range localmodels.NodeKinds {
		model := settings.Choices[kind].Model
		if model == nil {
			continue
		}
		if definition, registered := definitions[*model]; !registered || localmodels.Kind(definition.Format) != kind {
			return catalog, invalid("selected local model is not registered for its kind")
		}
	}
	return LocalModelCatalog{Settings: settings, Definitions: definitions}, nil
}

// LocalModelDefinition reads one pinned or registered definition.
func LocalModelDefinition(ctx context.Context, db *sql.Conn, id string) (*protocol.ModelsDefinition, error) {
	catalog, err := ReadLocalModelCatalog(ctx, db)
	if err != nil {
		return nil, err
	}
	if definition, registered := catalog.Definitions[id]; registered {
		return definition, nil
	}
	return nil, fault(protocol.ErrorCodeNotFound, "local model is not registered")
}

// SaveLocalModelSettings replaces the settings at their expected revision.
// Every selected model must be registered for its kind.
func SaveLocalModelSettings(ctx context.Context, db *sql.Conn, settings protocol.ModelsLocalSettings) (protocol.Output, []protocol.Event, error) {
	catalog, err := ReadLocalModelCatalog(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err := checkRevision(catalog.Settings.Revision, settings.Revision); err != nil {
		return protocol.Output{}, nil, err
	}
	localmodels.FillDefaults(&settings)
	if err := localmodels.ValidateSettings(&settings); err != nil {
		return protocol.Output{}, nil, err
	}
	for _, kind := range localmodels.NodeKinds {
		model := settings.Choices[kind].Model
		if model == nil {
			continue
		}
		definition, registered := catalog.Definitions[*model]
		if !registered {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "local model is not registered")
		}
		if localmodels.Kind(definition.Format) != kind {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "local model is not registered for its kind")
		}
	}
	if settings.Revision >= math.MaxInt64 {
		return protocol.Output{}, nil, invalid("local model settings revision is exhausted")
	}
	settings.Revision++
	body, err := encode(settings)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "UPDATE local_model_settings SET body=?1 WHERE singleton=1", body); err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "local_model_settings", Data: settings},
		[]protocol.Event{{Kind: "local_model_settings_changed", Data: protocol.EventLocalModelSettingsChangedPayload{Revision: settings.Revision}}}, nil
}

// RegisterLocalModel stores a validated definition under its content identity
// and returns that identity. Registering a known definition changes nothing.
func RegisterLocalModel(ctx context.Context, db *sql.Conn, capabilities localmodels.Capabilities, definition protocol.ModelsDefinition) (protocol.Output, []protocol.Event, error) {
	if err := localmodels.ValidateDefinition(&definition); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := capabilities.Ensure(localmodels.Kind(definition.Format)); err != nil {
		return protocol.Output{}, nil, err
	}
	definition.Assets = slices.Clone(definition.Assets)
	slices.SortStableFunc(definition.Assets, func(left, right protocol.ModelsAsset) int { return strings.Compare(left.Name, right.Name) })
	for index := range definition.Assets {
		asset := &definition.Assets[index]
		asset.Source.Revision, asset.Sha256 = strings.ToLower(asset.Source.Revision), strings.ToLower(asset.Sha256)
	}
	digest := localmodels.ID(&definition)
	id := digest
	err := db.QueryRowContext(ctx, "SELECT id FROM local_models WHERE digest=?1", digest).Scan(&id)
	registered := err == nil
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return protocol.Output{}, nil, err
	}
	if !registered && !slices.ContainsFunc(localmodels.OCR(), func(pinned protocol.ModelsDefinition) bool { return localmodels.ID(&pinned) == id }) {
		kind := string(localmodels.Kind(definition.Format))
		var count int
		if err := db.QueryRowContext(ctx, "SELECT COUNT(*) FROM local_models WHERE kind=?1", kind).Scan(&count); err != nil {
			return protocol.Output{}, nil, err
		}
		if count >= localmodels.Limit {
			return protocol.Output{}, nil, invalid("local model catalog exceeds its limit")
		}
		body, err := encode(definition)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO local_models(id,kind,digest,body) VALUES(?1,?2,?3,?4)", id, kind, digest, body); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	return protocol.Output{Kind: "local_model", Data: protocol.OutputLocalModelPayload{ID: id}}, nil, nil
}

// AuthorizeLocalModelRemoval admits a plugin's model removal only as the
// approved tool call that captured it. Controllers pass.
func AuthorizeLocalModelRemoval(ctx context.Context, db *sql.Conn, request protocol.Request) error {
	scope := request.Plugin
	if scope == nil {
		return nil
	}
	denied := fault(protocol.ErrorCodePermissionDenied, "model change requires its exact captured approval")
	if scope.Turn == nil || scope.Invocation != nil {
		return denied
	}
	run, err := ConnectionOwner(ctx, db, *scope.Turn)
	if err != nil {
		return err
	}
	if scope.Session == nil || *scope.Session != run.Session || scope.Worktree == nil || *scope.Worktree != run.Worktree ||
		request.Command.Kind != "delete_local_model" {
		return denied
	}
	var body, bound []byte
	err = db.QueryRowContext(ctx, "SELECT body,binding FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1", scope.Turn.String(), request.ID.String()).Scan(&body, &bound)
	if errors.Is(err, sql.ErrNoRows) {
		return denied
	}
	if err != nil {
		return err
	}
	var approval protocol.ConversationApproval
	var binding *ApprovalBinding
	if err := json.Unmarshal(body, &approval); err != nil {
		return err
	}
	if err := json.Unmarshal(bound, &binding); err != nil {
		return err
	}
	call, err := approvalRequest(ctx, db, approval)
	if err != nil {
		return err
	}
	if approval.State != protocol.ConversationApprovalStateApproved || approval.Session != run.Session ||
		binding == nil || binding.Owner != scope.Package || binding.Operation == nil || *binding.Operation != protocol.ToolOperationRemoveModel ||
		call == nil || binding.Tool != call.Name {
		return denied
	}
	return nil
}
