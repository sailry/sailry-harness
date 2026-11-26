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
	if !ok || services.ProviderAccount == nil {
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
		tracker.Reconcile(func(k usageScopeKey) bool {
			account, ok := services.ProviderAccount(k.node, k.provider)
			return ok && supportsAccountUsage(account.Authentication) && account.ProviderRevision == k.providerRev &&
				account.Credential == k.credID && (!account.CredentialKnown || account.CredentialRevision == k.credRev)
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
			account, ok := services.ProviderAccount(nodeID, provID)
			if !ok || account.ProviderRevision != scope.ProviderRev || account.Credential != scope.Credential.ID || !supportsAccountUsage(account.Authentication) {
				return nil
			}
			credRev := scope.CredRev
			if account.CredentialKnown {
				credRev = account.CredentialRevision
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
			if !ok {
				return nil
			}
			provID, err := protocol.ParseProviderID(channelStr)
			if err != nil {
				return nil
			}
			account, ok := services.ProviderAccount(nodeID, provID)
			if !ok || !account.CredentialKnown || !supportsAccountUsage(account.Authentication) {
				return nil
			}
			usage := tracker.Observe(usageScopeKey{
				node:        nodeID,
				provider:    provID,
				providerRev: account.ProviderRevision,
				credID:      account.Credential,
				credRev:     account.CredentialRevision,
			}, true)
			return buildChannelUsage(usage, func(key string) string { return i18n.T(c, key) }, time.Local)
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
