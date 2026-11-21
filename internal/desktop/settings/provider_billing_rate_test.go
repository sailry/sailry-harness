package settings

import (
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestBillingRateText(t *testing.T) {
	for _, tc := range []struct {
		text string
		bps  uint32
		ok   bool
	}{
		{"100", 10000, true}, {" 85.5 ", 8550, true}, {"12.34", 1234, true}, {"0", 0, true}, {"0.01", 1, true},
		{".5", 50, true}, {"7.", 700, true}, {"150%", 15000, true}, {"42949672.95", math.MaxUint32, true},
		{"", 0, false}, {".", 0, false}, {"-1", 0, false}, {"+1", 0, false}, {"1.234", 0, false}, {"1e2", 0, false},
		{"1,5", 0, false}, {"abc", 0, false}, {"42949672.96", 0, false}, {"99999999999999999999", 0, false},
	} {
		bps, ok := parseBillingRate(tc.text)
		if ok != tc.ok || ok && bps != tc.bps {
			t.Errorf("parseBillingRate(%q) = %d, %t; want %d, %t", tc.text, bps, ok, tc.bps, tc.ok)
		}
	}
	for _, tc := range []struct {
		bps  *uint32
		text string
	}{
		{nil, "100"}, {billingRatePointer(10000), "100"}, {billingRatePointer(0), "0"}, {billingRatePointer(1250), "12.5"},
		{billingRatePointer(1234), "12.34"}, {billingRatePointer(5), "0.05"}, {billingRatePointer(math.MaxUint32), "42949672.95"},
	} {
		text := formatBillingRate(tc.bps)
		if text != tc.text {
			t.Errorf("formatBillingRate(%v) = %q, want %q", tc.bps, text, tc.text)
		}
		if bps, ok := parseBillingRate(text); !ok || tc.bps != nil && bps != *tc.bps {
			t.Errorf("%q does not read back as %v", text, tc.bps)
		}
	}
}

func billingRatePointer(value uint32) *uint32 { return &value }

func billingRateEditorFixture() (*Model, *[]ProviderDraft) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.Brands = []preview.Brand{{ID: "fixture", Methods: []preview.Method{
		{ID: "compatible", Name: "Gateway", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{{ID: "compatible/0", API: "responses", Custom: true}}},
		{ID: "login", Name: "Sign-in", Access: "provider_access_login", Login: "provider_login_chat_gpt", Auth: preview.AuthOAuth, Routes: []preview.Route{{ID: "login/0", API: "responses"}}},
		{ID: "plugin", Name: "Plugin", Access: "provider_access_login", Login: "provider_access_login", Auth: preview.AuthPlugin, Routes: []preview.Route{{ID: "plugin/0", API: "plugin"}}},
	}}}
	saved := &[]ProviderDraft{}
	m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
		*saved = append(*saved, draft)
		done(ProviderSaveResult{Failed: []string{"fixture-node"}})
	}
	return m, saved
}

// A new API-key channel starts at 100% and saves no rate until one is
// typed; an invalid rate keeps the draft and the editor open.
func TestProviderBillingRateNewChannel(t *testing.T) {
	m, saved := billingRateEditorFixture()
	e := &m.provider
	e.add(m)
	e.configure(m.Brands[0].Methods[0])
	e.endpoint, e.key = "https://gateway.example/v1", "fixture-key"
	e.models = draftsOf([]preview.ChannelModel{{ID: "model", Name: "model", Context: 128000}})
	if !e.billsByRate() || e.billingRate != "100" {
		t.Fatalf("new API-key channel rate = %q, shown %t", e.billingRate, e.billsByRate())
	}
	if problem := e.save(m); problem != "" || len(*saved) != 1 {
		t.Fatalf("saving the default rate returned %q with %d saves", problem, len(*saved))
	}
	if got := (*saved)[0].Channel.BillingRateBps; got != nil {
		t.Fatalf("an untouched 100%% saved a rate of %d basis points", *got)
	}
	for _, text := range []string{"", "-5", "12.345", "abc", "42949672.96"} {
		e.billingRate = text
		if problem := e.save(m); problem != "provider_billing_rate_invalid" {
			t.Fatalf("rate %q was accepted: %q", text, problem)
		}
		if e.billingRate != text || !e.open || len(*saved) != 1 {
			t.Fatalf("an invalid rate %q lost the draft or saved: %q open=%t saves=%d", text, e.billingRate, e.open, len(*saved))
		}
	}
	for _, tc := range []struct {
		text string
		want uint32
	}{{"0", 0}, {"85.5", 8550}, {"100", 10000}} {
		e.billingRate = tc.text
		if problem := e.save(m); problem != "" {
			t.Fatalf("rate %q was refused: %s", tc.text, problem)
		}
		if got := (*saved)[len(*saved)-1].Channel.BillingRateBps; tc.text == "100" && got != nil || tc.text != "100" && (got == nil || *got != tc.want) {
			t.Fatalf("rate %q saved %v", tc.text, got)
		}
	}
	e.billingRate = "85.5"
	// A rate typed before another method is chosen stays the draft.
	e.back()
	e.configure(m.Brands[0].Methods[0])
	if e.billingRate != "85.5" {
		t.Fatalf("choosing the method again reset the rate to %q", e.billingRate)
	}
}

