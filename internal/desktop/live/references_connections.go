package live

import (
	"encoding/json"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	connectionReferenceSource = "connections"
	sshReferencePrefix        = "ssh:"
	databasePagePrefix        = "database/"
)

// referenceCatalog is one browse_database read for an open picker page. A
// result is applied only while the entry is still the registered one, so a
// reopened picker or a changed profile never receives a late answer.
type referenceCatalog struct {
	loading bool
	catalog *protocol.DatabaseCatalog
}

// connectionScope is the conversation binding that decides which saved
// connections its composer may reference.
type connectionScope struct {
	project  *protocol.ProjectID
	resource *protocol.ConnectionResource
}

func (scope connectionScope) allows(kind string, id any, sharing *protocol.ConnectionSharing) bool {
	if scope.resource != nil {
		if scope.resource.Kind != kind {
			return false
		}
		switch want := id.(type) {
		case protocol.DatabaseID:
			bound, ok := detail[protocol.DatabaseID](scope.resource.Data)
			return ok && bound == want
		case protocol.SshID:
			bound, ok := detail[protocol.SshID](scope.resource.Data)
			return ok && bound == want
		}
		return false
	}
	if scope.project == nil || sharing == nil {
		return false
	}
	if sharing.Kind == "global" {
		return true
	}
	projects, _ := detail[[]protocol.ProjectID](sharing.Data)
	return slices.Contains(projects, *scope.project)
}

func referenceDatabases(snapshot protocol.Snapshot, scope connectionScope) []protocol.DatabaseProfile {
	var profiles []protocol.DatabaseProfile
	for _, profile := range snapshot.Databases {
		if scope.allows("database", profile.ID, profile.Sharing) {
			profiles = append(profiles, profile)
		}
	}
	return profiles
}

func referenceSsh(snapshot protocol.Snapshot, scope connectionScope) []protocol.SshProfile {
	var profiles []protocol.SshProfile
	for _, profile := range snapshot.Ssh {
		if scope.allows("ssh", profile.ID, profile.Sharing) {
			profiles = append(profiles, profile)
		}
	}
	return profiles
}

func (r *Runtime) connectionScope(cv *viewmodel.Conversation, comp *composer.State) connectionScope {
	scope := connectionScope{}
	if session, ok := r.session(cv.ID); ok {
		scope.project, scope.resource = session.Project, session.Config.Resource
		return scope
	}
	if comp != nil {
		if project, err := protocol.ParseProjectID(comp.Project); err == nil {
			scope.project = &project
		}
	}
	return scope
}

func databasePageID(profile protocol.DatabaseID, database *string) string {
	id := databasePagePrefix + profile.String()
	if database != nil {
		id += "/" + *database
	}
	return id
}

func parseDatabasePage(id string) (protocol.DatabaseID, *string, bool) {
	rest, ok := strings.CutPrefix(id, databasePagePrefix)
	if !ok {
		return protocol.DatabaseID{}, nil, false
	}
	profile, database, nested := strings.Cut(rest, "/")
	parsed, err := protocol.ParseDatabaseID(profile)
	if err != nil {
		return protocol.DatabaseID{}, nil, false
	}
	if !nested {
		return parsed, nil, true
	}
	return parsed, &database, true
}

func databaseReferenceID(target protocol.ConversationReferenceTargetDatabasePayload) string {
	encoded, _ := json.Marshal(target)
	return string(encoded)
}

func databasePageRows(profiles []protocol.DatabaseProfile) []composer.Reference {
	rows := make([]composer.Reference, 0, len(profiles))
	for _, profile := range profiles {
		rows = append(rows, composer.Reference{ID: databasePageID(profile.ID, nil), Source: connectionReferenceSource, Kind: composer.Page, Label: profile.Name, Icon: icons.HardDrive})
	}
	return rows
}

