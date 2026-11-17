package providers

import "testing"

func TestCatalogRejectsEmptySnapshot(t *testing.T) {
	if _, err := ParseCatalog([]byte(`{"fixture":{"models":{}}}`)); err == nil {
		t.Fatal("empty catalog could replace usable model metadata")
	}
}