// Sign-ins and plugin connections have no rate: neither shown, checked nor
// saved, whatever the draft holds.
func TestProviderBillingRateHiddenForSignIns(t *testing.T) {
	for _, index := range []int{1, 2} {
		m, saved := billingRateEditorFixture()
		e := &m.provider
		e.add(m)
		e.configure(m.Brands[0].Methods[index])
		e.billingRate = "not a rate"
		if e.billsByRate() {
			t.Fatalf("method %q offers a billing rate", e.method.ID)
		}
		if problem := e.save(m); problem != "" || len(*saved) != 1 {
			t.Fatalf("saving method %q returned %q with %d saves", e.method.ID, problem, len(*saved))
		}
		if got := (*saved)[0].Channel.BillingRateBps; got != nil {
			t.Fatalf("method %q saved a rate of %d basis points", e.method.ID, *got)
		}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			providerEditorDialog(c, m)
		}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
		tt.SetLocale("en")
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "provider_billing_rate")+" (%)"); ok {
			t.Fatalf("method %q shows the billing rate field", e.method.ID)
		}
	}
}

// An edited channel shows its saved rate, keeps nil while it stays at 100%,
// and keeps a saved 100% and a saved zero explicit.
func TestProviderBillingRateEdit(t *testing.T) {
	for _, tc := range []struct {
		name  string
		saved *uint32
		text  string
		typed string
		want  *uint32
	}{
		{"without a rate", nil, "100", "100.00", nil},
		{"without a rate changed", nil, "100", "120", billingRatePointer(12000)},
		{"a saved 100%", billingRatePointer(10000), "100", "100", billingRatePointer(10000)},
		{"a saved zero", billingRatePointer(0), "0", "0", billingRatePointer(0)},
		{"a saved fraction", billingRatePointer(12345), "123.45", "123.45", billingRatePointer(12345)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			m, saved := billingRateEditorFixture()
			m.Channels = []preview.Channel{{ID: "saved", Revision: 3, Name: "Saved", Brand: "fixture", Method: "compatible/0", API: "responses",
				Endpoint: "https://gateway.example/v1", Auth: preview.AuthKey, KeySet: true, BillingRateBps: tc.saved,
				Models: []preview.ChannelModel{{ID: "model", Name: "model", Context: 128000, DefaultEffort: "default"}}}}
			e := &m.provider
			e.edit(m, &m.Channels[0])
			if e.billingRate != tc.text {
				t.Fatalf("edit shows %q, want %q", e.billingRate, tc.text)
			}
			e.billingRate = tc.typed
			if problem := e.save(m); problem != "" || len(*saved) != 1 {
				t.Fatalf("save failed: %s", problem)
			}
			got := (*saved)[0].Channel.BillingRateBps
			if (got == nil) != (tc.want == nil) || got != nil && *got != *tc.want {
				t.Fatalf("saved rate %v, want %v", got, tc.want)
			}
			if tc.saved != nil && got == tc.saved {
				t.Fatal("the draft shares the channel's rate")
			}
		})
	}
}

// The field sits in the form at the supported widths with % inside the
// input's end; a click on the sign edits the input, and an invalid rate
// marks it.
func TestProviderBillingRateField(t *testing.T) {
	for _, size := range [][2]int{{1100, 900}, {560, 900}} {
		m, _ := billingRateEditorFixture()
		e := &m.provider
		e.add(m)
		e.configure(m.Brands[0].Methods[0])
		e.endpoint, e.key = "https://gateway.example/v1", "fixture-key"
		e.models = draftsOf([]preview.ChannelModel{{ID: "model", Name: "model", Context: 128000}})
		tt := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			providerEditorDialog(c, m)
		}, size[0], size[1], ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		label := i18n.In("en", "provider_billing_rate") + " (%)"
		field, ok := tt.AccessByRole(ui.RoleTextField, label)
		if !ok || field.Value != "100" || field.Invalid {
			t.Fatalf("%v: billing rate field = %+v, %t", size, field, ok)
		}
		name, _ := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "settings_name"))
		if field.Bounds.X != name.Bounds.X || field.Bounds.W != name.Bounds.W || field.Bounds.X+field.Bounds.W > float32(size[0]) {
			t.Fatalf("%v: rate field %+v is not aligned with the name field %+v", size, field.Bounds, name.Bounds)
		}
		sign, ok := tt.Find("%")
		if !ok || sign.X < field.Bounds.X+field.Bounds.W/2 || sign.X+sign.W > field.Bounds.X+field.Bounds.W ||
			sign.Y < field.Bounds.Y || sign.Y+sign.H > field.Bounds.Y+field.Bounds.H {
			t.Fatalf("%v: %% sign %+v is not inside the input's end %+v", size, sign, field.Bounds)
		}
		tt.ClickAt(sign.X+sign.W/2, sign.Y+sign.H/2)
		tt.Frame()
		tt.Type(".5")
		tt.Frame()
		if e.billingRate != "100.5" {
			t.Fatalf("%v: typing after a click on the sign made %q", size, e.billingRate)
		}
		tt.Type("55")
		tt.Frame()
		field, ok = tt.AccessByRole(ui.RoleTextField, label)
		if problem := e.Check(); !ok || !field.Invalid || problem != "provider_billing_rate_invalid" {
			t.Fatalf("%v: rate %q: field present %t, invalid %t, validation %q", size, e.billingRate, ok, field.Invalid, problem)
		}
	}
}