func sshReferenceRows(profiles []protocol.SshProfile) []composer.Reference {
	rows := make([]composer.Reference, 0, len(profiles))
	for _, profile := range profiles {
		rows = append(rows, composer.Reference{ID: sshReferencePrefix + profile.ID.String(), Kind: composer.Location, Label: profile.Name, Icon: icons.Network})
	}
	return rows
}

// databaseCatalogRows lists one browsed database page: the page itself first,
// then its databases as pages or its tables as references.
func databaseCatalogRows(profile protocol.DatabaseProfile, database *string, catalog protocol.DatabaseCatalog) []composer.Reference {
	current := profile.Name
	if database != nil {
		current += "/" + *database
	}
	rows := []composer.Reference{{
		ID:   databaseReferenceID(protocol.ConversationReferenceTargetDatabasePayload{Connection: profile.ID, Database: database}),
		Kind: composer.Database, Label: "reference_select_current", Key: true, Description: current, Insert: current, Icon: icons.HardDrive,
	}}
	switch catalog.Kind {
	case "databases":
		names, _ := detail[[]string](catalog.Data)
		for _, name := range names {
			rows = append(rows, composer.Reference{ID: databasePageID(profile.ID, &name), Source: connectionReferenceSource, Kind: composer.Page, Label: name, Icon: icons.HardDrive})
		}
	case "tables":
		tables, _ := detail[protocol.DatabaseCatalogTablesPayload](catalog.Data)
		for _, table := range tables.Tables {
			rows = append(rows, composer.Reference{
				ID:   databaseReferenceID(protocol.ConversationReferenceTargetDatabasePayload{Connection: profile.ID, Database: &tables.Database, Table: &table}),
				Kind: composer.Database, Label: tables.Database + "/" + table.Name, Icon: icons.HardDrive,
			})
		}
	}
	return rows
}

func referenceCatalogKey(session string, profile protocol.DatabaseProfile, database *string) string {
	key := session + "\x00" + profile.ID.String() + "\x00" + strconv.FormatUint(profile.Revision, 10)
	if database != nil {
		key += "\x00" + *database
	}
	return key
}

// projectConnectionReferences adds the Databases and SSH pages of the `@`
// picker. Profiles come from the conversation's own execution Node.
func (r *Runtime) projectConnectionReferences(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context) {
	scope := r.connectionScope(cv, comp)
	rows := []composer.Reference{
		{ID: "databases", Source: connectionReferenceSource, Kind: composer.Page, Label: "reference_databases", Key: true, Alias: "database", Group: "reference_group_connections", Icon: icons.HardDrive},
		{ID: "ssh", Source: connectionReferenceSource, Kind: composer.Page, Label: "reference_ssh", Key: true, Alias: "ssh", Group: "reference_group_connections", Icon: icons.Network},
	}
	at := len(ctx.References)
	for index, reference := range ctx.References {
		if reference.Group != "reference_group_context" {
			at = index
			break
		}
	}
	ctx.References = slices.Insert(ctx.References, at, rows...)
	session := cv.ID
	comp.AddReferenceChildren(func(page composer.Reference) ([]composer.Reference, bool) {
		if page.Source != connectionReferenceSource || page.Kind != composer.Page {
			return nil, false
		}
		snapshot := r.snapshot()
		switch page.ID {
		case "databases":
			return databasePageRows(referenceDatabases(snapshot, scope)), true
		case "ssh":
			return sshReferenceRows(referenceSsh(snapshot, scope)), true
		}
		id, database, ok := parseDatabasePage(page.ID)
		if !ok {
			return nil, false
		}
		for _, profile := range referenceDatabases(snapshot, scope) {
			if profile.ID == id {
				return r.databaseReferences(session, scope, profile, database), true
			}
		}
		return nil, true
	})
}

