package live

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type providerLocation struct {
	owner    *Runtime
	provider protocol.ConversationProvider
}

type providerMove struct {
	id      protocol.ProviderID
	target  *Runtime
	created uint64
	old     []providerLocation
}

func providerMoveKey(node string, id protocol.ProviderID) string {
	return node + "/" + id.String()
}

func uniqueProviderNodes(nodes []string) []string {
	result := make([]string, 0, len(nodes))
	for _, node := range nodes {
		if node != "" && !slices.Contains(result, node) {
			result = append(result, node)
		}
	}
	slices.Sort(result)
	return result
}

func providerTargetExpectedRevision(node, sourceNode string, sourceRevision uint64, targetRevisions map[string]uint64) uint64 {
	if revision, captured := targetRevisions[node]; captured {
		return revision
	}
	if node == sourceNode {
		return sourceRevision
	}
	return 0
}

func providerMember(node string, id protocol.ProviderID) (preferences.ProviderMember, bool) {
	var member preferences.ProviderMember
	decoded, err := hex.DecodeString(node)
	if err != nil || len(decoded) != len(member.Node) || id == (protocol.ProviderID{}) {
		return member, false
	}
	copy(member.Node[:], decoded)
	member.Provider = id
	return member, true
}

func providerGroup(data preferences.Data, id string) (preferences.ProviderGroup, bool) {
	if id == "" {
		return preferences.ProviderGroup{}, false
	}
	parsed, err := protocol.ParseRequestID(id)
	if err != nil {
		return preferences.ProviderGroup{}, false
	}
	for _, group := range data.ProviderGroups {
		if group.ID == parsed {
			return group, true
		}
	}
	return preferences.ProviderGroup{}, false
}

func providerGroupFor(data preferences.Data, node string, id protocol.ProviderID) (preferences.ProviderGroup, []preferences.ProviderMember) {
	member, valid := providerMember(node, id)
	if !valid {
		return preferences.ProviderGroup{}, nil
	}
	for _, group := range data.ProviderGroups {
		if slices.Contains(group.Members, member) {
			return group, slices.Clone(group.Members)
		}
	}
	return preferences.ProviderGroup{}, []preferences.ProviderMember{member}
}

func providerGroupForSave(data preferences.Data, node string, id protocol.ProviderID, revision uint64) (preferences.ProviderGroup, []preferences.ProviderMember) {
	group, members := providerGroupFor(data, node, id)
	if revision == 0 && group.ID == (protocol.RequestID{}) {
		return group, nil
	}
	return group, members
}

func runtimeOwners(root *Runtime) []*Runtime {
	owners := []*Runtime{root}
	for _, node := range slices.Sorted(maps.Keys(root.bindings)) {
		if owner := root.bindings[node]; owner != nil {
			owners = append(owners, owner)
		}
	}
	return owners
}

func providerLocations(root *Runtime, members []preferences.ProviderMember) map[preferences.ProviderMember]providerLocation {
	wanted := make(map[preferences.ProviderMember]bool, len(members))
	for _, member := range members {
		wanted[member] = true
	}
	result := make(map[preferences.ProviderMember]providerLocation, len(members))
	for _, owner := range runtimeOwners(root) {
		node := owner.controller.Target()
		for _, provider := range owner.snapshot().Providers {
			member := preferences.ProviderMember{Node: node, Provider: provider.ID}
			if wanted[member] {
				result[member] = providerLocation{owner: owner, provider: provider}
			}
		}
	}
	return result
}

// A partial save keeps the identity of each target and its original request
// until the editor closes. Receipts can then be inspected after reconnecting.
type providerSaveTarget struct {
	id            protocol.ProviderID
	request       *protocol.Request
	intent        [sha256.Size]byte
	requestIntent [sha256.Size]byte
	saved         *protocol.ConversationProvider
}

type providerSaveBatch map[string]*providerSaveTarget

