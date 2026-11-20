package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	configurationFormat  = "sailry-configuration"
	configurationVersion = 1
)

// ConfigurationDocument is one user's portable configuration. It is the unit a
// file export carries today and a later sync or backup could carry; it knows
// nothing of its transport. Every section is optional: an absent section is not
// included and leaves that area unchanged on import, so a document may be
// partial. Secrets are plaintext. Machine-bound settings (hooks, terminals,
// local models, code indexes, network relays and MCP servers) and credentials
// minted by sign-in are never part of it. Sandbox, power, Python and the
// personal instructions are machine settings, present only in an
// administrator's export.
type ConfigurationDocument struct {
	Format       string                                   `json:"format"`
	Version      int                                      `json:"version"`
	ExportedAt   string                                   `json:"exported_at"`
	Node         protocol.NodeID                          `json:"node"`
	Providers    []ConfigurationProvider                  `json:"providers,omitempty"`
	Search       *ConfigurationSearch                     `json:"search,omitempty"`
	Media        *protocol.MediaSettings                  `json:"media,omitempty"`
	Compaction   *protocol.ConversationCompactionSettings `json:"compaction,omitempty"`
	Team         *protocol.TeamSettings                   `json:"team,omitempty"`
	Roles        []protocol.RoleProfile                   `json:"roles,omitempty"`
	Teams        []protocol.TeamProfile                   `json:"teams,omitempty"`
	Sandbox      *protocol.SandboxSettings                `json:"sandbox,omitempty"`
	Power        *protocol.PowerSettings                  `json:"power,omitempty"`
	Python       *protocol.PythonSettings                 `json:"python,omitempty"`
	Instructions *string                                  `json:"instructions,omitempty"`
	Plugins      []ConfigurationPlugin                    `json:"plugins,omitempty"`
}

// ParseConfiguration accepts only this Node's document format and version.
func ParseConfiguration(document json.RawMessage) (ConfigurationDocument, error) {
	var parsed ConfigurationDocument
	if err := json.Unmarshal(document, &parsed); err != nil {
		return ConfigurationDocument{}, invalid("configuration document is invalid: " + err.Error())
	}
	if parsed.Format != configurationFormat {
		return ConfigurationDocument{}, invalid("file is not a Sailry configuration export")
	}
	if parsed.Version != configurationVersion {
		return ConfigurationDocument{}, invalid("configuration export version is unsupported")
	}
	return parsed, nil
}

// PluginNames lists the packages the document configures, in document order.
func (document ConfigurationDocument) PluginNames() []string {
	names := make([]string, 0, len(document.Plugins))
	for _, entry := range document.Plugins {
		names = append(names, entry.Name)
	}
	return names
}

// ExportConfiguration builds the current user's document. instructions is the
// profile's personal instructions text, which the Node reads. Lists are sorted
// by ID or name and maps encode in key order, so the same configuration and
// time give the same bytes.
func ExportConfiguration(ctx context.Context, db *sql.Conn, node protocol.NodeID, exportedAt time.Time, instructions *string) (ConfigurationDocument, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return ConfigurationDocument{}, err
	}
	document := ConfigurationDocument{Format: configurationFormat, Version: configurationVersion, ExportedAt: exportedAt.UTC().Format(time.RFC3339), Node: node}
	sections := []func(context.Context, *sql.Conn, protocol.NodeID, *ConfigurationDocument) error{
		exportProviders, exportSearch, exportMedia, exportCompaction, exportTeamSettings, exportRoles, exportTeams, exportPlugins,
	}
	if user.Administrator {
		document.Instructions = instructions
		sections = append(sections, exportSandbox, exportPower, exportPython)
	}
	for _, section := range sections {
		if err := section(ctx, db, node, &document); err != nil {
			return ConfigurationDocument{}, err
		}
	}
	return document, nil
}

