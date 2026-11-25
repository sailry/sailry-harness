package theme

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
)

func TestThemeMarketUnavailableWithoutCatalog(t *testing.T) {
	t.Parallel()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		http.Error(writer, "unavailable", http.StatusServiceUnavailable)
	}))
	defer server.Close()
	for _, existing := range []bool{false, true} {
		name := "fresh profile"
		if existing {
			name = "installed theme"
		}
		t.Run(name, func(t *testing.T) {
			profile := NewProfile(t.TempDir())
			snapshot := profileFixture(t, "coast", "0.1.0")
			if existing {
				if err := profile.Install(t.Context(), snapshot, nil); err != nil {
					t.Fatal(err)
				}
			}
			market := NewMarket(profile)
			market.Client, market.CatalogURL = server.Client(), server.URL
			listing, err := market.List(t.Context(), false)
			if err != nil || !listing.Unavailable || listing.Version != 1 || listing.Commit != "" || len(listing.Themes) != 0 || listing.Selection("coast") != nil {
				t.Fatalf("unavailable catalog = %+v, %v", listing, err)
			}
			installed, err := profile.Installed(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if existing {
				if len(installed) != 1 || installed[0].Digest != snapshot.Digest {
					t.Fatal("catalog failure changed an installed theme")
				}
			} else if len(installed) != 0 {
				t.Fatal("catalog failure installed a theme")
			} else if _, err := os.Stat(profile.Root); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("catalog failure created the theme profile: %v", err)
			}
		})
	}
}
