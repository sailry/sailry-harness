package providerstatusdesktop

import (
	"context"
	"encoding/hex"
	"errors"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Register installs the native provider-status desktop module into assembly.
func Register(assembly *shell.Assembly) {
	assembly.Locales = append(assembly.Locales, Locales)
	assembly.PluginControlReplacements = append(assembly.PluginControlReplacements,
		shell.PluginControlReplacement{
			Package: packageName,
			Slot:    protocol.PluginUiSlotFooter,
			ID:      footerControlID,
		},
		shell.PluginControlReplacement{
			Package: packageName,
			Slot:    protocol.PluginUiSlotProvider,
			ID:      providerControlID,
		},
	)
	assembly.MetricContributions = append(assembly.MetricContributions, shell.MetricContribution{
		Bind: bindProviderStatus,
	})
}

func parseNodeID(value string, fallback protocol.NodeID) (protocol.NodeID, bool) {
	if value == "" {
		if fallback != (protocol.NodeID{}) {
			return fallback, true
		}
		return protocol.NodeID{}, false
	}
	var id protocol.NodeID
	raw, err := hex.DecodeString(value)
	if err != nil || len(raw) != len(id) {
		return protocol.NodeID{}, false
	}
	copy(id[:], raw)
	return id, true
}

func bindProviderStatus(window shell.WindowContext) shell.MetricBinding {
	services, ok := viewapi.WindowServices(window.Context)
	if !ok {
		return shell.MetricBinding{}
	}

	fetch := func(ctx context.Context, node protocol.NodeID, provider protocol.ProviderID, expectedRev uint64) (protocol.ProviderAccountUsage, error) {
		if services.PrepareOn == nil || services.ExecuteOn == nil {
			return protocol.ProviderAccountUsage{}, errors.New("provider account usage service is unavailable")
		}
		req, err := services.PrepareOn(node, protocol.Command{
			Kind: "read_provider_account_usage",
			Data: &protocol.CommandReadProviderAccountUsagePayload{
				Provider:         provider,
				ExpectedRevision: expectedRev,
			},
		})
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		out, err := services.ExecuteOn(ctx, node, req)
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		if out.Kind != "provider_account_usage" {
			return protocol.ProviderAccountUsage{}, errors.New("unexpected provider account usage output")
		}
		switch payload := out.Data.(type) {
		case protocol.ProviderAccountUsage:
			return payload, nil
		case *protocol.ProviderAccountUsage:
			if payload != nil {
				return *payload, nil
			}
		}
		return protocol.ProviderAccountUsage{}, errors.New("invalid provider account usage payload")
	}

	tracker := newAccountUsageTracker(window.Context, trackerDeps{
		fetch: fetch,
		wake:  services.Wake,
	})

	isNodeConnected := func(node protocol.NodeID) bool {
		if services.Nodes == nil {
			if services.Connected != nil && node == services.Node {
				return services.Connected()
			}
			return true
		}
		for _, info := range services.Nodes() {
			if info.ID == node {
				return info.Connected
			}
		}
		return false
	}

	// window.Visible reads UI-thread App fields, so cache it in an atomic.Bool
	// updated only on UI-thread callbacks (Bind, Sync, Lead, Provider) and read
	// safely from background OnChange notifications.
	var visible atomic.Bool
	refreshVisible := func() bool {
		v := window.Visible == nil || window.Visible()
		visible.Store(v)
		return v
	}
	refreshVisible()

	reconcile := func() {
		if !visible.Load() {
			tracker.PauseAll()
			return
		}
		if services.Snapshot == nil {
			return
		}
		tracker.Reconcile(func(k usageScopeKey) bool {
			if !isNodeConnected(k.node) {
				return false
			}
			snap, exists := services.Snapshot(k.node)
			if !exists || snap == nil {
				return false
			}
			var creds map[protocol.CredentialID]protocol.Credential
			if services.Credentials != nil {
				creds = services.Credentials(k.node)
			}
			for _, prov := range snap.Providers {
				if prov.ID != k.provider {
					continue
				}
				if prov.Revision != k.providerRev {
					return false
				}
				if creds == nil {
					return supportsAccountUsage(prov.Authentication) &&
						prov.Credential != nil &&
						prov.Credential.ID == k.credID
				}
				cred, valid := validCredentialForProvider(prov, creds)
				return valid && cred.ID == k.credID && cred.Revision == k.credRev
			}
			return false
		})
	}

	unsub := func() {}
	if services.OnChange != nil {
		unsub = services.OnChange(reconcile)
	}

	return shell.MetricBinding{
		Lead: func(c *ui.Context, scope shell.ComposerMetricScope) []conversation.Metric {
			if scope.Demo || !scope.Connected || scope.Credential == nil || !supportsAccountUsage(scope.Auth) {
				return nil
			}
			if !refreshVisible() {
				return nil
			}
			nodeID, ok := parseNodeID(scope.Node, services.Node)
			if !ok {
				return nil
			}
			provID, err := protocol.ParseProviderID(scope.Provider)
			if err != nil {
				return nil
			}
			credRev := scope.CredRev
			if services.Credentials != nil {
				if creds := services.Credentials(nodeID); creds != nil {
					cred, exists := creds[scope.Credential.ID]
					if !exists || cred.Provider != provID || cred.Revoked {
						return nil
					}
					credRev = cred.Revision
				}
			}
			snapshot := tracker.Observe(usageScopeKey{
				node:        nodeID,
				provider:    provID,
				providerRev: scope.ProviderRev,
				credID:      scope.Credential.ID,
				credRev:     credRev,
			}, scope.Connected)
			metric, shown := buildFooterMetric(snapshot, func(key string) string { return i18n.T(c, key) }, time.Local)
			if !shown {
				return nil
			}
			return []conversation.Metric{metric}
		},
		Provider: func(c *ui.Context, nodeStr, channelStr string) *settings.ChannelUsage {
			if !refreshVisible() {
				return nil
			}
			nodeID, ok := parseNodeID(nodeStr, services.Node)
			if !ok || !isNodeConnected(nodeID) {
				return nil
			}
			provID, err := protocol.ParseProviderID(channelStr)
			if err != nil {
				return nil
			}
			if services.Snapshot == nil || services.Credentials == nil {
				return nil
			}
			snap, exists := services.Snapshot(nodeID)
			if !exists || snap == nil {
				return nil
			}
			creds := services.Credentials(nodeID)
			for _, prov := range snap.Providers {
				if prov.ID != provID {
					continue
				}
				cred, valid := validCredentialForProvider(prov, creds)
				if !valid {
					return nil
				}
				usage := tracker.Observe(usageScopeKey{
					node:        nodeID,
					provider:    prov.ID,
					providerRev: prov.Revision,
					credID:      cred.ID,
					credRev:     cred.Revision,
				}, true)
				return buildChannelUsage(usage, func(key string) string { return i18n.T(c, key) }, time.Local)
			}
			return nil
		},
		Sync: func() {
			if !refreshVisible() {
				tracker.PauseAll()
				return
			}
			tracker.PauseHidden()
			reconcile()
		},
		Close: func() {
			visible.Store(false)
			unsub()
			tracker.Close()
		},
	}
}
