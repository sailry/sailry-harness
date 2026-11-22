package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginMarketRetryAndPartialResults(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		r.App = shell.NewLiveWithAssembly(r.App.Data, desktop.Harness())
		m := r.App.Settings.Model
		m.MarketSource, m.MarketQuery = 1, "fixture"
		m.PackageActions = &settings.PackageActions{Catalog: r.packageCatalogService()}
		failPage := true
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSearchPluginCatalogPayload](request.Command.Data)
			if input.Page == 2 && failPage {
				failPage = false
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
		r.App.Navigate(shell.MarketPage)
		r.App.SyncNativePages()
		scope := r.App.Settings.MarketScope()
		synctest.Wait()
		tester.Frame()
		if len(source.sent("search_plugin_catalog")) != 1 || !m.MarketMore || len(m.MarketPlugins) != 1 || m.MarketPlugins[0].ID != "first" {
			t.Fatalf("the initial search must show the first page and offer a continuation: %+v", m.MarketPlugins)
		}
		scope.More()
		synctest.Wait()
		tester.Frame()
		if !m.MarketPagingFailed || m.MarketFailed || len(m.MarketPlugins) != 1 || m.MarketPlugins[0].ID != "first" || !tester.HasText("catalog page unavailable") {
			t.Fatalf("a later page failure should keep the loaded plugin and report the reason: %+v, %v", m.MarketPlugins, tester.Texts())
		}

		scope.More()
		scope.More()
		synctest.Wait()
		tester.Frame()
		if m.MarketPagingFailed || m.MarketMore || len(m.MarketPlugins) != 2 || m.MarketPlugins[1].ID != "second" {
			t.Fatalf("retry did not append the failed page: %+v, failed %v", m.MarketPlugins, m.MarketPagingFailed)
		}

		scope.Retry()
		synctest.Wait()
		tester.Frame()
		if m.MarketFailed || !m.MarketMore || len(m.MarketPlugins) != 1 {
			t.Fatalf("refresh did not replace the first page: %+v, failed %v", m.MarketPlugins, m.MarketFailed)
		}
		r.App.Navigate(shell.SettingsPage)
		r.App.SyncNativePages()
		scope.More()
		scope.Retry()
		synctest.Wait()
		requests := source.sent("search_plugin_catalog")
		if len(requests) != 4 {
			t.Fatalf("catalog received %d requests, want 4", len(requests))
		}
		for index, request := range requests {
			input, ok := detail[protocol.CommandSearchPluginCatalogPayload](request.Data)
			if !ok || input.Source != protocol.PluginCatalogSourceThirdParty || input.Query != "fixture" || input.Page != [...]uint32{1, 2, 2, 1}[index] || input.Force != (index == 3) {
				t.Fatalf("request %d: %+v", index, input)
			}
		}
	})
}
