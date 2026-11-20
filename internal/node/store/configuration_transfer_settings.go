package store

import (
	"context"
	"database/sql"
	"maps"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Single settings are exported without their revision and imported over the
// target's current revision: the document's values replace the target's.

// ConfigurationSearch is the search settings with each driver's credentials.
type ConfigurationSearch struct {
	Settings    protocol.SearchSettings              `json:"settings"`
	Credentials map[protocol.SearchDriver]searchAuth `json:"credentials,omitempty"`
}

func exportSearch(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	saved, err := readWebSearchStored(ctx, db)
	if err != nil {
		return err
	}
	search := ConfigurationSearch{Settings: saved.Settings, Credentials: map[protocol.SearchDriver]searchAuth{}}
	search.Settings.Revision = 0
	for driver, auth := range saved.Credentials {
		if auth.Key != nil || auth.Basic != nil {
			search.Credentials[driver] = auth
		}
	}
	document.Search = &search
	return nil
}

func importSearch(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Search == nil {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("search", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := readWebSearchStored(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		update := protocol.SearchUpdate{Settings: document.Search.Settings, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{}}
		update.Settings.Revision = current.Settings.Revision
		for driver, auth := range document.Search.Credentials {
			credentials := protocol.DefaultSearchCredentials()
			if auth.Key != nil {
				credentials.Key = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: *auth.Key}
			}
			if auth.Basic != nil {
				credentials.Basic = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: *auth.Basic}
			}
			update.Credentials[driver] = credentials
		}
		_, events, err := SaveSearchSettings(ctx, db, protocol.Request{}, update)
		return protocol.ConfigurationImportUpdated, nil, events, err
	})
}

func exportMedia(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	media, err := ReadMediaSettings(ctx, db)
	if err != nil {
		return err
	}
	media.Revision, media.Unavailable = 0, nil
	document.Media = &media
	return nil
}

// importMedia replaces the bindings the document names and keeps the others.
func importMedia(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Media == nil {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("media", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadMediaSettings(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		bindings := maps.Clone(current.Bindings)
		if bindings == nil {
			bindings = map[protocol.MediaKind]*protocol.MediaBinding{}
		}
		maps.Copy(bindings, document.Media.Bindings)
		_, events, err := saveMediaSettings(ctx, db, protocol.MediaSettings{Revision: current.Revision, Bindings: bindings})
		return protocol.ConfigurationImportUpdated, nil, events, err
	})
}

func exportCompaction(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	value, err := ReadCompaction(ctx, db)
	if err != nil {
		return err
	}
	value.Revision = 0
	document.Compaction = &value
	return nil
}

func importCompaction(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Compaction == nil {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("compaction", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadCompaction(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		value := *document.Compaction
		value.Revision = current.Revision
		_, events, err := saveCompaction(ctx, db, value)
		return protocol.ConfigurationImportUpdated, nil, events, err
	})
}

func exportTeamSettings(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	value, err := ReadTeamSettings(ctx, db)
	if err != nil {
		return err
	}
	value.Revision, value.Source = 0, nil
	document.Team = &value
	return nil
}

func importTeamSettings(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Team == nil {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("team", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadTeamSettings(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		value := *document.Team
		value.Revision, value.Source = current.Revision, nil
		_, events, err := ExecuteTeamCommand(ctx, db, protocol.Command{Kind: "save_team_settings", Data: &value})
		return protocol.ConfigurationImportUpdated, nil, events, err
	})
}

func exportSandbox(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	value, err := ReadSandboxSettings(ctx, db)
	if err != nil {
		return err
	}
	value.Revision = 0
	document.Sandbox = &value
	return nil
}

func importSandbox(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Sandbox == nil || !importer.machine("sandbox") {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("sandbox", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadSandboxSettings(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		value := *document.Sandbox
		value.Revision = current.Revision
		value, err = SaveSandboxSettings(ctx, db, value)
		return protocol.ConfigurationImportUpdated, nil, []protocol.Event{{Kind: "sandbox_settings_changed", Data: protocol.EventSandboxSettingsChangedPayload{Revision: value.Revision}}}, err
	})
}

func exportPower(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	value, err := ReadPower(ctx, db)
	if err != nil {
		return err
	}
	value.Revision = 0
	document.Power = &value
	return nil
}

func importPower(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Power == nil || !importer.machine("power") {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("power", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadPower(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		value := *document.Power
		value.Revision = current.Revision
		_, events, err := savePower(ctx, db, value)
		return protocol.ConfigurationImportUpdated, nil, events, err
	})
}

func exportPython(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	value, err := ReadPythonSettings(ctx, db)
	if err != nil {
		return err
	}
	value.Revision = 0
	document.Python = &value
	return nil
}

func importPython(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Python == nil || !importer.machine("python") {
		return nil
	}
	ctx, db := importer.ctx, importer.db
	return importer.entry("python", "", func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadPythonSettings(ctx, db)
		if err != nil {
			return "", nil, nil, err
		}
		value := *document.Python
		value.Revision = current.Revision
		output, events, err := SavePythonSettings(ctx, db, value)
		if err == nil {
			saved := output.Data.(protocol.PythonInfo).Settings
			importer.result.Python = &saved
		}
		return protocol.ConfigurationImportUpdated, nil, events, err
	})
}

// importInstructions reports the personal instructions file, which the Node
// writes after the commit when its text differs.
func importInstructions(importer *configurationImporter, document ConfigurationDocument) error {
	if document.Instructions == nil || !importer.machine(PersonalInstructionsArea) {
		return nil
	}
	current := importer.targets.Instructions
	importer.result.Items = append(importer.result.Items, protocol.ConfigurationImportItem{Area: PersonalInstructionsArea, Action: changedAction(current != nil)})
	if current == nil || *current != *document.Instructions {
		importer.result.Instructions = document.Instructions
	}
	return nil
}