func providerSaveIntent(d settings.ProviderDraft) [sha256.Size]byte {
	d.Channel.Group, d.Group = "", ""
	d.Channel.Targets, d.Targets = nil, nil
	d.Channel.TargetIDs = nil
	d.Channel.TargetRevisions, d.TargetRevisions = nil, nil
	d.Channel.Revision, d.ExpectedRevision = 0, 0
	b, _ := json.Marshal(d)
	return sha256.Sum256(b)
}

func (r *Runtime) saveManagedProvider(d settings.ProviderDraft, done func(settings.ProviderSaveResult)) {
	if done == nil {
		return
	}
	batch := providerSaveBatch{}
	batch.save(r.localRuntime(), d, done)
}

func (batch providerSaveBatch) save(root *Runtime, d settings.ProviderDraft, finished func(settings.ProviderSaveResult)) {
	done := func(result settings.ProviderSaveResult) {
		for _, node := range result.Failed {
			if target := batch[node]; target != nil && target.request != nil {
				result.Uncertain = append(result.Uncertain, node)
			}
		}
		result.Failed = slices.DeleteFunc(result.Failed, func(node string) bool { return slices.Contains(result.Uncertain, node) })
		if len(result.Failed) != 0 || len(result.Uncertain) != 0 {
			result.Resume = func(next settings.ProviderDraft, callback func(settings.ProviderSaveResult)) {
				batch.save(root, next, callback)
			}
		}
		finished(result)
	}
	// Inspect each unresolved effect before considering the revised draft,
	// including targets which have since been deselected.
	for node, target := range batch {
		if target.request == nil {
			continue
		}
		owner := root.providerOwner(node)
		if owner == nil {
			done(providerSaveFailure([]string{node}))
			return
		}
		request := *target.request
		owner.async(func() {
			resolution, err := owner.controller.Resolve(owner.ctx, request)
			owner.enqueue(func(c *ui.Context) {
				if err == nil {
					output, outcome := resolution.Result()
					switch resolution.State {
					case client.RequestApplied:
						saved, ok := detail[protocol.ConversationProvider](output.Data)
						if ok && saved.ID == target.id {
							target.request, target.saved, target.intent = nil, &saved, target.requestIntent
							owner.refreshProviderSnapshot()
							batch.save(root, d, finished)
							return
						}
					case client.RequestAbsent:
						target.request = nil
						batch.save(root, d, finished)
						return
					case client.RequestFailed:
						target.request = nil
					}
					err = outcome
					if resolution.State == client.RequestApplied {
						err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "provider save returned no matching channel"}
					}
				}
				owner.reportKeys(c, err, "provider_save_failed", "provider_revision_changed", "provider_save_unknown")
				done(providerSaveFailure([]string{node}))
			})
		})
		return
	}
	sourceNode := d.Channel.Node
	if sourceNode == "" {
		sourceNode = root.providerDefaultTarget()
	}
	source := root.providerOwner(sourceNode)
	if source == nil {
		done(providerSaveFailure(d.Targets))
		return
	}
	d.Channel.Node = sourceNode
	provider := source.providerDraft(d)
	if provider.ID == (protocol.ProviderID{}) {
		done(providerSaveFailure(d.Targets))
		return
	}
	if d.TargetRevisions == nil {
		d.TargetRevisions = d.Channel.TargetRevisions
	}
	nodes := uniqueProviderNodes(d.Targets)
	if len(nodes) == 0 {
		nodes = []string{sourceNode}
	}
	if provider.Authentication != protocol.AuthenticationApiKey && provider.Authentication != "" && len(nodes) != 1 {
		done(providerSaveFailure(nodes))
		return
	}
	group, oldMembers := providerGroupForSave(root.Preferences.Data, sourceNode, provider.ID, d.ExpectedRevision)
	if provider.Authentication != protocol.AuthenticationApiKey && provider.Authentication != "" && group.ID != (protocol.RequestID{}) {
		done(providerSaveFailure(nodes))
		return
	}
	if d.Group != "" && len(nodes) > 1 {
		selected, exists := providerGroup(root.Preferences.Data, d.Group)
		related := slices.Contains(selected.Members, mustProviderMember(sourceNode, provider.ID)) || slices.ContainsFunc(selected.Members, func(member preferences.ProviderMember) bool {
			target := batch[member.Node.String()]
			return target != nil && target.saved != nil && target.id == member.Provider
		})
		if !exists || !related {
			done(providerSaveFailure(nodes))
			return
		}
		group, oldMembers = selected, slices.Clone(selected.Members)
	}
	for node, target := range batch {
		member, ok := providerMember(node, target.id)
		if ok && target.saved != nil && !slices.Contains(oldMembers, member) {
			oldMembers = append(oldMembers, member)
		}
	}
	oldByMember := providerLocations(root, oldMembers)
	oldByNode := make(map[string]providerLocation, len(oldMembers))
	oldIDByNode := make(map[string]protocol.ProviderID, len(oldMembers))
	for _, member := range oldMembers {
		node := member.Node.String()
		oldIDByNode[node] = member.Provider
		location, exists := oldByMember[member]
		if target := batch[node]; target != nil && target.saved != nil {
			location, exists = providerLocation{owner: root.providerOwner(node), provider: *target.saved}, true
		}
		if exists && location.owner != nil {
			revision := providerTargetExpectedRevision(node, sourceNode, d.ExpectedRevision, d.TargetRevisions)
			if target := batch[node]; target != nil && target.saved != nil {
				revision = target.saved.Revision
			}
			location.provider.Revision = revision
			oldByNode[node] = location
		}
	}

	targets := make([]*Runtime, 0, len(nodes))
	targetIDs := make(map[string]protocol.ProviderID, len(nodes))
	for _, node := range nodes {
		owner := root.providerOwner(node)
		if owner == nil {
			targets = append(targets, nil)
			continue
		}
		targets = append(targets, owner)
		if target := batch[node]; target != nil {
			targetIDs[node] = target.id
			continue
		}
		if id, exists := oldIDByNode[node]; exists {
			targetIDs[node] = id
			continue
		}
		if id, exists := d.Channel.TargetIDs[node]; exists {
			if parsed, err := protocol.ParseProviderID(id); err == nil {
				if _, exists := owner.provider(parsed.String()); exists {
					targetIDs[node] = parsed
					continue
				}
			}
		}
		id := provider.ID
		if _, collision := owner.provider(id.String()); collision {
			id = protocol.NewProviderID()
		}
		targetIDs[node] = id
	}

	save := func(sourceKey string) {
		root.saveProviderTargets(batch, providerSaveIntent(d), provider, group, oldMembers, oldByNode, nodes, targets, targetIDs, sourceNode, d.ExpectedRevision, d.TargetRevisions, d.Key, sourceKey, done)
	}
	location := oldByNode[sourceNode]
	if provider.Authentication == protocol.AuthenticationApiKey && strings.TrimSpace(d.Key) == "" && location.provider.Credential != nil {
		for index, node := range nodes {
			if targets[index] != nil && oldByNode[node].provider.Credential == nil {
				source.readProviderKeyForSave(location.provider, func(key string, ok bool) {
					if !ok {
						done(providerSaveFailure(nodes))
						return
					}
					save(key)
				})
				return
			}
		}
	}
	save("")
}

