package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"maps"
	"slices"
	"strconv"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	view "github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type syncedNodeEntry struct {
	id             protocol.NodeID
	client         *client.Client
	label          string
	local          bool
	projectSources []string
	tools          []protocol.HostTool
	snapTools      []protocol.HostTool
	canClone       bool
	snapRev        uint64
	hasSnap        bool
	credentials    map[protocol.CredentialID]protocol.Credential
	credKey        string
	modelOptions   []kit.ModelOption
	providerRev    uint64
	scoreRev       uint64
	presetCount    int
}

type usageWatchEntry struct {
	close func()
}

type windowViewHost struct {
	runtime *Runtime
	mounts  map[*nativeWindowResource]struct{}

	mu              sync.Mutex
	closed          bool
	synced          bool
	locale          string
	syncedNodes     map[protocol.NodeID]syncedNodeEntry
	rememberedNames map[protocol.NodeID]string
	network         *Network
	demoOn          bool
	demoUsage       *preview.Usage
	demoRevision    uint64
	changeSubs      map[uint64]func()
	nextSubID       uint64
	clientUnsubs    map[*client.Client]func()
	netUnsub        func()
	observedNet     *Network
	watches         map[*usageWatchEntry]struct{}
}

func (h *windowViewHost) services() view.Services {
	r, controller := h.runtime, h.runtime.controller
	h.syncState(true)
	r.nativeWindowResources = append(r.nativeWindowResources, nativeWindowResource{
		beforeFrame: func() { h.syncState(false) },
		close:       h.close,
	})
	return view.Services{
		Node: controller.Target(), Connected: func() bool { return h.open() && controller.Connected() },
		Next: h.next, Post: h.post, Wake: r.wake,
		Async: func(work func()) bool { return work != nil && h.open() && r.async(work) },
		Prepare: func(command protocol.Command) protocol.Request {
			if !h.open() {
				return protocol.Request{}
			}
			return controller.Prepare(command)
		},
		Execute: func(ctx context.Context, request protocol.Request) (protocol.Output, error) {
			return windowRequest(h, ctx, request, controller.Execute)
		},
		Settle: func(ctx context.Context, request protocol.Request) (protocol.Output, error) {
			return windowRequest(h, ctx, request, controller.Settle)
		},
		Resolve: h.resolve,
		Outcome: func(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
			return windowRequest(h, ctx, request, controller.Outcome)
		},
		Nodes:           h.nodes,
		NodeLabel:       h.nodeLabel,
		ModelOptions:    h.modelOptions,
		PrepareOn:       h.prepareOn,
		ExecuteOn:       h.executeOn,
		SettleOn:        h.settleOn,
		ResolveOn:       h.resolveOn,
		OutcomeOn:       h.outcomeOn,
		Snapshot:        h.snapshot,
		ProviderAccount: h.providerAccount,
		Credentials:     h.credentials,
		OpenSession:     h.openSession,
		RevealDetails:   h.revealDetails,
		OnChange:        h.onChange,
		Usage:           h.usageServices(),
		TerminalLaunch: func(c *ui.Context) any {
			return protocol.SshTerminalLaunch{Viewport: terminalViewport, Appearance: pluginNativeAppearance(theme.Of(c))}
		},
		MountTerminal: h.mountTerminal, MountConversation: h.mountConversation,
		PickFiles: h.pickFiles, PickFolder: h.pickFolder, PickDestination: h.pickDestination, PickSecret: h.pickSecret,
		WriteClipboard: h.writeClipboard, Preview: h.preview,
		OpenSource: h.openSource, OpenDestination: h.openDestination,
		Upload: h.upload, Download: h.download, ReleaseStream: h.releaseStream,
	}
}

func (h *windowViewHost) close() {
	h.mu.Lock()
	if h.closed {
		h.mu.Unlock()
		return
	}
	h.closed = true
	h.changeSubs = nil
	clientUnsubs := h.clientUnsubs
	h.clientUnsubs = nil
	netUnsub := h.netUnsub
	h.netUnsub = nil
	h.observedNet = nil
	watches := make([]*usageWatchEntry, 0, len(h.watches))
	for entry := range h.watches {
		watches = append(watches, entry)
	}
	h.watches = nil
	h.mu.Unlock()

	if netUnsub != nil {
		netUnsub()
	}
	for _, unsub := range clientUnsubs {
		unsub()
	}
	for _, entry := range watches {
		entry.close()
	}
}