func (r *Runtime) databaseReferences(session string, scope connectionScope, profile protocol.DatabaseProfile, database *string) []composer.Reference {
	key := referenceCatalogKey(session, profile, database)
	if entry := r.connectionReferences[key]; entry != nil {
		if entry.catalog == nil {
			return nil
		}
		return databaseCatalogRows(profile, database, *entry.catalog)
	}
	if r.connectionReferences == nil {
		r.connectionReferences = map[string]*referenceCatalog{}
	}
	entry := &referenceCatalog{loading: true}
	r.connectionReferences[key] = entry
	command := protocol.Command{Kind: "browse_database", Data: protocol.CommandBrowseDatabasePayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Database: database}}
	started := r.async(func() {
		out, err := r.controller.Do(r.ctx, command)
		r.enqueue(func(c *ui.Context) {
			if r.connectionReferences[key] != entry {
				return
			}
			entry.loading = false
			if !slices.ContainsFunc(referenceDatabases(r.snapshot(), scope), func(current protocol.DatabaseProfile) bool { return current.ID == profile.ID }) {
				return
			}
			catalog, valid := databaseCatalogOutput(out)
			if err == nil && !valid {
				err = fileActionFault(protocol.ErrorCodeUnavailable, "database catalog response is unavailable")
			}
			if err != nil {
				r.reportKeys(c, err, "reference_load_failed", "reference_load_failed", "reference_load_failed")
				return
			}
			entry.catalog = &catalog
		})
	})
	if !started {
		delete(r.connectionReferences, key)
	}
	return nil
}

func databaseCatalogOutput(out protocol.Output) (protocol.DatabaseCatalog, bool) {
	outcome, ok := detail[protocol.DatabaseOutcome](out.Data)
	if !ok || out.Kind != "database_outcome" || outcome.Kind != "catalog" {
		return protocol.DatabaseCatalog{}, false
	}
	return detail[protocol.DatabaseCatalog](outcome.Data)
}

// resetConnectionReferences forgets the settled catalogs of one composer so
// every opened picker browses again; reads in flight keep their entry.
func (r *Runtime) resetConnectionReferences(session string) {
	for key, entry := range r.connectionReferences {
		if !entry.loading && strings.HasPrefix(key, session+"\x00") {
			delete(r.connectionReferences, key)
		}
	}
}

func databaseReferenceTarget(id string) (protocol.ConversationReferenceTarget, bool) {
	var payload protocol.ConversationReferenceTargetDatabasePayload
	if json.Unmarshal([]byte(id), &payload) != nil {
		return protocol.ConversationReferenceTarget{}, false
	}
	return protocol.ConversationReferenceTarget{Kind: "database", Data: payload}, true
}

func sshReferenceTarget(id string) (protocol.ConversationReferenceTarget, bool) {
	value, ok := strings.CutPrefix(id, sshReferencePrefix)
	if !ok {
		return protocol.ConversationReferenceTarget{}, false
	}
	parsed, err := protocol.ParseSshID(value)
	if err != nil {
		return protocol.ConversationReferenceTarget{}, false
	}
	return protocol.ConversationReferenceTarget{Kind: "ssh", Data: parsed}, true
}

// connectionReferenceDetails returns the dialog subtitle key and body of a sent
// database or SSH reference while its profile is still referable.
func connectionReferenceDetails(snapshot protocol.Snapshot, scope connectionScope, target protocol.ConversationReferenceTarget) (string, string, bool) {
	switch target.Kind {
	case "ssh":
		id, ok := detail[protocol.SshID](target.Data)
		for _, profile := range referenceSsh(snapshot, scope) {
			if ok && profile.ID == id {
				return "reference_ssh", profile.Username + "@" + profile.Host + ":" + strconv.FormatUint(uint64(profile.Port), 10), true
			}
		}
	case "database":
		payload, ok := detail[protocol.ConversationReferenceTargetDatabasePayload](target.Data)
		for _, profile := range referenceDatabases(snapshot, scope) {
			if ok && profile.ID == payload.Connection {
				parts := []string{profile.Name}
				if payload.Database != nil {
					parts = append(parts, *payload.Database)
				}
				if payload.Table != nil {
					parts = append(parts, payload.Table.Schema+"."+payload.Table.Name)
				}
				return "reference_databases", strings.Join(parts, " / "), true
			}
		}
	}
	return "", "", false
}