func mustProviderMember(node string, id protocol.ProviderID) preferences.ProviderMember {
	member, _ := providerMember(node, id)
	return member
}

func providerSaveFailure(nodes []string) settings.ProviderSaveResult {
	return settings.ProviderSaveResult{Failed: uniqueProviderNodes(nodes)}
}

func (r *Runtime) saveProviderTargets(batch providerSaveBatch, intent [sha256.Size]byte, provider protocol.ConversationProvider, oldGroup preferences.ProviderGroup, oldMembers []preferences.ProviderMember,
	oldByNode map[string]providerLocation, nodes []string, targets []*Runtime, ids map[string]protocol.ProviderID,
	sourceNode string, sourceRevision uint64, targetRevisions map[string]uint64, changedKey, sourceKey string,
	done func(settings.ProviderSaveResult)) {
	root := r.localRuntime()
	groupID := oldGroup.ID
	if groupID == (protocol.RequestID{}) && (len(nodes) > 1 || len(batch) > 1) {
		groupID = protocol.NewRequestID()
	}

	members := make(map[protocol.NodeID]preferences.ProviderMember, len(oldMembers)+len(nodes))
	for _, member := range oldMembers {
		members[member.Node] = member
	}
	succeeded := make([]string, 0, len(nodes))
	failed := make([]string, 0, len(nodes))
	savedRevisions := make(map[string]uint64, len(nodes))
	var pending int
	for index, node := range nodes {
		owner := targets[index]
		if owner == nil {
			failed = append(failed, node)
			continue
		}
		id := ids[node]
		member := preferences.ProviderMember{Node: owner.controller.Target(), Provider: id}
		location, exists := oldByNode[node]
		if !exists {
			if previous, found := providerLocations(root, []preferences.ProviderMember{member})[member]; found {
				location, exists = previous, true
			}
		}
		copy := provider
		copy.ID = id
		copy.Revision = 0
		copy.Source, copy.Owner, copy.Shared = nil, nil, false
		copy.Credential = nil
		expected := providerTargetExpectedRevision(node, sourceNode, sourceRevision, targetRevisions)
		if exists {
			copy.Credential = location.provider.Credential
		}
		target := batch[node]
		if target == nil {
			target = &providerSaveTarget{id: id}
			batch[node] = target
		}
		if target.saved != nil {
			expected = target.saved.Revision
			if target.intent == intent {
				members[member.Node] = member
				savedRevisions[node] = expected
				succeeded = append(succeeded, node)
				continue
			}
		}
		pending++
		secret := providerSaveKey(changedKey, sourceKey, copy.Credential)
		command := protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: copy, ExpectedRevision: expected, Secret: secret}}
		request := owner.controller.Prepare(command)
		owner.async(func() {
			output, err := owner.controller.Settle(owner.ctx, request)
			owner.enqueue(func(c *ui.Context) {
				saved, valid := detail[protocol.ConversationProvider](output.Data)
				if err == nil && (!valid || saved.ID != id) {
					err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "provider save returned no matching channel"}
				}
				owner.reportKeys(c, err, "provider_save_failed", "provider_revision_changed", "provider_save_unknown")
				if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
					target.request, target.requestIntent = &request, intent
				}
				if err != nil {
					failed = append(failed, node)
				} else {
					if owner.authorizations == nil {
						owner.initAuthorization()
					}
					target.saved, target.intent = &saved, intent
					owner.authorizations.savedProviders[id.String()] = saved
					members[member.Node] = member
					savedRevisions[node] = saved.Revision
					succeeded = append(succeeded, node)
					owner.refreshProviderSnapshot()
				}
				pending--
				if pending == 0 {
					root.finishManagedProviderSave(batch, groupID, members, nodes, succeeded, failed, ids, savedRevisions, oldMembers, oldByNode, provider.Authentication, done)
				}
			})
		})
	}
	if pending == 0 {
		root.finishManagedProviderSave(batch, groupID, members, nodes, succeeded, failed, ids, savedRevisions, oldMembers, oldByNode, provider.Authentication, done)
	}
}