func (h *windowViewHost) syncState(initial bool) {
	root := h.runtime.localRuntime()
	locale := root.packageLocale()
	next := make(map[protocol.NodeID]syncedNodeEntry)

	readCreds := func(owner *Runtime, prev syncedNodeEntry) (map[protocol.CredentialID]protocol.Credential, string) {
		if owner == nil || owner.authorizations == nil || owner.authorizations.credentialKey == "" || owner.authorizations.credentials == nil {
			return nil, ""
		}
		key := owner.authorizations.credentialKey
		if prev.credentials != nil && prev.credKey == key && len(prev.credentials) == len(owner.authorizations.credentials) {
			return prev.credentials, key
		}
		return maps.Clone(owner.authorizations.credentials), key
	}

	h.mu.Lock()
	prevNodes := h.syncedNodes
	h.mu.Unlock()

	for _, entry := range root.canonicalHosts() {
		if entry.ID == (protocol.NodeID{}) {
			continue
		}
		prev := prevNodes[entry.ID]
		creds, credKey := readCreds(entry.Owner, prev)
		label := entry.NameKey
		if entry.Local || label == localHostNameKey {
			label = i18n.In(locale, localHostNameKey)
		}
		var opts []kit.ModelOption
		var provRev, scoreRev uint64
		var presetCount int
		if entry.Client != nil {
			provRev = entry.Client.ProviderRevision()
			if entry.Owner != nil {
				scoreRev = entry.Owner.modelScores.revision
				presetCount = len(entry.Owner.providerPresets) + len(entry.Owner.providerBrands)
			}
			if prev.client == entry.Client && prev.providerRev == provRev && prev.scoreRev == scoreRev && prev.presetCount == presetCount && prev.modelOptions != nil {
				opts = prev.modelOptions
			} else if entry.Owner != nil {
				opts = entry.Owner.modelOptions()
			} else {
				opts = snapshotModelOptions(entry.Client.Snapshot())
			}
		}
		next[entry.ID] = syncedNodeEntry{
			id:             entry.ID,
			client:         entry.Client,
			label:          label,
			local:          entry.Local,
			projectSources: entry.ProjectSources,
			tools:          entry.Tools,
			snapTools:      entry.SnapTools,
			canClone:       entry.CanClone,
			snapRev:        entry.SnapRev,
			hasSnap:        entry.HasSnap,
			credentials:    creds,
			credKey:        credKey,
			modelOptions:   opts,
			providerRev:    provRev,
			scoreRev:       scoreRev,
			presetCount:    presetCount,
		}
	}
	remembered := root.rememberedHostNames()
	network := root.network

	demoOn := root.App != nil && root.App.DemoOn()
	var demoUsage *preview.Usage
	if root.App != nil {
		demoUsage = root.App.DemoUsage()
	}

	var staleUnsubs []func()
	changed := false

	h.mu.Lock()
	if !h.closed {
		if h.locale != locale {
			h.locale = locale
			changed = true
		}
		if h.demoOn != demoOn || h.demoUsage != demoUsage {
			h.demoOn = demoOn
			h.demoUsage = demoUsage
			h.demoRevision++
			changed = true
		}
		if len(h.syncedNodes) != len(next) {
			changed = true
		} else {
			for id, entry := range next {
				prev, ok := h.syncedNodes[id]
				if !ok || prev.client != entry.client || prev.label != entry.label || prev.credKey != entry.credKey ||
					prev.canClone != entry.canClone || !slices.Equal(prev.projectSources, entry.projectSources) || !hostToolsEqual(prev.tools, entry.tools) {
					changed = true
					break
				}
			}
		}
		if !maps.Equal(h.rememberedNames, remembered) {
			h.rememberedNames = remembered
			changed = true
		}
		h.syncedNodes = next
		h.network = network
		h.synced = true
		if len(h.changeSubs) > 0 {
			staleUnsubs = h.syncSubscriptionsLocked()
		}
	}
	h.mu.Unlock()

	for _, unsub := range staleUnsubs {
		unsub()
	}
	if changed && !initial {
		h.notifyChange()
	}
}

func (h *windowViewHost) syncSubscriptionsLocked() []func() {
	var stale []func()
	if h.closed || len(h.changeSubs) == 0 {
		if h.netUnsub != nil {
			stale = append(stale, h.netUnsub)
			h.netUnsub = nil
			h.observedNet = nil
		}
		for c, unsub := range h.clientUnsubs {
			stale = append(stale, unsub)
			delete(h.clientUnsubs, c)
		}
		return stale
	}
	if h.clientUnsubs == nil {
		h.clientUnsubs = make(map[*client.Client]func())
	}
	desired := map[*client.Client]struct{}{
		h.runtime.localRuntime().controller: {},
	}
	for _, entry := range h.syncedNodes {
		if entry.client != nil {
			desired[entry.client] = struct{}{}
		}
	}
	if h.network != nil {
		for _, peer := range h.network.snapshot() {
			if peer.controller != nil {
				desired[peer.controller] = struct{}{}
			}
		}
	}
	for c, unsub := range h.clientUnsubs {
		if _, ok := desired[c]; !ok {
			stale = append(stale, unsub)
			delete(h.clientUnsubs, c)
		}
	}
	for c := range desired {
		if _, ok := h.clientUnsubs[c]; !ok {
			h.clientUnsubs[c] = c.OnChange(h.notifyChange)
		}
	}
	if h.observedNet != h.network {
		if h.netUnsub != nil {
			stale = append(stale, h.netUnsub)
			h.netUnsub = nil
		}
		h.observedNet = h.network
		if h.network != nil {
			h.netUnsub = h.network.onChange(h.notifyChange)
		}
	}
	return stale
}

