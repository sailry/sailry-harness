package live

import (
	"context"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testConnectionReferences covers the Databases and SSH pages of the `@`
// picker. Register it under an existing conversation test:
// t.Run("connection references", testConnectionReferences).
func testConnectionReferences(t *testing.T) {
	node, project, other := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.ProjectID{3}
	global := &protocol.ConnectionSharing{Kind: "global"}
	shared := &protocol.ConnectionSharing{Kind: "projects", Data: []protocol.ProjectID{project}}
	foreign := &protocol.ConnectionSharing{Kind: "projects", Data: &[]protocol.ProjectID{other}}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	file := protocol.DatabaseConnection{Kind: "sqlite", Data: protocol.DatabaseConnectionSqlitePayload{Path: "/fixture/data.db"}}
	snapshot.Databases = []protocol.DatabaseProfile{
		{ID: protocol.DatabaseID{1}, Revision: 4, Name: "billing", Sharing: global, Connection: file},
		{ID: protocol.DatabaseID{2}, Revision: 1, Name: "cache", Sharing: shared, Connection: file},
		{ID: protocol.DatabaseID{3}, Revision: 1, Name: "foreign", Sharing: foreign, Connection: file},
		{ID: protocol.DatabaseID{4}, Revision: 1, Name: "private", Connection: file},
	}
	snapshot.Ssh = []protocol.SshProfile{
		{ID: protocol.SshID{1}, Revision: 2, Name: "build", Host: "build.example", Port: 2222, Username: "ci", Authentication: protocol.SshAuthenticationPassword, Sharing: shared},
		{ID: protocol.SshID{2}, Revision: 1, Name: "foreign", Host: "other.example", Port: 22, Username: "root", Authentication: protocol.SshAuthenticationPassword, Sharing: foreign},
	}
	inProject := connectionScope{project: &project}
	billing := snapshot.Databases[0]

	t.Run("profiles follow sharing and the bound resource", func(t *testing.T) {
		names := func(scope connectionScope) string {
			text := ""
			for _, profile := range referenceDatabases(snapshot, scope) {
				text += profile.Name + " "
			}
			for _, profile := range referenceSsh(snapshot, scope) {
				text += "ssh:" + profile.Name + " "
			}
			return text
		}
		if got := names(inProject); got != "billing cache ssh:build " {
			t.Fatal("project scope", got)
		}
		if got := names(connectionScope{}); got != "" {
			t.Fatal("a conversation without a project listed connections", got)
		}
		bound := connectionScope{project: &project, resource: &protocol.ConnectionResource{Kind: "database", Data: protocol.DatabaseID{4}}}
		if got := names(bound); got != "private " {
			t.Fatal("bound database scope", got)
		}
		host := protocol.SshID{2}
		if got := names(connectionScope{resource: &protocol.ConnectionResource{Kind: "ssh", Data: &host}}); got != "ssh:foreign " {
			t.Fatal("bound SSH scope", got)
		}
	})
	t.Run("rows insert protocol targets", func(t *testing.T) {
		pages := databasePageRows(referenceDatabases(snapshot, inProject))
		if len(pages) != 2 || pages[0].Kind != composer.Page || pages[0].Source != connectionReferenceSource || pages[0].Label != "billing" {
			t.Fatalf("database pages: %+v", pages)
		}
		if id, database, ok := parseDatabasePage(pages[0].ID); !ok || id != billing.ID || database != nil {
			t.Fatal("profile page identity did not round-trip")
		}
		nested := "tenant/main"
		if id, database, ok := parseDatabasePage(databasePageID(billing.ID, &nested)); !ok || id != billing.ID || database == nil || *database != nested {
			t.Fatal("database page identity did not round-trip")
		}
		if _, _, ok := parseDatabasePage("databases"); ok {
			t.Fatal("the category page parsed as a profile page")
		}
		listed := databaseCatalogRows(billing, nil, protocol.DatabaseCatalog{Kind: "databases", Data: []string{"main", "audit"}})
		if len(listed) != 3 || listed[0].Kind != composer.Database || listed[0].Label != "reference_select_current" || !listed[0].Key || listed[0].Insert != "billing" || listed[1].Kind != composer.Page || listed[2].Label != "audit" {
			t.Fatalf("database list rows: %+v", listed)
		}
		main := "main"
		tables := databaseCatalogRows(billing, &main, protocol.DatabaseCatalog{Kind: "tables", Data: &protocol.DatabaseCatalogTablesPayload{Database: "main", Tables: []protocol.DatabaseTable{{Schema: "public", Name: "invoices"}, {Schema: "audit", Name: "invoices"}}}})
		if len(tables) != 3 || tables[0].Insert != "billing/main" || tables[1].Label != "main/invoices" || tables[1].Kind != composer.Database {
			t.Fatalf("table rows: %+v", tables)
		}
		seen := map[string]bool{}
		for _, row := range append(listed, tables...) {
			if seen[row.ID] {
				t.Fatal("sibling rows share an identity", row.ID)
			}
			seen[row.ID] = true
		}
		hosts := sshReferenceRows(referenceSsh(snapshot, inProject))
		if len(hosts) != 1 || hosts[0].Kind != composer.Location || hosts[0].Label != "build" {
			t.Fatalf("SSH rows: %+v", hosts)
		}
		current := tables[0]
		current.Label, current.Key = current.Insert, false
		input := conversationInput(composer.Message{Text: "@billing/main @main/invoices @build", References: []composer.Reference{current, tables[1], hosts[0]}})
		if len(input.References) != 3 {
			t.Fatalf("references were not submitted: %+v", input.References)
		}
		whole, ok := detail[protocol.ConversationReferenceTargetDatabasePayload](input.References[0].Target.Data)
		if !ok || input.References[0].Target.Kind != "database" || input.References[0].Label != "billing/main" || whole.Connection != billing.ID || whole.Database == nil || *whole.Database != "main" || whole.Table != nil {
			t.Fatalf("database target: %+v", input.References[0])
		}
		table, ok := detail[protocol.ConversationReferenceTargetDatabasePayload](input.References[1].Target.Data)
		if !ok || table.Table == nil || *table.Table != (protocol.DatabaseTable{Schema: "public", Name: "invoices"}) || *table.Database != "main" {
			t.Fatalf("table target: %+v", input.References[1])
		}
		ssh, ok := detail[protocol.SshID](input.References[2].Target.Data)
		if !ok || input.References[2].Target.Kind != "ssh" || ssh != (protocol.SshID{1}) {
			t.Fatalf("SSH target: %+v", input.References[2])
		}
		for index, chosen := range []composer.Reference{current, tables[1], hosts[0]} {
			restored, ok := composerReference(input.References[index])
			if !ok || restored.ID != chosen.ID || restored.Kind != chosen.Kind || restored.Label != chosen.Label {
				t.Fatalf("sent reference %d did not restore its composer identity: %+v", index, restored)
			}
		}
		if key, body, ok := connectionReferenceDetails(snapshot, inProject, input.References[1].Target); !ok || key != "reference_databases" || body != "billing / main / public.invoices" {
			t.Fatal("database details", key, body)
		}
		if key, body, ok := connectionReferenceDetails(snapshot, inProject, input.References[2].Target); !ok || key != "reference_ssh" || body != "ci@build.example:2222" {
			t.Fatal("SSH details", key, body)
		}
		if _, _, ok := connectionReferenceDetails(snapshot, connectionScope{project: &other}, input.References[2].Target); ok {
			t.Fatal("an unshared profile was described")
		}
	})
	t.Run("catalogs load from the captured Node per composer", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
		defer cancel()
		transport := newConnectionFixtureTransport(snapshot)
		transport.catalogs[billing.ID.String()] = protocol.DatabaseCatalog{Kind: "databases", Data: []string{"main"}}
		transport.gate = make(chan struct{})
		controller := client.New(transport)
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		r := &Runtime{ctx: ctx, controller: controller}
		updates := make(chan struct{}, 1)
		r.invalidate = func() {
			select {
			case updates <- struct{}{}:
			default:
			}
		}
		defer func() { cancel(); r.workers.Wait() }()
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			r.mu.Lock()
			queue := r.queue
			r.queue = nil
			r.mu.Unlock()
			for _, complete := range queue {
				complete(c)
			}
		}, 600, 400)
		tester.SetLocale("en")
		pump := func(done func() bool) {
			t.Helper()
			for !done() {
				tester.Frame()
				if done() {
					return
				}
				select {
				case <-updates:
				case <-ctx.Done():
					t.Fatal("catalog read did not settle")
				}
			}
		}
		settled := func(session string) func() bool {
			return func() bool {
				entry := r.connectionReferences[referenceCatalogKey(session, billing, nil)]
				return entry != nil && !entry.loading
			}
		}
		if rows := r.databaseReferences("first", inProject, billing, nil); rows != nil {
			t.Fatal("rows appeared before the Node answered")
		}
		if rows := r.databaseReferences("first", inProject, billing, nil); rows != nil || !transport.await(ctx, "browse_database", 1) {
			t.Fatal("a pending page was browsed twice")
		}
		var request protocol.CommandBrowseDatabasePayload
		transport.mu.Lock()
		for _, admitted := range transport.requests {
			if admitted.Command.Kind == "browse_database" {
				request, _ = detail[protocol.CommandBrowseDatabasePayload](admitted.Command.Data)
			}
		}
		transport.mu.Unlock()
		if request.Profile != billing.ID || request.ExpectedRevision != billing.Revision || request.Database != nil {
			t.Fatalf("browse request: %+v", request)
		}
		// A late answer belongs to the composer that asked for it.
		close(transport.gate)
		pump(settled("first"))
		if rows := r.databaseReferences("first", inProject, billing, nil); len(rows) != 2 || rows[1].Label != "main" {
			t.Fatalf("loaded rows: %+v", rows)
		}
		if rows := r.databaseReferences("second", inProject, billing, nil); rows != nil || !transport.await(ctx, "browse_database", 2) {
			t.Fatal("another composer reused a late answer")
		}
		pump(settled("second"))
		// Every opened picker browses again; another composer keeps its rows.
		r.resetConnectionReferences("first")
		if rows := r.databaseReferences("second", inProject, billing, nil); len(rows) != 2 || transport.count("browse_database") != 2 {
			t.Fatal("reopening one picker dropped another composer's rows")
		}
		transport.mu.Lock()
		transport.failure = protocol.ErrorCodeUnavailable
		transport.mu.Unlock()
		if rows := r.databaseReferences("first", inProject, billing, nil); rows != nil || !transport.await(ctx, "browse_database", 3) {
			t.Fatal("a reopened picker did not browse again")
		}
		pump(settled("first"))
		if rows := r.databaseReferences("first", inProject, billing, nil); rows != nil || transport.count("browse_database") != 3 {
			t.Fatal("a failed page retried without reopening the picker")
		}
	})
}
