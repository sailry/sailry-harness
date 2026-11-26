package live

import (
	"encoding/hex"
	"runtime"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// localHostNameKey is the canonical i18n key naming the local machine across
// every Desktop surface (sidebar, composer, settings, usage, tasks, reminders).
const localHostNameKey = "local_host_name"

// canonicalHost is one execution host known to this window from the single
// canonical host directory.
type canonicalHost struct {
	ID             protocol.NodeID
	IDString       string
	NameKey        string // localHostNameKey for local; peer/catalog/remembered name or shortNodeID for remote
	Platform       string
	Connected      bool
	Local          bool
	ProjectSources []string
	Tools          []protocol.HostTool
	CanClone       bool
	SnapTools      []protocol.HostTool
	SnapRev        uint64
	HasSnap        bool
	Client         *client.Client
	Owner          *Runtime
}

type hostCapabilities struct {
	projectSources []string
	tools          []protocol.HostTool
	snapTools      []protocol.HostTool
	canClone       bool
	snapRev        uint64
	hasSnap        bool
}

func hostToolAvailable(tools []protocol.HostTool, name string) bool {
	for _, tool := range tools {
		if tool.Name == name && tool.Available {
			return true
		}
	}
	return false
}

func hostCanClone(projectSources []string, tools []protocol.HostTool) bool {
	return slices.Contains(projectSources, "clone") && hostToolAvailable(tools, "git")
}

func cloneHostTools(tools []protocol.HostTool) []protocol.HostTool {
	if len(tools) == 0 {
		return nil
	}
	out := make([]protocol.HostTool, len(tools))
	for i, tool := range tools {
		cloned := protocol.HostTool{
			Name:      tool.Name,
			Available: tool.Available,
		}
		if tool.Path != nil {
			path := *tool.Path
			cloned.Path = &path
		}
		if tool.Version != nil {
			version := *tool.Version
			cloned.Version = &version
		}
		out[i] = cloned
	}
	return out
}

func hostToolsEqual(left, right []protocol.HostTool) bool {
	if len(left) != len(right) {
		return false
	}
	for i := range left {
		if left[i].Name != right[i].Name || left[i].Available != right[i].Available {
			return false
		}
		if (left[i].Path == nil) != (right[i].Path == nil) || (left[i].Path != nil && *left[i].Path != *right[i].Path) {
			return false
		}
		if (left[i].Version == nil) != (right[i].Version == nil) || (left[i].Version != nil && *left[i].Version != *right[i].Version) {
			return false
		}
	}
	return true
}

// effectiveHostCapabilities resolves a host's project sources, CLI tools and
// clone capability from its Client snapshot, overlaying fresher inspect_host
// tool probes while the snapshot's own HostTools have not superseded them.
func effectiveHostCapabilities(state *liveHost, c *client.Client) hostCapabilities {
	var caps hostCapabilities
	if c != nil {
		caps.snapRev = c.Revision()
		snap := c.Snapshot()
		caps.hasSnap = true
		caps.projectSources = slices.Clone(snap.ProjectSources)
		caps.snapTools = cloneHostTools(snap.HostTools)
		caps.tools = cloneHostTools(snap.HostTools)
	}
	if state != nil && state.info != nil && len(state.info.Tools) > 0 && hostToolsEqual(caps.snapTools, state.infoSnapTools) {
		caps.tools = cloneHostTools(state.info.Tools)
	}
	caps.canClone = hostCanClone(caps.projectSources, caps.tools)
	return caps
}

// shortNodeID formats an unnamed or offline Node ID consistently without
// exposing a raw 64-character hex string in UI labels.
func shortNodeID(id string) string {
	trimmed := strings.TrimSpace(id)
	if len(trimmed) > 8 {
		return trimmed[:8]
	}
	return trimmed
}

func parseNodeIDString(value string) (protocol.NodeID, bool) {
	raw, err := hex.DecodeString(strings.TrimSpace(value))
	if err != nil || len(raw) != len(protocol.NodeID{}) {
		return protocol.NodeID{}, false
	}
	var id protocol.NodeID
	copy(id[:], raw)
	return id, true
}

func usableRemoteHostName(candidate, idStr string) (string, bool) {
	name := strings.TrimSpace(candidate)
	if name == "" || name == idStr || name == localHostNameKey {
		return "", false
	}
	return name, true
}

func peerDisplayName(peer networkPeer) string {
	idStr := peer.address.ID.String()
	if peer.info.Name != nil {
		if name, ok := usableRemoteHostName(*peer.info.Name, idStr); ok {
			return name
		}
	}
	if name, ok := usableRemoteHostName(peer.rememberedName, idStr); ok {
		return name
	}
	return shortNodeID(idStr)
}

// canonicalHostName resolves the canonical name for node: localHostNameKey for
// the local device, otherwise the best known peer/catalog/remembered name, and
// shortNodeID when no name has ever been observed.
func (r *Runtime) canonicalHostName(id protocol.NodeID, peer *networkPeer) string {
	root := r.localRuntime()
	localID := root.controller.Target()
	if id == (protocol.NodeID{}) || id == localID {
		return localHostNameKey
	}
	idStr := id.String()
	if state := root.hosts[idStr]; state != nil && state.info != nil && state.info.Name != nil {
		if name, ok := usableRemoteHostName(*state.info.Name, idStr); ok {
			return name
		}
	}
	if peer != nil && peer.info.Name != nil {
		if name, ok := usableRemoteHostName(*peer.info.Name, idStr); ok {
			return name
		}
	}
	if peer == nil && root.network != nil {
		for _, candidate := range root.network.snapshot() {
			if candidate.address.ID == id {
				copied := candidate
				peer = &copied
				break
			}
		}
		if peer != nil && peer.info.Name != nil {
			if name, ok := usableRemoteHostName(*peer.info.Name, idStr); ok {
				return name
			}
		}
	}
	if root.resourceCatalog != nil {
		for _, source := range root.resourceCatalog.Sources {
			if source.Node == id {
				if name, ok := usableRemoteHostName(source.Name, idStr); ok {
					return name
				}
			}
		}
	}
	if peer != nil {
		if name, ok := usableRemoteHostName(peer.rememberedName, idStr); ok {
			return name
		}
	}
	if root.Preferences != nil && root.Preferences.Data.Devices != nil {
		if remembered, exists := (*root.Preferences.Data.Devices)[idStr]; exists && remembered.Name != nil {
			if name, ok := usableRemoteHostName(*remembered.Name, idStr); ok {
				return name
			}
		}
	}
	if state := root.hosts[idStr]; state != nil && state.view != nil {
		if name, ok := usableRemoteHostName(state.view.Name, idStr); ok {
			return name
		}
	}
	if root.App != nil && root.App.Data != nil {
		for _, host := range root.App.Data.Hosts {
			if host.ID == idStr {
				if name, ok := usableRemoteHostName(host.Name, idStr); ok {
					return name
				}
			}
		}
	}
	return shortNodeID(idStr)
}

// hostLabelInLocale resolves a Node's localized display name in locale.
func (r *Runtime) hostLabelInLocale(id protocol.NodeID, locale string, peer *networkPeer) string {
	name := r.canonicalHostName(id, peer)
	if name == localHostNameKey {
		return i18n.In(locale, localHostNameKey)
	}
	return name
}

// hostLabel resolves a Node's localized display name for the active UI context.
func (r *Runtime) hostLabel(c *ui.Context, id protocol.NodeID) string {
	name := r.canonicalHostName(id, nil)
	if name == localHostNameKey {
		return i18n.T(c, localHostNameKey)
	}
	return name
}

// hostLabelByID resolves a host's localized display name from a string host ID,
// handling both canonical hex NodeIDs and synthetic test host IDs.
func (r *Runtime) hostLabelByID(c *ui.Context, id string) string {
	root := r.localRuntime()
	if id == "" || id == root.controller.Target().String() {
		return i18n.T(c, localHostNameKey)
	}
	if parsed, ok := parseNodeIDString(id); ok {
		return root.hostLabel(c, parsed)
	}
	if state := root.hosts[id]; state != nil && state.view != nil {
		if state.view.Local || state.view.Name == localHostNameKey {
			return i18n.T(c, localHostNameKey)
		}
		if name, ok := usableRemoteHostName(state.view.Name, id); ok {
			return name
		}
	}
	if root.App != nil && root.App.Data != nil {
		for _, host := range root.App.Data.Hosts {
			if host.ID == id {
				if host.Local || host.Name == localHostNameKey {
					return i18n.T(c, localHostNameKey)
				}
				if name, ok := usableRemoteHostName(host.Name, id); ok {
					return name
				}
			}
		}
	}
	return shortNodeID(id)
}

// canonicalHosts builds the unified ordered list of execution hosts for this
// window (local first, followed by known remote hosts from App.Data.Hosts,
// live network peers, and bound runtimes). Call on the UI thread.
func (r *Runtime) canonicalHosts() []canonicalHost {
	root := r.localRuntime()
	localID := root.controller.Target()
	localIDStr := localID.String()

	peers := map[string]networkPeer{}
	var peerOrder []networkPeer
	if root.network != nil {
		peerOrder = root.network.snapshot()
		for _, peer := range peerOrder {
			peers[peer.address.ID.String()] = peer
		}
	}

	localPlatform := hostOS(runtime.GOOS)
	if root.App != nil && root.App.Data != nil {
		for _, host := range root.App.Data.Hosts {
			if (host.Local || host.ID == localIDStr) && host.Platform != "" {
				localPlatform = host.Platform
				break
			}
		}
	}

	localCaps := effectiveHostCapabilities(root.hosts[localIDStr], root.controller)
	hosts := []canonicalHost{{
		ID:             localID,
		IDString:       localIDStr,
		NameKey:        localHostNameKey,
		Platform:       localPlatform,
		Connected:      root.controller.Connected(),
		Local:          true,
		ProjectSources: localCaps.projectSources,
		Tools:          localCaps.tools,
		CanClone:       localCaps.canClone,
		SnapTools:      localCaps.snapTools,
		SnapRev:        localCaps.snapRev,
		HasSnap:        localCaps.hasSnap,
		Client:         root.controller,
		Owner:          root,
	}}
	seen := map[string]bool{localIDStr: true}

	appendRemote := func(id protocol.NodeID, idStr, platform string, peerPtr *networkPeer, c *client.Client, owner *Runtime, fallback *viewmodel.Host) {
		if idStr == "" || seen[idStr] {
			return
		}
		seen[idStr] = true
		if owner == nil {
			owner = root.runtimeFor(idStr)
		}
		if c == nil {
			if peerPtr != nil && peerPtr.controller != nil {
				c = peerPtr.controller
			} else if owner != nil {
				c = owner.controller
			}
		}
		if id == (protocol.NodeID{}) && c != nil {
			id = c.Target()
		}
		if platform == "" {
			if peerPtr != nil && peerPtr.info.Platform != "" {
				platform = hostOS(peerPtr.info.Platform)
			} else if state := root.hosts[idStr]; state != nil && state.view != nil {
				platform = state.view.OS
			}
		}
		var nameKey string
		if id != (protocol.NodeID{}) {
			nameKey = root.canonicalHostName(id, peerPtr)
		} else {
			if state := root.hosts[idStr]; state != nil && state.view != nil {
				if name, ok := usableRemoteHostName(state.view.Name, idStr); ok {
					nameKey = name
				}
			}
			if nameKey == "" && root.App != nil && root.App.Data != nil {
				for _, h := range root.App.Data.Hosts {
					if h.ID == idStr {
						if name, ok := usableRemoteHostName(h.Name, idStr); ok {
							nameKey = name
							break
						}
					}
				}
			}
			if nameKey == "" {
				nameKey = shortNodeID(idStr)
			}
		}
		caps := effectiveHostCapabilities(root.hosts[idStr], c)
		if c == nil && fallback != nil {
			caps.projectSources = slices.Clone(fallback.ProjectSources)
			caps.tools = cloneHostTools(fallback.Tools)
			caps.canClone = fallback.CanClone
		}
		hosts = append(hosts, canonicalHost{
			ID:             id,
			IDString:       idStr,
			NameKey:        nameKey,
			Platform:       platform,
			Connected:      c != nil && c.Connected(),
			Local:          false,
			ProjectSources: caps.projectSources,
			Tools:          caps.tools,
			CanClone:       caps.canClone,
			SnapTools:      caps.snapTools,
			SnapRev:        caps.snapRev,
			HasSnap:        caps.hasSnap,
			Client:         c,
			Owner:          owner,
		})
	}

	if root.App != nil && root.App.Data != nil {
		for _, host := range root.App.Data.Hosts {
			if host.Local || host.ID == localIDStr {
				continue
			}
			id, _ := parseNodeIDString(host.ID)
			var peerPtr *networkPeer
			if p, ok := peers[host.ID]; ok {
				copied := p
				peerPtr = &copied
			}
			hostCopy := host
			appendRemote(id, host.ID, host.Platform, peerPtr, nil, nil, &hostCopy)
		}
	}
	for _, peer := range peerOrder {
		if peer.controller == nil {
			continue
		}
		copied := peer
		appendRemote(peer.address.ID, peer.address.ID.String(), "", &copied, peer.controller, nil, nil)
	}
	for _, bound := range root.bindings {
		if bound == nil || bound.controller == nil {
			continue
		}
		id := bound.controller.Target()
		idStr := id.String()
		var peerPtr *networkPeer
		if p, ok := peers[idStr]; ok {
			copied := p
			peerPtr = &copied
		}
		appendRemote(id, idStr, "", peerPtr, bound.controller, bound, nil)
	}
	return hosts
}

// rememberedHostNames snapshots known remote peer names for goroutine-safe
// lookups (such as formatting offline or historical Node IDs in NodeLabel).
func (r *Runtime) rememberedHostNames() map[protocol.NodeID]string {
	root := r.localRuntime()
	names := map[protocol.NodeID]string{}
	record := func(id protocol.NodeID, candidate string) {
		if id == (protocol.NodeID{}) || id == root.controller.Target() {
			return
		}
		if name, ok := usableRemoteHostName(candidate, id.String()); ok {
			names[id] = name
		}
	}
	if root.Preferences != nil && root.Preferences.Data.Devices != nil {
		for idStr, dev := range *root.Preferences.Data.Devices {
			if dev.Name != nil {
				if id, ok := parseNodeIDString(idStr); ok {
					record(id, *dev.Name)
				}
			}
		}
	}
	if root.App != nil && root.App.Data != nil {
		for _, host := range root.App.Data.Hosts {
			if !host.Local {
				if id, ok := parseNodeIDString(host.ID); ok {
					record(id, host.Name)
				}
			}
		}
	}
	if root.resourceCatalog != nil {
		for _, source := range root.resourceCatalog.Sources {
			record(source.Node, source.Name)
		}
	}
	if root.network != nil {
		for _, peer := range root.network.snapshot() {
			record(peer.address.ID, peer.rememberedName)
			if peer.info.Name != nil {
				record(peer.address.ID, *peer.info.Name)
			}
		}
	}
	for idStr, state := range root.hosts {
		if state == nil || state.view == nil || state.view.Local {
			continue
		}
		id := state.node
		if id == (protocol.NodeID{}) {
			id, _ = parseNodeIDString(idStr)
		}
		record(id, state.view.Name)
		if state.info != nil && state.info.Name != nil {
			record(id, *state.info.Name)
		}
	}
	return names
}