func (h *windowViewHost) notifyChange() {
	h.mu.Lock()
	if h.closed || len(h.changeSubs) == 0 {
		h.mu.Unlock()
		return
	}
	callbacks := make([]func(), 0, len(h.changeSubs))
	for _, fn := range h.changeSubs {
		callbacks = append(callbacks, fn)
	}
	h.mu.Unlock()
	for _, fn := range callbacks {
		fn()
	}
}

func (h *windowViewHost) onChange(callback func()) func() {
	if callback == nil || !h.open() {
		return func() {}
	}
	h.mu.Lock()
	if h.closed {
		h.mu.Unlock()
		return func() {}
	}
	if h.changeSubs == nil {
		h.changeSubs = make(map[uint64]func())
	}
	h.nextSubID++
	id := h.nextSubID
	h.changeSubs[id] = callback
	stale := h.syncSubscriptionsLocked()
	h.mu.Unlock()
	for _, unsub := range stale {
		unsub()
	}
	var once sync.Once
	return func() {
		once.Do(func() {
			h.mu.Lock()
			delete(h.changeSubs, id)
			var release []func()
			if len(h.changeSubs) == 0 {
				release = h.syncSubscriptionsLocked()
			}
			h.mu.Unlock()
			for _, unsub := range release {
				unsub()
			}
		})
	}
}

func (h *windowViewHost) activeLocale() string {
	h.mu.Lock()
	locale := h.locale
	h.mu.Unlock()
	if locale == "" {
		return "en"
	}
	return locale
}

func (h *windowViewHost) nodeLabel(node protocol.NodeID) string {
	root := h.runtime.localRuntime()
	localID := root.controller.Target()
	h.mu.Lock()
	locale := h.locale
	entry, hasSynced := h.syncedNodes[node]
	remembered := h.rememberedNames[node]
	network := h.network
	h.mu.Unlock()
	if locale == "" {
		locale = "en"
	}
	if node == (protocol.NodeID{}) || node == localID {
		return i18n.In(locale, localHostNameKey)
	}
	if hasSynced && entry.label != "" {
		return entry.label
	}
	if network != nil {
		for _, peer := range network.snapshot() {
			if peer.address.ID == node {
				return peerDisplayName(peer)
			}
		}
	}
	if remembered != "" {
		return remembered
	}
	return shortNodeID(node.String())
}

func (h *windowViewHost) currentEntries() []syncedNodeEntry {
	root := h.runtime.localRuntime()
	localID := root.controller.Target()
	byID := make(map[protocol.NodeID]syncedNodeEntry)

	h.mu.Lock()
	locale := h.locale
	for id, entry := range h.syncedNodes {
		byID[id] = entry
	}
	remembered := maps.Clone(h.rememberedNames)
	network := h.network
	h.mu.Unlock()
	if locale == "" {
		locale = "en"
	}

	if _, ok := byID[localID]; !ok {
		byID[localID] = syncedNodeEntry{
			id:     localID,
			client: root.controller,
			label:  i18n.In(locale, localHostNameKey),
			local:  true,
		}
	}
	if network != nil {
		for _, peer := range network.snapshot() {
			if peer.controller == nil || peer.address.ID == localID {
				continue
			}
			entry := byID[peer.address.ID]
			entry.id = peer.address.ID
			entry.client = peer.controller
			if entry.label == "" || entry.label == shortNodeID(peer.address.ID.String()) {
				label := peerDisplayName(peer)
				if label == shortNodeID(peer.address.ID.String()) && remembered[peer.address.ID] != "" {
					label = remembered[peer.address.ID]
				}
				entry.label = label
			}
			entry.local = false
			byID[peer.address.ID] = entry
		}
	}

	entries := make([]syncedNodeEntry, 0, len(byID))
	for _, entry := range byID {
		entries = append(entries, entry)
	}
	slices.SortFunc(entries, func(left, right syncedNodeEntry) int {
		if left.local != right.local {
			if left.local {
				return -1
			}
			return 1
		}
		return bytes.Compare(left.id[:], right.id[:])
	})
	return entries
}

