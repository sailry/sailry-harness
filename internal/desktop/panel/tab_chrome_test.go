package panel

import "testing"

func TestBrowserTabChromeLifetime(t *testing.T) {
	s := New(nil)
	parked := s.Browse("https://parked.example/first")
	page := s.pagesOf[parked.ID]
	page.Go("https://parked.example/second")
	page.shown, page.address = page.address, "unfinished address"
	s.Mount(nil, Tab{})

	for range 8 {
		tab := s.Browse("https://closed.example/")
		s.Close(tab)
		if len(s.pagesOf) != 1 || s.pagesOf[tab.ID] != nil {
			t.Fatalf("closing browser %q left %d entries in the chrome map", tab.ID, len(s.pagesOf))
		}
	}
	tab := s.Browse("https://cleared.example/")
	s.Clear()
	if s.pagesOf[tab.ID] != nil {
		t.Fatal("clearing the mounted strip retained a closed browser's chrome")
	}

	s.PruneBrowserChrome(func(tab Tab) bool { return tab == parked })
	s.Mount([]Tab{parked}, parked)
	if got := s.pagesOf[parked.ID]; got != page || got.address != "unfinished address" || got.shown != "https://parked.example/second" || len(got.history) != 2 {
		t.Fatalf("switching away and back lost the parked browser's chrome: %+v", got)
	}
	s.Mount(nil, Tab{})
	s.PruneBrowserChrome(func(Tab) bool { return false })
	if len(s.pagesOf) != 0 {
		t.Fatal("closing the parked browser left its chrome in the window")
	}
}

func TestTerminalTabChromeLifetime(t *testing.T) {
	s := New(nil)
	tab := Tab{Kind: Terminal, ID: "1"}
	s.Show(tab)
	s.Find()
	chrome := s.terms[tab.ID]
	chrome.query, chrome.match = "remember the query", 2
	s.TerminalActions = &TerminalActions{Close: func(string) { t.Fatal("the terminal closed without confirmation") }}
	s.Close(tab)
	s.Mount(nil, Tab{})
	s.Mount([]Tab{tab}, tab)
	s.Clear()
	s.Show(tab)
	if got := s.terms[tab.ID]; got != chrome || !got.find || got.query != "remember the query" || got.match != 2 {
		t.Fatalf("parking and reopening lost the running terminal's search state: %+v", got)
	}

	s.TerminalActions = nil
	s.Close(tab)
	if s.terms[tab.ID] != nil {
		t.Fatal("closing a preview terminal retained its chrome")
	}
}