func providerSaveKey(changed, inherited string, existing *protocol.CredentialRef) *protocol.Secret {
	value := strings.TrimSpace(changed)
	if value == "" && existing == nil {
		value = inherited
	}
	if value == "" {
		return nil
	}
	key := protocol.NewSecret(value)
	return &key
}

func (r *Runtime) finishManagedProviderSave(batch providerSaveBatch, groupID protocol.RequestID, members map[protocol.NodeID]preferences.ProviderMember,
	nodes, succeeded, failed []string, ids map[string]protocol.ProviderID, savedRevisions map[string]uint64,
	oldMembers []preferences.ProviderMember, oldByNode map[string]providerLocation,
	auth protocol.Authentication, done func(settings.ProviderSaveResult)) {
	root := r.localRuntime()
	if auth != protocol.AuthenticationApiKey && auth != "" && len(nodes) == 1 && len(succeeded) == 1 {
		selected := nodes[0]
		var stale []providerLocation
		var staleUnknown []preferences.ProviderMember
		for _, member := range oldMembers {
			if member.Node.String() == selected {
				continue
			}
			if location, exists := oldByNode[member.Node.String()]; exists {
				stale = append(stale, location)
			} else {
				staleUnknown = append(staleUnknown, member)
			}
			delete(members, member.Node)
		}
		if len(stale) > 0 || len(staleUnknown) > 0 {
			id := ids[selected]
			target := r.providerOwner(selected)
			if target != nil && id != (protocol.ProviderID{}) && savedRevisions[selected] > 0 {
				if root.providerMoves == nil {
					root.providerMoves = map[string]providerMove{}
				}
				root.providerMoves[providerMoveKey(selected, id)] = providerMove{id: id, target: target, created: savedRevisions[selected], old: stale}
			}
			for _, member := range staleUnknown {
				failed = append(failed, member.Node.String())
			}
		}
		r.finishManagedProviderGroup(protocol.RequestID{}, nil, members, succeeded, failed, savedRevisions, done)
		return
	}
	// The destinations are explicit writes. Existing copies elsewhere remain
	// independent and keep their identities when omitted from the save.
	root.finishManagedProviderGroup(groupID, batch, members, succeeded, failed, savedRevisions, done)
}

