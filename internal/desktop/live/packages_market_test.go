package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginMarketRetryAndPartialResults(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		m := r.App.Settings.Model
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSearchPluginCatalogPayload](request.Command.Data)
			if input.Page == 2 && !input.Force {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "catalog page unavailable"}}
			}
			entry := protocol.DefaultPluginCatalogEntry()
			entry.ID, entry.Name = "first", "First plugin"
			if input.Page == 2 {
				entry.ID, entry.Name = "second", "Second plugin"
			}
			page := protocol.PluginCatalogPage{Entries: []protocol.PluginCatalogEntry{entry}, Page: input.Page, Pages: 2}
			return link.Response{Output: protocol.Output{Kind: "plugin_catalog", Data: page}}
		}
		tester := pluginUpdateQueueTester(r)
		r.searchPackageMarket(0, 1, "fixture", false)
		synctest.Wait()
		tester.Frame()
		if !m.MarketFailed || len(m.MarketPlugins) != 1 || m.MarketPlugins[0].ID != "first" || !tester.HasText("catalog page unavailable") {
			t.Fatalf("a later page failure should keep the loaded plugin and report the reason: %+v, %v", m.MarketPlugins, tester.Texts())
		}

		r.searchPackageMarket(0, 1, "fixture", true)
		synctest.Wait()
		tester.Frame()
		if m.MarketFailed || len(m.MarketPlugins) != 2 || m.MarketPlugins[1].ID != "second" {
			t.Fatalf("retry did not fetch both pages: %+v, failed %v", m.MarketPlugins, m.MarketFailed)
		}
		requests := source.sent("search_plugin_catalog")
		if len(requests) != 4 {
			t.Fatalf("catalog received %d requests, want 4", len(requests))
		}
		for index, request := range requests {
			input, ok := detail[protocol.CommandSearchPluginCatalogPayload](request.Data)
			if !ok || input.Source != protocol.PluginCatalogSourceThirdParty || input.Query != "fixture" || input.Page != uint32(index%2+1) || input.Force != (index >= 2) {
				t.Fatalf("request %d: %+v", index, input)
			}
		}
	})
}
