package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSkillMarketLoadsSourcesIndependently(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		m := r.App.Settings.Model
		m.MarketKind = 1
		hubReady := make(chan struct{})
		failPage := false
		defer func() {
			if hubReady != nil {
				close(hubReady)
			}
		}()
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSearchSkillCatalogPayload](request.Command.Data)
			if input.Source == protocol.PluginSkillcatalogSourceOfficial || input.Force {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid fixture catalog"}}
			}
			if input.Page == 2 && failPage {
				failPage = false
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid fixture page"}}
			}
			page := protocol.DefaultPluginSkillcatalogPage()
			page.QueryScope, page.Page = protocol.PluginSkillcatalogQueryScopeCatalog, input.Page
			if input.Source == protocol.PluginSkillcatalogSourceSkillhub && input.Page == 1 {
				<-hubReady
				page.Next = new(uint32(2))
			}
			entry := protocol.DefaultPluginSkillcatalogEntry()
			entry.ID, entry.Name = string(input.Source)+"/pdf", "PDF"
			if input.Page == 2 {
				entry.ID = string(input.Source) + "/next"
			}
			page.Entries = append(page.Entries, entry)
			return link.Response{Output: protocol.Output{Kind: "skill_catalog", Data: page}}
		}
		tester := pluginUpdateQueueTester(r)
		r.searchPackageMarket(1, 0, "", false)
		synctest.Wait()
		tester.Frame()
		if m.MarketLoad.Loading || !m.MarketLoad.Loaded || len(m.MarketSkills) != 1 || m.MarketSkills[0].ID != "skills_sh/pdf" {
			t.Fatalf("fast source blocked by pending or failed source: %+v, %+v", m.MarketLoad, m.MarketSkills)
		}
		if !tester.HasText("invalid fixture catalog") || tester.HasText("Plugin operation failed") {
			t.Fatalf("missing catalog failure detail: %q", tester.Texts())
		}
		if !m.MarketFailed || m.MarketMore || !m.Busy[settings.BusyMarket] {
			t.Fatalf("a failed source must offer Retry while another still reads: failed %v, more %v, busy %v", m.MarketFailed, m.MarketMore, m.Busy)
		}
		close(hubReady)
		synctest.Wait()
		hubReady = nil
		tester.Frame()
		if len(source.sent("search_skill_catalog")) != 3 || len(m.MarketSkills) != 2 || m.MarketSkills[1].ID != "skillhub/pdf" {
			t.Fatalf("browse should read only the first page of each source: %+v", m.MarketSkills)
		}
		if !m.MarketMore || !m.MarketFailed || m.Busy[settings.BusyMarket] {
			t.Fatalf("a finished partial search must keep Retry and offer Load more: more %v, failed %v, busy %v", m.MarketMore, m.MarketFailed, m.Busy)
		}
		m.MarketQuery = "draft"
		r.loadMoreSkillMarket()
		synctest.Wait()
		m.MarketQuery = ""
		if len(source.sent("search_skill_catalog")) != 3 {
			t.Fatal("a query the Node has not searched paged the previous results")
		}
		failPage = true
		r.loadMoreSkillMarket()
		synctest.Wait()
		tester.Frame()
		if len(source.sent("search_skill_catalog")) != 4 || len(m.MarketSkills) != 2 || !m.MarketMore || m.Busy[settings.BusyMarket] || !tester.HasText("invalid fixture page") {
			t.Fatalf("a failed page must keep the cards and stay next: %+v, more %v, busy %v", m.MarketSkills, m.MarketMore, m.Busy)
		}
		r.loadMoreSkillMarket()
		r.loadMoreSkillMarket()
		synctest.Wait()
		tester.Frame()
		if len(source.sent("search_skill_catalog")) != 5 || len(m.MarketSkills) != 3 || m.MarketSkills[2].ID != "skillhub/next" || m.MarketMore {
			t.Fatalf("explicit continuation should fetch only the pending source once: %+v", m.MarketSkills)
		}
		r.searchPackageMarket(1, 0, "", true)
		synctest.Wait()
		tester.Frame()
		commands := source.sent("search_skill_catalog")
		if len(commands) != 8 || m.MarketLoad.Loading || len(m.MarketSkills) != 3 || !m.MarketFailed {
			t.Fatalf("failed refresh lost previously loaded cards or was not sent: %d, %+v", len(commands), m.MarketSkills)
		}
		for _, command := range commands[5:] {
			input, _ := detail[protocol.CommandSearchSkillCatalogPayload](command.Data)
			if !input.Force || input.Page != 1 {
				t.Fatalf("refresh did not request a fresh first page: %+v", input)
			}
		}
		r.searchPackageMarket(1, 0, "other", true)
		synctest.Wait()
		tester.Frame()
		if m.MarketLoad.Loading || m.MarketLoad.Loaded || len(m.MarketSkills) != 0 || !m.MarketFailed || m.MarketMore {
			t.Fatalf("a wholly failed new query must finish failed without old rows: %+v, %+v, failed %v", m.MarketLoad, m.MarketSkills, m.MarketFailed)
		}
	})
}