func (r *Runtime) finishManagedProviderGroup(groupID protocol.RequestID, batch providerSaveBatch, members map[protocol.NodeID]preferences.ProviderMember,
	succeeded, failed []string, savedRevisions map[string]uint64, done func(settings.ProviderSaveResult)) {
	root := r.localRuntime()
	group, _ := providerGroup(root.Preferences.Data, groupID.String())
	if len(batch) > 1 {
		group.Members = root.recordProviderCopies(groupID, group.Members, batch)
	}
	result := settings.ProviderSaveResult{Succeeded: uniqueProviderNodes(succeeded), Failed: uniqueProviderNodes(failed)}
	if len(group.Members) > 1 {
		result.Group = groupID.String()
	}
	result.TargetIDs = make(map[string]string, len(members))
	result.TargetRevisions = maps.Clone(savedRevisions)
	for _, member := range members {
		result.TargetIDs[member.Node.String()] = member.Provider.String()
	}
	done(result)
	root.projectProviderSnapshot()
}

// recordProviderCopies merges confirmed saves into the current association,
// including a destination whose receipt was resolved after it was unchecked.
func (r *Runtime) recordProviderCopies(groupID protocol.RequestID, previous []preferences.ProviderMember, batch providerSaveBatch) []preferences.ProviderMember {
	current := make(map[protocol.NodeID]preferences.ProviderMember, len(previous)+len(batch))
	for _, member := range previous {
		current[member.Node] = member
	}
	for node, target := range batch {
		if target.saved != nil {
			if member, ok := providerMember(node, target.saved.ID); ok {
				current[member.Node] = member
			}
		}
	}
	ordered := make([]preferences.ProviderMember, 0, len(current))
	for _, member := range current {
		ordered = append(ordered, member)
	}
	slices.SortFunc(ordered, func(a, b preferences.ProviderMember) int {
		if order := strings.Compare(a.Node.String(), b.Node.String()); order != 0 {
			return order
		}
		return strings.Compare(a.Provider.String(), b.Provider.String())
	})
	if len(ordered) > 1 && !slices.Equal(previous, ordered) {
		r.replaceProviderGroup(groupID, ordered)
	}
	return ordered
}

func (r *Runtime) replaceProviderGroup(id protocol.RequestID, members []preferences.ProviderMember) {
	root := r.localRuntime()
	groups := slices.DeleteFunc(slices.Clone(root.Preferences.Data.ProviderGroups), func(group preferences.ProviderGroup) bool { return group.ID == id })
	if len(members) >= 2 {
		groups = append(groups, preferences.ProviderGroup{ID: id, Members: slices.Clone(members)})
	}
	root.Preferences.Data.ProviderGroups = groups
	root.wake()
}