func (h *windowViewHost) entryFor(node protocol.NodeID) (syncedNodeEntry, bool) {
	root := h.runtime.localRuntime()
	localID := root.controller.Target()
	if node == (protocol.NodeID{}) || node == localID {
		h.mu.Lock()
		entry, ok := h.syncedNodes[localID]
		locale := h.locale
		h.mu.Unlock()
		if ok {
			return entry, true
		}
		if locale == "" {
			locale = "en"
		}
		return syncedNodeEntry{id: localID, client: root.controller, label: i18n.In(locale, localHostNameKey), local: true}, true
	}
	h.mu.Lock()
	entry, ok := h.syncedNodes[node]
	remembered := h.rememberedNames[node]
	network := h.network
	h.mu.Unlock()
	if ok && entry.client != nil {
		return entry, true
	}
	if network != nil {
		for _, peer := range network.snapshot() {
			if peer.address.ID == node && peer.controller != nil {
				label := peerDisplayName(peer)
				if label == shortNodeID(node.String()) && remembered != "" {
					label = remembered
				}
				return syncedNodeEntry{id: node, client: peer.controller, label: label}, true
			}
		}
	}
	return syncedNodeEntry{}, false
}

func (h *windowViewHost) nodes() []view.NodeInfo {
	if !h.open() {
		return nil
	}
	entries := h.currentEntries()
	out := make([]view.NodeInfo, 0, len(entries))
	for _, entry := range entries {
		sources := slices.Clone(entry.projectSources)
		tools := cloneHostTools(entry.tools)
		canClone := entry.canClone
		if entry.client != nil && (!entry.hasSnap || entry.snapRev != entry.client.Revision()) {
			var snapTools []protocol.HostTool
			sources, snapTools, _ = entry.client.HostCapabilities()
			if !entry.hasSnap || !hostToolsEqual(snapTools, entry.snapTools) {
				tools = snapTools
			}
			canClone = hostCanClone(sources, tools)
		}
		out = append(out, view.NodeInfo{
			ID:             entry.id,
			Label:          entry.label,
			Connected:      entry.client != nil && entry.client.Connected(),
			Local:          entry.local,
			ProjectSources: sources,
			Tools:          tools,
			CanClone:       canClone,
		})
	}
	return out
}

func (h *windowViewHost) modelOptions(node protocol.NodeID) []kit.ModelOption {
	if !h.open() {
		return nil
	}
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return nil
	}
	if entry.modelOptions != nil && entry.providerRev == entry.client.ProviderRevision() {
		return cloneModelOptions(entry.modelOptions)
	}
	return snapshotModelOptions(entry.client.Snapshot())
}

func cloneModelOptions(options []kit.ModelOption) []kit.ModelOption {
	out := make([]kit.ModelOption, len(options))
	for i, opt := range options {
		opt.Efforts = slices.Clone(opt.Efforts)
		out[i] = opt
	}
	return out
}

func (h *windowViewHost) prepareOn(node protocol.NodeID, command protocol.Command) (protocol.Request, error) {
	if !h.open() {
		return protocol.Request{}, context.Canceled
	}
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return protocol.Request{}, fileActionFault(protocol.ErrorCodeUnavailable, "execution Node is unavailable")
	}
	return entry.client.Prepare(command), nil
}

func (h *windowViewHost) executeOn(ctx context.Context, node protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return protocol.Output{}, fileActionFault(protocol.ErrorCodeUnavailable, "execution Node is unavailable")
	}
	return windowRequestOn(h, entry.client, ctx, request, entry.client.Execute)
}

func (h *windowViewHost) settleOn(ctx context.Context, node protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return protocol.Output{}, fileActionFault(protocol.ErrorCodeUnavailable, "execution Node is unavailable")
	}
	return windowRequestOn(h, entry.client, ctx, request, entry.client.Settle)
}

func (h *windowViewHost) resolveOn(ctx context.Context, node protocol.NodeID, request protocol.Request) (view.Receipt, error) {
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return view.Receipt{}, fileActionFault(protocol.ErrorCodeUnavailable, "execution Node is unavailable")
	}
	result, err := windowRequestOn(h, entry.client, ctx, request, entry.client.Resolve)
	if err != nil {
		return view.Receipt{}, err
	}
	return receiptFromResolution(result)
}

func (h *windowViewHost) outcomeOn(ctx context.Context, node protocol.NodeID, request protocol.Request) (protocol.RequestOutcome, error) {
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return protocol.RequestOutcome{}, fileActionFault(protocol.ErrorCodeUnavailable, "execution Node is unavailable")
	}
	return windowRequestOn(h, entry.client, ctx, request, entry.client.Outcome)
}

func (h *windowViewHost) snapshot(node protocol.NodeID) (*protocol.Snapshot, bool) {
	if !h.open() {
		return nil, false
	}
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return nil, false
	}
	snap := entry.client.Snapshot()
	return &snap, true
}

