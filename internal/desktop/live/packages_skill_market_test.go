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
			if input.Source == protocol.PluginSkillcatalogSourceOfficial {
				t.Error("the skills market requested plugin-owned skills")
			}
			if input.Force {
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
			t.Fatalf("fast source blocked by pending source: %+v, %+v", m.MarketLoad, m.MarketSkills)
		}
		if m.MarketFailed || m.MarketMore || !m.Busy[settings.BusyMarket] {
			t.Fatalf("unexpected state while the second source reads: failed %v, more %v, busy %v", m.MarketFailed, m.MarketMore, m.Busy)
		}
		close(hubReady)
		synctest.Wait()
		hubReady = nil
		tester.Frame()
		if len(source.sent("search_skill_catalog")) != 2 || len(m.MarketSkills) != 2 || m.MarketSkills[1].ID != "skillhub/pdf" {
			t.Fatalf("browse should read only the first page of each source: %+v", m.MarketSkills)
		}
		if !m.MarketMore || m.MarketFailed || m.Busy[settings.BusyMarket] {
			t.Fatalf("a finished search must have a next page: more %v, failed %v, busy %v", m.MarketMore, m.MarketFailed, m.Busy)
		}
		m.MarketQuery = "draft"
		r.loadMoreMarket()
		synctest.Wait()
		m.MarketQuery = ""
		if len(source.sent("search_skill_catalog")) != 2 {
			t.Fatal("a query the Node has not searched paged the previous results")
		}
		failPage = true
		r.loadMoreMarket()
		if !m.MarketPaging || m.MarketPagingFailed || !m.Busy[settings.BusyMarket] {
			t.Fatalf("a continuation must enter the loading state: paging %v, failed %v, busy %v", m.MarketPaging, m.MarketPagingFailed, m.Busy)
		}
		synctest.Wait()
		tester.Frame()
		if len(source.sent("search_skill_catalog")) != 3 || len(m.MarketSkills) != 2 || !m.MarketMore || m.MarketPaging || !m.MarketPagingFailed || m.MarketFailed || m.Busy[settings.BusyMarket] || !tester.HasText("invalid fixture page") {
			t.Fatalf("a failed page must keep its cursor and pause pagination: %+v, more %v, paging %v, failed %v, busy %v", m.MarketSkills, m.MarketMore, m.MarketPaging, m.MarketPagingFailed, m.Busy)
		}
		r.loadMoreMarket()
		r.loadMoreMarket()
		synctest.Wait()
		tester.Frame()
		if len(source.sent("search_skill_catalog")) != 4 || len(m.MarketSkills) != 3 || m.MarketSkills[2].ID != "skillhub/next" || m.MarketMore || m.MarketPaging || m.MarketPagingFailed {
			t.Fatalf("retry should fetch only the pending source once: %+v", m.MarketSkills)
		}
		r.searchPackageMarket(1, 0, "", true)
		synctest.Wait()
		tester.Frame()
		commands := source.sent("search_skill_catalog")
		if len(commands) != 6 || m.MarketLoad.Loading || len(m.MarketSkills) != 3 || !m.MarketFailed || !tester.HasText("invalid fixture catalog") {
			t.Fatalf("failed refresh lost previously loaded cards or was not sent: %d, %+v", len(commands), m.MarketSkills)
		}
		for _, command := range commands[4:] {
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

func TestSkillMarketSelectedSourceExcludesPluginSkills(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		m := r.App.Settings.Model
		m.MarketKind, m.MarketSkillSource = 1, settings.SkillMarketSkillsSh
		var requests []protocol.CommandSearchSkillCatalogPayload
		unavailable := true
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSearchSkillCatalogPayload](request.Command.Data)
			requests = append(requests, input)
			page := protocol.DefaultPluginSkillcatalogPage()
			page.Page, page.QueryScope = input.Page, protocol.PluginSkillcatalogQueryScopeCatalog
			if input.Query == "x" {
				page.QueryScope = protocol.PluginSkillcatalogQueryScopePage
				page.Next = new(uint32(2))
				return link.Response{Output: protocol.Output{Kind: "skill_catalog", Data: page}}
			}
			if input.Source == protocol.PluginSkillcatalogSourceSkillsSh {
				page.Entries = []protocol.PluginSkillcatalogEntry{
					{ID: "owned", Name: "Bundled review", Package: new("code-review")},
					{ID: "indexed-copy", Name: "Indexed review", Origin: "Sailry/Sailry-Plugins"},
				}
				if input.Page == 1 {
					page.Next = new(uint32(2))
				} else {
					page.Unavailable = unavailable
					unavailable = false
					page.Entries = []protocol.PluginSkillcatalogEntry{{ID: "code-review", Name: "Community review", Origin: "another/team"}}
				}
			} else {
				page.Entries = []protocol.PluginSkillcatalogEntry{{ID: "hub", Name: "Registry skill", Origin: "sailry"}}
			}
			return link.Response{Output: protocol.Output{Kind: "skill_catalog", Data: page}}
		}
		tester := pluginUpdateQueueTester(r)
		search := func() {
			r.searchPackageMarket(m.MarketKind, marketSource(m), m.MarketQuery, false)
			synctest.Wait()
			tester.Frame()
		}
		search()
		if len(requests) != 1 || requests[0].Source != protocol.PluginSkillcatalogSourceSkillsSh || len(m.MarketSkills) != 0 || !m.MarketMore {
			t.Fatalf("selected source did not preserve paging through filtered entries: %+v, %+v", requests, m.MarketSkills)
		}
		r.loadMoreMarket()
		if m.MarketLoad.Loading || !m.MarketPaging {
			t.Fatalf("an empty page needs a pagination footer, not the first-page skeleton: %+v, paging %v", m.MarketLoad, m.MarketPaging)
		}
		synctest.Wait()
		tester.Frame()
		if len(requests) != 2 || requests[1].Page != 2 || len(m.MarketSkills) != 0 || !m.MarketMore || !m.MarketPagingFailed {
			t.Fatalf("an unavailable page with cached cards must keep its cursor for retry: %+v, %+v", requests, m.MarketSkills)
		}
		r.loadMoreMarket()
		synctest.Wait()
		tester.Frame()
		if len(requests) != 3 || requests[2].Source != protocol.PluginSkillcatalogSourceSkillsSh || requests[2].Page != 2 || len(m.MarketSkills) != 1 || m.MarketSkills[0].ID != "code-review" {
			t.Fatalf("selected source lost an independent skill on its next page: %+v, %+v", requests, m.MarketSkills)
		}
		m.MarketSkillSource = settings.SkillMarketSkillhub
		search()
		if len(requests) != 4 || requests[3].Source != protocol.PluginSkillcatalogSourceSkillhub || len(m.MarketSkills) != 1 || m.MarketSkills[0].ID != "hub" || m.MarketMore {
			t.Fatalf("changing sources retained earlier cards or pagination: %+v, %+v", requests, m.MarketSkills)
		}
		m.MarketSkillSource, m.MarketQuery = settings.SkillMarketSkillsSh, "x"
		search()
		if len(requests) != 5 || m.MarketMore || len(m.MarketSkills) != 0 {
			t.Fatalf("an empty page-scoped result must stop automatic paging: %+v, %+v", requests, m.MarketSkills)
		}
	})
}