func (r *Runtime) removeProviderLocations(locations []providerLocation, done func([]string)) {
	if len(locations) == 0 {
		done(nil)
		return
	}
	pending := len(locations)
	var failed []string
	for _, location := range locations {
		owner, provider := location.owner, location.provider
		command := protocol.Command{Kind: "remove_provider", Data: protocol.CommandRemoveProviderPayload{Provider: provider.ID, ExpectedRevision: provider.Revision}}
		owner.providerCommand(command, func(_ protocol.Output, ok bool) {
			if ok {
				if owner.authorizations != nil {
					delete(owner.authorizations.savedProviders, provider.ID.String())
				}
			} else {
				failed = append(failed, owner.controller.Target().String())
			}
			pending--
			if pending == 0 {
				done(failed)
			}
		})
	}
}

func (r *Runtime) finishProviderMove(move providerMove) {
	root := r.localRuntime()
	key := providerMoveKey(move.target.controller.Target().String(), move.id)
	root.removeProviderLocations(move.old, func(failed []string) {
		if len(failed) != 0 {
			return
		}
		delete(root.providerMoves, key)
		root.projectProviderSnapshot()
	})
}

func (r *Runtime) providerLoginFinished(id protocol.ProviderID, success bool) {
	root := r.localRuntime()
	if root.providerMoves == nil {
		return
	}
	key := providerMoveKey(r.controller.Target().String(), id)
	move, exists := root.providerMoves[key]
	if !exists {
		return
	}
	if success {
		root.finishProviderMove(move)
		return
	}
	delete(root.providerMoves, key)
	if move.created == 0 {
		return
	}
	move.target.providerCommand(protocol.Command{Kind: "remove_provider", Data: protocol.CommandRemoveProviderPayload{Provider: id, ExpectedRevision: move.created}}, func(_ protocol.Output, ok bool) {
		if ok {
			if move.target.authorizations != nil {
				delete(move.target.authorizations.savedProviders, id.String())
			}
			root.projectProviderSnapshot()
		}
	})
}

func providerIDFromString(value string) protocol.ProviderID {
	id, _ := protocol.ParseProviderID(value)
	return id
}

func (r *Runtime) enableManagedProvider(node, id string, revision uint64, enabled bool) {
	root := r.localRuntime()
	if node == "" {
		return
	}
	owner := root.providerOwner(node)
	if owner == nil {
		root.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the settings host is unavailable"))
		root.projectProviderSnapshot()
		return
	}
	provider, exists := owner.provider(id)
	if !exists {
		root.Report(fileActionFault(protocol.ErrorCodeNotFound, "provider is unavailable"))
		root.projectProviderSnapshot()
		return
	}
	provider.Enabled = enabled
	owner.providerCommand(protocol.Command{Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: provider, ExpectedRevision: revision}}, func(_ protocol.Output, ok bool) {
		if ok {
			owner.refreshProviderSnapshot()
		} else {
			root.projectProviderSnapshot()
		}
	})
}

func (r *Runtime) removeManagedProvider(node, id string, revision uint64) {
	root := r.localRuntime()
	providerID, err := protocol.ParseProviderID(id)
	if node == "" || err != nil {
		return
	}
	owner := root.providerOwner(node)
	if owner == nil {
		root.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the settings host is unavailable"))
		return
	}
	command := protocol.Command{Kind: "remove_provider", Data: protocol.CommandRemoveProviderPayload{Provider: providerID, ExpectedRevision: revision}}
	owner.providerCommand(command, func(_ protocol.Output, ok bool) {
		if !ok {
			return
		}
		if owner.authorizations != nil {
			delete(owner.authorizations.savedProviders, id)
		}
		group, members := providerGroupFor(root.Preferences.Data, node, providerID)
		if group.ID != (protocol.RequestID{}) {
			member := preferences.ProviderMember{Node: owner.controller.Target(), Provider: providerID}
			remaining := slices.DeleteFunc(members, func(candidate preferences.ProviderMember) bool { return candidate == member })
			root.replaceProviderGroup(group.ID, remaining)
		}
		owner.refreshProviderSnapshot()
	})
}

func (r *Runtime) readManagedProviderKey(node, id string, done func(string)) {
	owner := r.localRuntime().providerOwner(node)
	if owner == nil {
		if done != nil {
			done("")
		}
		return
	}
	owner.readProviderKey(id, done)
}