func (h *windowViewHost) providerAccount(node protocol.NodeID, provider protocol.ProviderID) (view.ProviderAccount, bool) {
	if !h.open() {
		return view.ProviderAccount{}, false
	}
	entry, ok := h.entryFor(node)
	if !ok || entry.client == nil {
		return view.ProviderAccount{}, false
	}
	auth, rev, id, ok := entry.client.ProviderCredential(provider)
	if !ok {
		return view.ProviderAccount{}, false
	}
	account := view.ProviderAccount{Authentication: auth, ProviderRevision: rev, Credential: id}
	if entry.credentials != nil {
		credential, exists := entry.credentials[id]
		if !exists || credential.Revoked || credential.Provider != provider {
			return view.ProviderAccount{}, false
		}
		account.CredentialKnown = true
		account.CredentialRevision = credential.Revision
	}
	return account, true
}

func (h *windowViewHost) credentials(node protocol.NodeID) map[protocol.CredentialID]protocol.Credential {
	if !h.open() {
		return nil
	}
	entry, ok := h.entryFor(node)
	if !ok || entry.credentials == nil {
		return nil
	}
	return maps.Clone(entry.credentials)
}

func (h *windowViewHost) openSession(node protocol.NodeID, id protocol.SessionID) {
	if !h.open() || id == (protocol.SessionID{}) {
		return
	}
	h.post(func(*ui.Context) {
		root := h.runtime.localRuntime()
		owner := root
		if node != (protocol.NodeID{}) {
			owner = root.runtimeFor(node.String())
			if owner == nil {
				return
			}
		}
		if _, exists := owner.session(id.String()); !exists {
			return
		}
		owner.project()
		if cv := root.App.Data.Find(id.String()); cv != nil && cv.Node == owner.controller.Target().String() {
			root.OpenSession(id.String())
		}
	})
}

func (h *windowViewHost) revealDetails() {
	if !h.open() {
		return
	}
	h.post(func(*ui.Context) {
		root := h.runtime.localRuntime()
		if root.App != nil {
			root.App.RevealDetails()
		}
	})
}

func (h *windowViewHost) usageSample() (*view.UsageSample, bool) {
	if !h.open() {
		return nil, false
	}
	h.mu.Lock()
	ledger, rev := h.demoUsage, h.demoRevision
	h.mu.Unlock()
	if ledger == nil {
		return nil, false
	}
	return &view.UsageSample{
		Revision:  rev,
		Report:    ledger.Report,
		Inventory: ledger.Inventory,
	}, true
}

func (h *windowViewHost) usageServices() *view.UsageServices {
	return &view.UsageServices{
		Sources:       h.usageSources,
		WatchOverview: h.watchUsageOverview,
		ReadUsage:     h.readUsage,
		Inventory:     h.usageInventory,
		Sample:        h.usageSample,
	}
}

func (h *windowViewHost) usageSources() []view.UsageSource {
	if !h.open() {
		return nil
	}
	localID := h.runtime.localRuntime().controller.Target()
	if _, demo := h.usageSample(); demo {
		return []view.UsageSource{{ID: localID, Label: h.nodeLabel(localID), Local: true}}
	}
	entries := h.currentEntries()
	sources := make([]view.UsageSource, 0, len(entries))
	for _, entry := range entries {
		if entry.client == nil {
			continue
		}
		sources = append(sources, view.UsageSource{
			ID:    entry.id,
			Label: entry.label,
			Local: entry.local,
		})
	}
	return sources
}

func (h *windowViewHost) usageInventory(target protocol.NodeID) (view.UsageInventory, bool) {
	if !h.open() {
		return view.UsageInventory{}, false
	}
	localID := h.runtime.localRuntime().controller.Target()
	if sample, demo := h.usageSample(); demo {
		if target != (protocol.NodeID{}) && target != localID {
			return view.UsageInventory{}, false
		}
		projects, providers := sample.Inventory()
		return view.UsageInventory{
			Node:      localID,
			Label:     h.nodeLabel(localID),
			Projects:  projects,
			Providers: providers,
		}, true
	}
	entry, ok := h.entryFor(target)
	if !ok || entry.client == nil {
		return view.UsageInventory{}, false
	}
	snap := entry.client.Snapshot()
	return view.UsageInventory{
		Node:      snap.Node,
		Label:     entry.label,
		Projects:  snap.Projects,
		Providers: snap.Providers,
	}, true
}

func (h *windowViewHost) readUsage(ctx context.Context, target protocol.NodeID, query protocol.UsageQuery) (protocol.UsageReport, error) {
	if !h.open() {
		return protocol.UsageReport{}, context.Canceled
	}
	localID := h.runtime.localRuntime().controller.Target()
	if sample, demo := h.usageSample(); demo {
		if target != (protocol.NodeID{}) && target != localID {
			return protocol.UsageReport{}, fileActionFault(protocol.ErrorCodeUnavailable, "usage source is unavailable")
		}
		return sample.Report(localID, query)
	}
	entry, ok := h.entryFor(target)
	if !ok || entry.client == nil {
		return protocol.UsageReport{}, fileActionFault(protocol.ErrorCodeUnavailable, "usage source is unavailable")
	}
	opCtx, done := h.operationContext(ctx)
	defer done()
	if err := opCtx.Err(); err != nil {
		return protocol.UsageReport{}, err
	}
	return entry.client.ReadUsage(opCtx, query)
}