// ConfigurationImport is the outcome of one import inside the caller's
// transaction. The Node writes Instructions to the profile after the commit
// and configures Python with Python.
type ConfigurationImport struct {
	Items        []protocol.ConfigurationImportItem
	Events       []protocol.Event
	Instructions *string
	Python       *protocol.PythonSettings
}

// PersonalInstructionsArea names the report item of the file the Node writes.
const PersonalInstructionsArea = "instructions"

// ConfigurationTargets are the inputs an import needs from outside SQL: the
// profile's personal instructions text and, per installed package, the MCP
// settings destinations the Node resolves.
type ConfigurationTargets struct {
	Instructions *string
	Plugins      map[string]map[string]string
}

type configurationImporter struct {
	ctx           context.Context
	db            *sql.Conn
	node          protocol.NodeID
	administrator bool
	targets       ConfigurationTargets
	// roles maps document role IDs to the target's roles matched by key.
	roles  map[protocol.RoleID]protocol.RoleID
	result ConfigurationImport
}

// ImportConfiguration merges a document into the current user's
// configuration: entries with the same ID are replaced, new entries are added
// and entries only on this Node are kept. Areas run in an order where
// references (media and roles to providers, teams to roles) are already
// imported.
func ImportConfiguration(ctx context.Context, db *sql.Conn, node protocol.NodeID, document ConfigurationDocument, targets ConfigurationTargets) (ConfigurationImport, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return ConfigurationImport{}, err
	}
	importer := &configurationImporter{ctx: ctx, db: db, node: node, administrator: user.Administrator, targets: targets}
	for _, section := range []func(*configurationImporter, ConfigurationDocument) error{
		importProviders, importSearch, importMedia, importCompaction, importTeamSettings, importRoles, importTeams,
		importSandbox, importPower, importPython, importInstructions, importPlugins,
	} {
		if err := section(importer, document); err != nil {
			return ConfigurationImport{}, err
		}
	}
	return importer.result, nil
}

// entry applies one entry in a savepoint of its own. A refusal of this entry
// (a validation or conflict fault) is rolled back and reported as skipped;
// any other error aborts the import.
func (importer *configurationImporter) entry(area, name string, apply func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error)) error {
	if _, err := importer.db.ExecContext(importer.ctx, "SAVEPOINT sailry_configuration_entry"); err != nil {
		return err
	}
	action, reason, events, err := apply()
	if err != nil {
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code == protocol.ErrorCodeInternal {
			return err
		}
		if _, err := importer.db.ExecContext(importer.ctx, "ROLLBACK TO sailry_configuration_entry"); err != nil {
			return err
		}
		action, reason, events = protocol.ConfigurationImportSkipped, &failure.Message, nil
	}
	if _, err := importer.db.ExecContext(importer.ctx, "RELEASE sailry_configuration_entry"); err != nil {
		return err
	}
	importer.result.Items = append(importer.result.Items, protocol.ConfigurationImportItem{Area: area, Name: name, Action: action, Reason: reason})
	importer.result.Events = append(importer.result.Events, events...)
	return nil
}

func (importer *configurationImporter) skip(area, name, reason string) {
	importer.result.Items = append(importer.result.Items, protocol.ConfigurationImportItem{Area: area, Name: name, Action: protocol.ConfigurationImportSkipped, Reason: &reason})
}

// machine reports a machine section as skipped unless the caller administers
// this machine.
func (importer *configurationImporter) machine(area string) bool {
	if !importer.administrator {
		importer.skip(area, "", "only a machine administrator can change this setting")
	}
	return importer.administrator
}

func reasonText(value string) *string { return &value }

func changedAction(existing bool) protocol.ConfigurationImportAction {
	if existing {
		return protocol.ConfigurationImportUpdated
	}
	return protocol.ConfigurationImportAdded
}

func isFault(err error, code protocol.ErrorCode) bool {
	var failure *protocol.Fault
	return errors.As(err, &failure) && failure.Code == code
}
