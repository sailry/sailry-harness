//go:build integration

package node

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// These are real authenticated loopback links and local stores. Text indexing
// uses keyword mode, so no model, model download, or user profile is involved.
func TestMultiHostControllerRoutesIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 60*time.Second)
	defer cancel()
	start := func(name string) *Node {
		network := link.NetworkScope{Bind: "127.0.0.1:0"}
		environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
		node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), name), Options{
			Network: &network, Environment: &environment, MCP: fixtureMCPOptions(),
			knowledge: func(options *knowledgeservice.Options) {
				options.Choice = func() protocol.ModelsLocalChoice { return protocol.ModelsLocalChoice{} }
				options.Installed = func() (map[string]bool, error) { return map[string]bool{}, nil }
			},
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := node.Stop(context.Background()); err != nil {
				t.Error(err)
			}
		})
		return node
	}
	a, b, c := start("controller"), start("storage"), start("executor")
	phone, err := link.OpenController(ctx, filepath.Join(t.TempDir(), "phone"), link.NetworkScope{Bind: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = phone.Close() })
	pair := func(inviter *Node, claimant *link.Link) link.EndpointAddress {
		offer, err := inviter.Link().Invite(ctx)
		if err != nil {
			t.Fatal(err)
		}
		defer offer.Close()
		address, err := claimant.Pair(ctx, offer.Ticket())
		if err != nil {
			t.Fatal(err)
		}
		if err := offer.Paired(ctx); err != nil {
			t.Fatal(err)
		}
		return address
	}
	addressB := pair(b, a.Link())
	pair(c, a.Link())
	addressA := pair(a, phone)
	directB := pair(b, phone)
	connect := func(transport link.Transport) *client.Client {
		value := client.New(transport)
		t.Cleanup(func() { _ = value.Close() })
		if err := value.Start(ctx); err != nil {
			t.Fatal(err)
		}
		return value
	}
	desktop := connect(link.NewLocal(a.ID(), a.ID(), a))
	mobile := connect(phone.Remote(addressA))
	direct := connect(phone.Remote(directB))
	via := connect(phone.Via(addressA, b.ID()))
	wait := func(label string, ready func() bool) {
		t.Helper()
		ticker := time.NewTicker(20 * time.Millisecond)
		defer ticker.Stop()
		for !ready() {
			select {
			case <-ctx.Done():
				t.Fatalf("%s: %v", label, context.Cause(ctx))
			case <-ticker.C:
			}
		}
	}
	wait("controller host clients", func() bool {
		peers := a.controllerClients()
		return peers[b.ID()] != nil && peers[c.ID()] != nil && peers[b.ID()].Connected() && peers[c.ID()].Connected()
	})
	execute := func(owner *client.Client, request protocol.Request) protocol.Output {
		t.Helper()
		output, err := owner.ExecuteKnowledge(ctx, request)
		if err != nil {
			t.Fatalf("%s at %s: %v", request.Command.Kind, request.Target, err)
		}
		return output
	}
	collection := func(output protocol.Output) protocol.KnowledgeCollection {
		t.Helper()
		value, valid := dispatchPayload[protocol.KnowledgeCollection](output.Data)
		if !valid || output.Kind != "knowledge_collection" {
			t.Fatalf("unexpected collection output: %+v", output)
		}
		return value
	}
	definition := func(name, text string) protocol.KnowledgeDefinition {
		t.Helper()
		folder := t.TempDir()
		if err := os.WriteFile(filepath.Join(folder, "notes.md"), []byte(text), 0o600); err != nil {
			t.Fatal(err)
		}
		value := protocol.DefaultKnowledgeDefinition()
		value.Name = name
		value.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: folder}}
		return value
	}
	create := func(owner *client.Client, target protocol.NodeID, name, text string) (protocol.KnowledgeCollection, protocol.Request) {
		t.Helper()
		request := owner.PrepareKnowledge(protocol.KnowledgeRef{Node: target}, protocol.Command{Kind: "create_knowledge_collection",
			Data: &protocol.CommandCreateKnowledgeCollectionPayload{Node: &target, Definition: definition(name, text)}})
		return collection(execute(owner, request)), request
	}
	kb, original := create(desktop, b.ID(), "Controller private notes", "uniquecontroller bibliography")
	kc, _ := create(desktop, c.ID(), "Executor local notes", "executorlocal bibliography")
	phoneKB, _ := create(direct, b.ID(), "Host local notes", "hostlocal bibliography")
	if kb.Node != b.ID() || kc.Node != c.ID() || phoneKB.Node != b.ID() || kb.OwnerUser == phoneKB.OwnerUser {
		t.Fatal("storage Nodes or direct and routed user identities were conflated")
	}
	if outcome, err := desktop.KnowledgeOutcome(ctx, original); err != nil || outcome.Kind != "completed" {
		t.Fatalf("original create could not be recovered through its entry: %+v: %v", outcome, err)
	}
	if replay := collection(execute(desktop, original)); replay.ID != kb.ID {
		t.Fatal("replaying an admitted request created another library")
	}
	// A direct transport call must obey the explicit storage selection even
	// when the receiving Node's actor owns a matching collection ID.
	misrouted := protocol.NewRequest(b.ID(), protocol.Command{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{
		Node: &kc.Node, ID: kb.ID, ExpectedRevision: kb.Revision, Paused: true,
	}})
	storage := a.Link().Remote(addressB)
	if _, err := resourceResult(ctx, storage.Resource, protocol.ResourceCall{Local: true, Request: misrouted}); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("storage Node accepted a differently targeted mutation: %v", err)
	}
	project := func(owner *client.Client, name string) protocol.Project {
		t.Helper()
		draft := protocol.DefaultProjectsDraft()
		draft.Name, draft.Path = name, t.TempDir()
		draft.Visibility = protocol.ProjectVisibilityPrivate
		draft.Source = protocol.ProjectsSource{Kind: "local"}
		output, err := owner.Do(ctx, protocol.Command{Kind: "create_project", Data: &draft})
		value, valid := dispatchPayload[protocol.Project](output.Data)
		if err != nil || !valid {
			t.Fatalf("create project: %+v: %v", output, err)
		}
		return value
	}
	viaProject, directProject := project(via, "Controller private project"), project(direct, "Host local project")
	wait("same desktop and mobile source catalog", func() bool {
		left, err := desktop.ReadResourceCatalog(ctx)
		if err != nil || len(left.Sources) != 3 {
			return false
		}
		right, err := mobile.ReadResourceCatalog(ctx)
		if err != nil || len(right.Sources) != 3 {
			return false
		}
		for _, catalog := range []protocol.ResourceCatalog{left, right} {
			source := slices.IndexFunc(catalog.Sources, func(source protocol.ResourceSource) bool { return source.Node == b.ID() })
			if source < 0 || catalog.Sources[source].Fault != nil ||
				!slices.ContainsFunc(catalog.Sources[source].Knowledge, func(item protocol.KnowledgeCollection) bool { return item.ID == kb.ID }) ||
				slices.ContainsFunc(catalog.Sources[source].Knowledge, func(item protocol.KnowledgeCollection) bool { return item.ID == phoneKB.ID }) ||
				!slices.ContainsFunc(catalog.Sources[source].Projects, func(item protocol.Project) bool { return item.ID == viaProject.ID }) ||
				slices.ContainsFunc(catalog.Sources[source].Projects, func(item protocol.Project) bool { return item.ID == directProject.ID }) {
				return false
			}
		}
		return true
	})
	directSnapshot, err := direct.Do(ctx, protocol.Command{Kind: "snapshot"})
	snapshot, valid := dispatchPayload[protocol.Snapshot](directSnapshot.Data)
	if err != nil || !valid || !slices.ContainsFunc(snapshot.Projects, func(item protocol.Project) bool { return item.ID == directProject.ID }) ||
		slices.ContainsFunc(snapshot.Projects, func(item protocol.Project) bool { return item.ID == viaProject.ID }) {
		t.Fatalf("direct host view used the controller entry identity: %+v: %v", directSnapshot, err)
	}
	remoteUser := func(node *Node) protocol.User {
		t.Helper()
		var user protocol.User
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			user, err = store.CallerUser(ctx, db, node.ID(), a.ID())
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if user.Controller == nil || *user.Controller != a.ID() {
			t.Fatal("execution user lost its captured controller")
		}
		return user
	}
	executorCtx := store.WithUser(ctx, remoteUser(c))
	runtimeCall := func(request protocol.Request) (protocol.Output, error) {
		return c.dispatchCallbackResult(executorCtx, request)
	}
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Mode, query.Limit = "uniquecontroller", protocol.KnowledgeModeKeyword, 5
	var hit protocol.KnowledgeHit
	queryRequest := protocol.NewRequest(a.ID(), protocol.Command{Kind: "query_knowledge", Data: &query})
	wait("controller aggregates its directly connected stores", func() bool {
		output, err := desktop.ExecuteKnowledge(ctx, queryRequest)
		page, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
		if err != nil || !valid || len(page.Hits) == 0 {
			return false
		}
		hit = page.Hits[0]
		return hit.Citation.Node == b.ID() && hit.Citation.Collection == kb.ID
	})
	read := protocol.KnowledgeCitationRequest{Node: b.ID(), Collection: hit.Citation.Collection, Chunk: hit.Citation.Chunk, Revision: hit.Citation.Revision, Limit: 4096}
	readRequest := protocol.NewRequest(b.ID(), protocol.Command{Kind: "read_knowledge_citation", Data: &read})
	output, err := desktop.ExecuteKnowledge(ctx, readRequest)
	cited, valid := dispatchPayload[protocol.KnowledgeCitationPage](output.Data)
	if err != nil || !valid || !strings.Contains(cited.Text, "uniquecontroller") {
		t.Fatalf("entry did not read the original storage citation: %+v: %v", output, err)
	}
	for _, request := range []protocol.Request{queryRequest, readRequest} {
		if _, err := runtimeCall(request); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
			t.Fatalf("executor accepted a foreign knowledge target: %v", err)
		}
	}
	reverse := c.Link().Remote(addressA)
	for _, local := range []bool{false, true} {
		if _, err := reverse.Resource(executorCtx, protocol.ResourceCall{Local: local, Request: queryRequest}); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodePermissionDenied {
			t.Fatalf("entry accepted reverse knowledge access (local=%t): %v", local, err)
		}
	}
	pluginName := "scoped-knowledge"
	pluginSource := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + pluginName + `","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","knowledge.read","knowledge.write"]}}}`
	if err := os.WriteFile(filepath.Join(pluginSource, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := b.plugins.manager.Install(ctx, pluginSource, "", pluginName)
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("scoped knowledge fixture: %+v: %v", info.Issues, err)
	}
	owner := store.WithUser(ctx, remoteUser(b))
	if err := b.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(owner, db, info, 0, true)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(owner, db, b.ID(), []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	b.publish()
	kb.Definition.SharedWith = []string{pluginName}
	kb = collection(execute(desktop, desktop.PrepareKnowledge(protocol.KnowledgeRef{Node: b.ID()}, protocol.Command{
		Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{
			Node: &kb.Node, ID: kb.ID, ExpectedRevision: kb.Revision, Definition: kb.Definition,
		},
	})))
	var scopeWorktree protocol.WorktreeID
	wait("workspace scope reaches mobile execution view", func() bool {
		for _, worktree := range via.Snapshot().Worktrees {
			if worktree.Project != nil && *worktree.Project == viaProject.ID {
				scopeWorktree = worktree.ID
				return true
			}
		}
		return false
	})
	pluginScope := protocol.DefaultPluginContext()
	pluginScope.Package, pluginScope.Worktree = plugins.PackageReference(info), &scopeWorktree
	scopedQuery := protocol.DefaultKnowledgeQuery()
	scopedQuery.Query, scopedQuery.Mode, scopedQuery.Limit = "uniquecontroller", protocol.KnowledgeModeKeyword, 5
	scopedQuery.Refs = []protocol.KnowledgeRef{{Node: c.ID(), ID: kc.ID}}
	scopedRequest := protocol.NewRequest(c.ID(), protocol.Command{Kind: "query_knowledge", Data: &scopedQuery})
	scopedRequest.Plugin = &pluginScope
	workspace := phone.Via(addressA, b.ID())
	if _, err := workspace.ScopedKnowledge(ctx, protocol.ScopedKnowledgeCall{Request: scopedRequest}); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("workspace accepted another storage Node: %v", err)
	}
	scopedRequest.Target = b.ID()
	scopedQuery.Refs = append(scopedQuery.Refs, protocol.KnowledgeRef{Node: b.ID(), ID: kb.ID})
	if _, err := workspace.ScopedKnowledge(ctx, protocol.ScopedKnowledgeCall{Request: scopedRequest}); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("workspace accepted a mixed-Node selection: %v", err)
	}
	scopedQuery.Refs = nil
	var scopedHit protocol.KnowledgeHit
	wait("mobile workspace query uses its execution Node", func() bool {
		output, err := via.ExecuteScopedKnowledge(ctx, scopedRequest)
		page, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
		if err != nil {
			t.Fatalf("workspace query: %v", err)
		}
		if !valid || len(page.Hits) != 1 {
			return false
		}
		scopedHit = page.Hits[0]
		return scopedHit.Citation.Node == b.ID() && scopedHit.Citation.Collection == kb.ID
	})
	scopedRead := protocol.NewRequest(b.ID(), protocol.Command{Kind: "read_knowledge_citation", Data: &protocol.KnowledgeCitationRequest{
		Node: b.ID(), Collection: kb.ID, Chunk: scopedHit.Citation.Chunk, Revision: scopedHit.Citation.Revision, Limit: 4096,
	}})
	scopedRead.Plugin = &pluginScope
	output, err = via.ExecuteScopedKnowledge(ctx, scopedRead)
	scopedCitation, valid := dispatchPayload[protocol.KnowledgeCitationPage](output.Data)
	if err != nil || !valid || !strings.Contains(scopedCitation.Text, "uniquecontroller") {
		t.Fatalf("workspace citation lost its storage Node: %+v: %v", output, err)
	}
	if _, err := mobile.ExecuteScopedKnowledge(ctx, scopedRead); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("entry accepted the execution host's workspace scope: %v", err)
	}
	personalDefinition := protocol.DefaultKnowledgeDefinition()
	personalDefinition.Name = "Scoped personal library"
	personalDefinition.Source = protocol.KnowledgeSource{Kind: "storage", Data: &protocol.KnowledgeSourceStoragePayload{Scope: protocol.PluginStorageScopeNode}}
	scopedCreate := protocol.NewRequest(b.ID(), protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{
		Node: &addressB.ID, Definition: personalDefinition,
	}})
	scopedCreate.Plugin = &pluginScope
	output, err = via.ExecuteScopedKnowledge(ctx, scopedCreate)
	if err != nil || collection(output).Node != b.ID() {
		t.Fatalf("scoped create did not reach its execution Node: %+v: %v", output, err)
	}
	if recovered, err := via.ScopedKnowledgeOutcome(ctx, scopedCreate); err != nil || recovered.Kind != "completed" {
		t.Fatalf("scoped create recovery lost its original request: %+v: %v", recovered, err)
	}
	selectedLibrary := protocol.NewRequest(b.ID(), protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{
		Node: &kb.Node, Plugin: &pluginName, Definition: definition("Plugin library", "scopedplugin"),
	}})
	pluginLibrary := collection(execute(desktop, selectedLibrary))
	scopedPause := protocol.NewRequest(b.ID(), protocol.Command{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{
		Node: &pluginLibrary.Node, ID: pluginLibrary.ID, ExpectedRevision: pluginLibrary.Revision, Paused: true,
	}})
	scopedPause.Plugin = &pluginScope
	output, err = via.ExecuteScopedKnowledge(ctx, scopedPause)
	if err != nil {
		t.Fatal(err)
	}
	paused := collection(output)
	recoveredPause, err := mobile.KnowledgeOutcome(ctx, scopedPause)
	recoveredResult, valid := dispatchPayload[protocol.Result[protocol.Output, protocol.Fault]](recoveredPause.Data)
	if err != nil || recoveredPause.Kind != "completed" || !valid || recoveredResult.Ok == nil ||
		collection(*recoveredResult.Ok).ID != paused.ID || collection(*recoveredResult.Ok).Revision != paused.Revision {
		t.Fatalf("entry could not recover an admitted workspace operation at its storage Node: %+v: %v", recoveredPause, err)
	}
	phoneKB.Definition.Visibility = protocol.KnowledgeVisibilityShared
	phoneKB = collection(execute(direct, direct.PrepareKnowledge(protocol.KnowledgeRef{Node: b.ID()}, protocol.Command{
		Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{
			Node: &addressB.ID, ID: phoneKB.ID, ExpectedRevision: phoneKB.Revision, Definition: phoneKB.Definition,
		},
	})))
	sharedQuery := protocol.DefaultKnowledgeQuery()
	sharedQuery.Query, sharedQuery.Mode, sharedQuery.Limit = "hostlocal", protocol.KnowledgeModeKeyword, 5
	sharedQuery.Refs = []protocol.KnowledgeRef{{Node: b.ID(), ID: phoneKB.ID}}
	var sharedHit protocol.KnowledgeHit
	wait("shared library visible without another grant", func() bool {
		output, err := desktop.ExecuteKnowledge(ctx, protocol.NewRequest(a.ID(), protocol.Command{Kind: "query_knowledge", Data: &sharedQuery}))
		page, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
		if err != nil || !valid || len(page.Hits) != 1 {
			return false
		}
		sharedHit = page.Hits[0]
		return sharedHit.Citation.Collection == phoneKB.ID
	})
	sharedRef := protocol.KnowledgeRef{Node: b.ID(), ID: phoneKB.ID}
	if _, err := desktop.ExecuteKnowledge(ctx, protocol.NewRequest(b.ID(), protocol.Command{Kind: "delete_knowledge_collection",
		Data: &protocol.CommandDeleteKnowledgeCollectionPayload{Node: &sharedRef.Node, ID: sharedRef.ID, ExpectedRevision: phoneKB.Revision}})); err == nil {
		t.Fatal("a shared reader deleted its owner's library")
	}
	phoneKB.Definition.Visibility = protocol.KnowledgeVisibilityPrivate
	execute(direct, direct.PrepareKnowledge(sharedRef, protocol.Command{Kind: "update_knowledge_collection",
		Data: &protocol.CommandUpdateKnowledgeCollectionPayload{
			Node: &sharedRef.Node, ID: sharedRef.ID, ExpectedRevision: phoneKB.Revision, Definition: phoneKB.Definition,
		}}))
	sharedRead := protocol.KnowledgeCitationRequest{Node: b.ID(), Collection: sharedHit.Citation.Collection, Chunk: sharedHit.Citation.Chunk, Revision: sharedHit.Citation.Revision, Limit: 4096}
	if _, err := desktop.ExecuteKnowledge(ctx, protocol.NewRequest(b.ID(), protocol.Command{Kind: "read_knowledge_citation", Data: &sharedRead})); err == nil {
		t.Fatal("a citation remained accessible after shared visibility was removed")
	}
	runtimeRequest := protocol.NewRequest(c.ID(), protocol.Command{Kind: "create_knowledge_collection",
		Data: &protocol.CommandCreateKnowledgeCollectionPayload{Node: &kc.Node, Definition: definition("Runtime library", "runtimecreated")}})
	output, err = runtimeCall(runtimeRequest)
	if err != nil {
		t.Fatal(err)
	}
	created := collection(output)
	if created.Node != c.ID() || created.OwnerUser != kc.OwnerUser {
		t.Fatal("executor mutation reached a different storage owner")
	}
	viaFeed, err := phone.Via(addressA, b.ID()).Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer viaFeed.Close()
	if _, err := viaFeed.Next(ctx); err != nil {
		t.Fatal(err)
	}
	phoneAddress, err := phone.Address(ctx)
	if err != nil {
		t.Fatal(err)
	}
	output, err = desktop.Do(ctx, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device",
		Data: protocol.RevokeDevice{Device: phoneAddress.ID, ConfirmLast: true}}})
	if err != nil || output.Kind != "device_revoked" {
		t.Fatalf("revoke entry device: %+v: %v", output, err)
	}
	wait("revoked entry closes its routed subscription", func() bool {
		next, release := context.WithTimeout(ctx, 100*time.Millisecond)
		defer release()
		_, err := viaFeed.Next(next)
		return err != nil && ctx.Err() == nil && !errors.Is(err, context.DeadlineExceeded)
	})
	if _, err := mobile.ReadResourceCatalog(ctx); err == nil {
		t.Fatal("revoked mobile retained the controller catalog")
	}
	if _, err := via.Do(ctx, protocol.Command{Kind: "snapshot"}); err == nil {
		t.Fatal("revoked mobile retained the routed host")
	}
	if _, err := direct.Do(ctx, protocol.Command{Kind: "snapshot"}); err != nil {
		t.Fatalf("entry revocation broke the independent direct host route: %v", err)
	}
	localQuery := protocol.DefaultKnowledgeQuery()
	localQuery.Query, localQuery.Mode, localQuery.Limit = "executorlocal", protocol.KnowledgeModeKeyword, 5
	wait("executor local index", func() bool {
		output, err := runtimeCall(protocol.NewRequest(c.ID(), protocol.Command{Kind: "query_knowledge", Data: &localQuery}))
		page, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
		return err == nil && valid && len(page.Hits) == 1
	})
	if err := a.Link().Close(); err != nil {
		t.Fatal(err)
	}
	output, err = runtimeCall(protocol.NewRequest(c.ID(), protocol.Command{Kind: "query_knowledge", Data: &localQuery}))
	localPage, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
	if err != nil || !valid || len(localPage.Hits) != 1 || localPage.Hits[0].Citation.Node != c.ID() {
		t.Fatalf("local knowledge depended on an offline controller: %+v: %v", output, err)
	}
}