func (h *windowViewHost) watchUsageOverview(ctx context.Context, target protocol.NodeID, query protocol.UsageQuery, onUpdate func(view.UsageOverview, error)) (view.UsageWatch, error) {
	if !h.open() {
		return view.UsageWatch{}, context.Canceled
	}
	if onUpdate == nil {
		return view.UsageWatch{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "usage overview callback is required")
	}
	if ctx == nil {
		ctx = context.Background()
	}
	localID := h.runtime.localRuntime().controller.Target()
	if sample, demo := h.usageSample(); demo {
		if target != (protocol.NodeID{}) && target != localID {
			return view.UsageWatch{}, fileActionFault(protocol.ErrorCodeUnavailable, "usage source is unavailable")
		}
		watchCtx, cancel := h.operationContext(ctx)
		if err := watchCtx.Err(); err != nil {
			cancel()
			return view.UsageWatch{}, err
		}
		entry := &usageWatchEntry{}
		var once sync.Once
		closeFn := func() {
			once.Do(func() {
				h.untrackWatch(entry)
				cancel()
			})
		}
		entry.close = closeFn
		if !h.trackWatch(entry) {
			cancel()
			return view.UsageWatch{}, context.Canceled
		}
		emit := func() {
			if watchCtx.Err() != nil {
				return
			}
			overview := demoUsageOverview(localID, sample, query)
			if watchCtx.Err() != nil {
				return
			}
			onUpdate(overview, nil)
		}
		if !h.runtime.observe(emit) {
			closeFn()
			return view.UsageWatch{}, context.Canceled
		}
		return view.UsageWatch{
			Refresh: func(rctx context.Context) error {
				if rctx != nil && rctx.Err() != nil {
					return rctx.Err()
				}
				if watchCtx.Err() != nil {
					return watchCtx.Err()
				}
				emit()
				return nil
			},
			Close: closeFn,
		}, nil
	}

	var clients []*client.Client
	if target == (protocol.NodeID{}) {
		if len(query.Projects) != 0 || len(query.Worktrees) != 0 || len(query.Providers) != 0 {
			return view.UsageWatch{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "cross-Node usage cannot filter Node-local resource identities")
		}
		for _, entry := range h.currentEntries() {
			if entry.client != nil {
				clients = append(clients, entry.client)
			}
		}
	} else {
		entry, ok := h.entryFor(target)
		if !ok || entry.client == nil {
			return view.UsageWatch{}, fileActionFault(protocol.ErrorCodeUnavailable, "usage source is unavailable")
		}
		clients = []*client.Client{entry.client}
	}
	body, err := json.Marshal(query)
	if err != nil {
		return view.UsageWatch{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid usage query")
	}
	watchCtx, cancel := h.operationContext(ctx)
	if err := watchCtx.Err(); err != nil {
		cancel()
		return view.UsageWatch{}, err
	}
	observer, err := client.WatchOverview(watchCtx, clients, body)
	if err != nil {
		cancel()
		return view.UsageWatch{}, err
	}
	entry := &usageWatchEntry{}
	var once sync.Once
	closeFn := func() {
		once.Do(func() {
			h.untrackWatch(entry)
			cancel()
			_ = observer.Close()
		})
	}
	entry.close = closeFn
	if !h.trackWatch(entry) {
		closeFn()
		return view.UsageWatch{}, context.Canceled
	}
	started := h.runtime.observe(func() {
		defer closeFn()
		for {
			data, err := observer.Next(watchCtx)
			if err != nil {
				if watchCtx.Err() == nil {
					onUpdate(view.UsageOverview{}, err)
				}
				return
			}
			overview, err := decodeUsageOverview(data)
			if watchCtx.Err() != nil {
				return
			}
			onUpdate(overview, err)
		}
	})
	if !started {
		closeFn()
		return view.UsageWatch{}, context.Canceled
	}
	return view.UsageWatch{
		Refresh: func(rctx context.Context) error {
			if rctx == nil {
				rctx = watchCtx
			}
			opCtx, done := h.operationContext(rctx)
			defer done()
			if err := opCtx.Err(); err != nil {
				return err
			}
			if watchCtx.Err() != nil {
				return watchCtx.Err()
			}
			return observer.Control(opCtx, "refresh", json.RawMessage("null"))
		},
		Close: closeFn,
	}, nil
}

func (h *windowViewHost) trackWatch(entry *usageWatchEntry) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.closed {
		return false
	}
	if h.watches == nil {
		h.watches = make(map[*usageWatchEntry]struct{})
	}
	h.watches[entry] = struct{}{}
	return true
}

func (h *windowViewHost) untrackWatch(entry *usageWatchEntry) {
	h.mu.Lock()
	delete(h.watches, entry)
	h.mu.Unlock()
}

type wireUsageNodeView struct {
	Report     *protocol.UsageReport `json:"report"`
	Connected  bool                  `json:"connected"`
	Refreshing bool                  `json:"refreshing"`
	Error      *protocol.Fault       `json:"error"`
}

type wireUsageNodeSource struct {
	Node  protocol.NodeID   `json:"node"`
	Entry protocol.NodeID   `json:"entry"`
	View  wireUsageNodeView `json:"view"`
}

type wireUsageScopedGroup struct {
	Node  protocol.NodeID     `json:"node"`
	Entry protocol.NodeID     `json:"entry"`
	Group protocol.UsageGroup `json:"group"`
}

type wireUsageRequestData protocol.UsageRequest

type wireUsageScopedRequest struct {
	wireUsageRequestData
	Entry protocol.NodeID `json:"entry"`
}

type wireUsageScopedPage struct {
	Items   []wireUsageScopedRequest `json:"items"`
	HasMore bool                     `json:"has_more"`
}

type wireUsageSummary struct {
	Totals   protocol.UsageMetrics  `json:"totals"`
	Days     []protocol.UsagePoint  `json:"days"`
	Recent   []protocol.UsagePoint  `json:"recent"`
	Groups   []wireUsageScopedGroup `json:"groups"`
	Requests wireUsageScopedPage    `json:"requests"`
}

type wireUsageOverview struct {
	Sources  []wireUsageNodeSource `json:"sources"`
	Summary  *wireUsageSummary     `json:"summary"`
	Complete bool                  `json:"complete"`
	Error    *protocol.Fault       `json:"error"`
}

func decodeUsageOverview(data json.RawMessage) (view.UsageOverview, error) {
	var wire wireUsageOverview
	if err := protocol.DecodeAs(protocol.PeerData, data, &wire); err != nil {
		return view.UsageOverview{}, err
	}
	out := view.UsageOverview{
		Sources:  make([]view.UsageNodeSource, len(wire.Sources)),
		Complete: wire.Complete,
		Error:    wire.Error,
	}
	for i, src := range wire.Sources {
		out.Sources[i] = view.UsageNodeSource{
			Node:  src.Node,
			Entry: src.Entry,
			View: view.UsageNodeView{
				Report:     src.View.Report,
				Connected:  src.View.Connected,
				Refreshing: src.View.Refreshing,
				Error:      src.View.Error,
			},
		}
	}
	if wire.Summary != nil {
		groups := make([]view.UsageScopedGroup, len(wire.Summary.Groups))
		for i, group := range wire.Summary.Groups {
			groups[i] = view.UsageScopedGroup{Node: group.Node, Entry: group.Entry, Group: group.Group}
		}
		items := make([]view.UsageScopedRequest, len(wire.Summary.Requests.Items))
		for i, req := range wire.Summary.Requests.Items {
			items[i] = view.UsageScopedRequest{Request: protocol.UsageRequest(req.wireUsageRequestData), Entry: req.Entry}
		}
		out.Summary = &view.UsageSummary{
			Totals:   wire.Summary.Totals,
			Days:     wire.Summary.Days,
			Recent:   wire.Summary.Recent,
			Groups:   groups,
			Requests: view.UsageScopedPage{Items: items, HasMore: wire.Summary.Requests.HasMore},
		}
	}
	return out, nil
}

func demoUsageOverview(node protocol.NodeID, sample *view.UsageSample, query protocol.UsageQuery) view.UsageOverview {
	report, err := sample.Report(node, query)
	if err != nil {
		fault := usageErrorFault(err)
		return view.UsageOverview{
			Sources: []view.UsageNodeSource{{
				Node:  node,
				Entry: node,
				View:  view.UsageNodeView{Connected: true, Error: fault},
			}},
			Complete: false,
			Error:    fault,
		}
	}
	groups := make([]view.UsageScopedGroup, len(report.Groups))
	for i, group := range report.Groups {
		groups[i] = view.UsageScopedGroup{Node: node, Entry: node, Group: group}
	}
	items := make([]view.UsageScopedRequest, len(report.Requests.Items))
	for i, req := range report.Requests.Items {
		items[i] = view.UsageScopedRequest{Request: req, Entry: node}
	}
	return view.UsageOverview{
		Sources: []view.UsageNodeSource{{
			Node:  node,
			Entry: node,
			View:  view.UsageNodeView{Report: &report, Connected: true},
		}},
		Summary: &view.UsageSummary{
			Totals:   report.Totals,
			Days:     report.Days,
			Recent:   report.Recent,
			Groups:   groups,
			Requests: view.UsageScopedPage{Items: items, HasMore: report.Requests.HasMore},
		},
		Complete: true,
	}
}

func usageErrorFault(err error) *protocol.Fault {
	if err == nil {
		return nil
	}
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		copied := *fault
		return &copied
	}
	var linkErr *link.FaultError
	if errors.As(err, &linkErr) {
		copied := linkErr.Fault
		return &copied
	}
	return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: err.Error()}
}

func (h *windowViewHost) open() bool {
	r := h.runtime
	r.mu.Lock()
	defer r.mu.Unlock()
	return !r.closed && r.ctx.Err() == nil
}

func (h *windowViewHost) post(apply func(*ui.Context)) bool {
	r := h.runtime
	r.mu.Lock()
	if apply == nil || r.closed || r.ctx.Err() != nil {
		r.mu.Unlock()
		return false
	}
	r.queue = append(r.queue, func(c *ui.Context) {
		if h.open() {
			apply(c)
		}
	})
	r.mu.Unlock()
	r.wake()
	return true
}

func (h *windowViewHost) next(ctx context.Context, seen string) (view.Change, error) {
	controller := h.runtime.controller
	changed := make(chan struct{}, 1)
	unsubscribe := controller.OnChange(func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	defer unsubscribe()
	for {
		if err := ctx.Err(); err != nil {
			return view.Change{}, err
		}
		if !h.open() {
			return view.Change{}, context.Canceled
		}
		connected := controller.Connected()
		cursor := strconv.FormatUint(controller.Revision(), 10) + ":" + strconv.FormatBool(connected)
		if cursor != seen {
			return view.Change{Cursor: cursor, Connected: connected}, nil
		}
		select {
		case <-ctx.Done():
			return view.Change{}, ctx.Err()
		case <-h.runtime.ctx.Done():
			return view.Change{}, h.runtime.ctx.Err()
		case <-changed:
		}
	}
}

func (h *windowViewHost) operationContext(ctx context.Context) (context.Context, func()) {
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(h.runtime.ctx, cancel)
	if !h.open() {
		cancel()
	}
	return ctx, func() { stop(); cancel() }
}

func windowRequest[Value any](h *windowViewHost, ctx context.Context, request protocol.Request, run func(context.Context, protocol.Request) (Value, error)) (Value, error) {
	return windowRequestOn(h, h.runtime.controller, ctx, request, run)
}

func windowRequestOn[Value any](h *windowViewHost, controller *client.Client, ctx context.Context, request protocol.Request, run func(context.Context, protocol.Request) (Value, error)) (Value, error) {
	var zero Value
	// A compiled contribution uses the window's client, not a package scope
	// supplied by a template. A request must stay on the captured Node.
	if controller == nil {
		return zero, fileActionFault(protocol.ErrorCodeUnavailable, "execution Node is unavailable")
	}
	if request.Target != controller.Target() || request.Version != protocol.VERSION {
		return zero, fileActionFault(protocol.ErrorCodeWrongTarget, "request belongs to another Node or protocol version")
	}
	if request.Plugin != nil || request.ID == (protocol.RequestID{}) {
		return zero, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid native window request")
	}
	ctx, done := h.operationContext(ctx)
	defer done()
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	return run(ctx, request)
}

func receiptFromResolution(result client.Resolution) (view.Receipt, error) {
	var state string
	switch result.State {
	case client.RequestPending:
		state = "pending"
	case client.RequestApplied:
		state = "applied"
	case client.RequestFailed:
		state = "failed"
	case client.RequestAbsent:
		state = "absent"
	case client.RequestUnknown:
		state = "unknown"
	case client.RequestReplayable:
		state = "replayable"
	default:
		return view.Receipt{}, fileActionFault(protocol.ErrorCodeInternal, "invalid native window receipt")
	}
	return view.Receipt{State: state, Output: result.Output, Fault: result.Fault}, nil
}

func (h *windowViewHost) resolve(ctx context.Context, request protocol.Request) (view.Receipt, error) {
	result, err := windowRequest(h, ctx, request, h.runtime.controller.Resolve)
	if err != nil {
		return view.Receipt{}, err
	}
	return receiptFromResolution(result)
}

func (h *windowViewHost) trackMount(resource nativeWindowResource) func() {
	if h.mounts == nil {
		h.mounts = make(map[*nativeWindowResource]struct{})
		h.runtime.nativeWindowResources = append(h.runtime.nativeWindowResources, nativeWindowResource{
			afterFrame: func() {
				for mount := range h.mounts {
					if mount.afterFrame != nil {
						mount.afterFrame()
					}
				}
			},
			close: func() {
				for mount := range h.mounts {
					mount.close()
				}
				clear(h.mounts)
			},
		})
	}
	h.mounts[&resource] = struct{}{}
	return func() {
		if _, exists := h.mounts[&resource]; exists {
			delete(h.mounts, &resource)
			resource.close()
		}
	}
}
