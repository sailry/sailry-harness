package theme

import (
	"errors"
	"fmt"
	"math"
	"net/url"
	"path"
	"strings"
	"unicode"
)

func themeText(value string, limit int, multiline bool) bool {
	return strings.TrimSpace(value) != "" && len(value) <= limit && !strings.ContainsFunc(value, func(r rune) bool {
		return unicode.IsControl(r) && !(multiline && (r == '\n' || r == '\r' || r == '\t'))
	})
}

func themeHomepage(value string) error {
	parsed, err := url.Parse(value)
	if err != nil {
		return errors.New("invalid theme homepage")
	}
	credentials := false
	if parsed.User != nil {
		_, password := parsed.User.Password()
		credentials = parsed.User.Username() != "" || password
	}
	scheme := strings.ToLower(parsed.Scheme)
	if (scheme != "https" && scheme != "http") || parsed.Hostname() == "" || credentials || strings.ContainsFunc(value, func(r rune) bool { return unicode.IsControl(r) || unicode.IsSpace(r) }) {
		return errors.New("invalid theme homepage")
	}
	return nil
}

func validThemeID(value string) bool {
	if len(value) == 0 || len(value) > 64 || value == BuiltinID || value[0] == '-' {
		return false
	}
	for _, r := range []byte(value) {
		if !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '-') {
			return false
		}
	}
	return true
}

func themeRange(value, min, max float32) bool {
	return value >= min && value <= max
}
func themeInteger(value, min, max float32) bool {
	return themeRange(value, min, max) && value == float32(math.Trunc(float64(value)))
}

func (value License) validate() error {
	if !themeText(value.Author, 512, false) || (strings.TrimSpace(value.Text) == "" || len(value.Text) > 16*1024) {
		return errors.New("theme package requires author and license text")
	}
	return nil
}

func (value *Package) validate() error {
	if value.Version != 1 {
		return errors.New("unsupported theme package version")
	}
	if !validThemeID(value.ID) {
		return errors.New("invalid theme package ID")
	}
	if !themeText(value.Name, 160, false) {
		return errors.New("invalid theme package name")
	}
	if err := value.License.validate(); err != nil {
		return err
	}
	if value.Light == nil || value.Dark == nil || value.Light.Theme.Mode != "light" || value.Dark.Theme.Mode != "dark" {
		return errors.New("theme package requires light and dark variants")
	}
	for _, variant := range []*Variant{value.Light, value.Dark} {
		if err := validateEffects(variant); err != nil {
			return err
		}
		for key, message := range variant.Messages {
			if key != "user" && key != "assistant" {
				return fmt.Errorf("unsupported message style: %s", key)
			}
			if !themeInteger(message.BorderWidth, 0, 4) || (message.Radius != nil && !themeInteger(*message.Radius, 0, 32)) {
				return fmt.Errorf("invalid message dimensions: %s", key)
			}
		}
		if !themeInteger(variant.Composer.BorderWidth, 0, 4) {
			return errors.New("invalid composer border width")
		}
		for key, card := range variant.Cards {
			switch key {
			case "explore", "build", "review", "plan":
			default:
				return fmt.Errorf("unsupported welcome card: %s", key)
			}
			if !themeInteger(card.BorderWidth, 0, 4) || (card.Radius != nil && !themeInteger(*card.Radius, 0, 32)) {
				return fmt.Errorf("invalid welcome card dimensions: %s", key)
			}
		}
		for slot, asset := range variant.Assets {
			if _, known := themeSlots[slot]; !known {
				return fmt.Errorf("unsupported theme slot: %s", slot)
			}
			if asset == nil {
				return errors.New("theme decoration must be an object")
			}
			switch asset.Fit {
			case Cover, Contain, Fill, Tile:
			default:
				return fmt.Errorf("unknown theme fit: %s", asset.Fit)
			}
			if !themeInteger(asset.TileSize, 32, 1024) || (asset.Fit == Tile && !strings.HasPrefix(slot, "background.")) {
				return fmt.Errorf("invalid theme tiling: %s", slot)
			}
			if err := validateThemePath(asset.Path); err != nil {
				return err
			}
			minimum, maximum := float32(16), float32(96)
			if slot == "hero.new_session" {
				maximum = 240
			}
			if slot == "button.new_session" {
				minimum = 4
			}
			if !themeRange(asset.Opacity, 0, 1) || !themeRange(asset.Alignment[0], 0, 1) || !themeRange(asset.Alignment[1], 0, 1) || !themeInteger(asset.Height, minimum, maximum) {
				return fmt.Errorf("invalid theme decoration dimensions: %s", slot)
			}
			if asset.License != nil {
				if err := asset.License.validate(); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

func validateThemePath(value string) error {
	if !strings.HasPrefix(value, "assets/") || len(value) > 512 || strings.ContainsAny(value, "\\:?#") || strings.ContainsFunc(value, unicode.IsControl) {
		return errors.New("theme resource must be a normalized path under assets/")
	}
	for _, part := range strings.Split(value, "/") {
		if part == "" || part == "." || part == ".." {
			return errors.New("theme resource must be a normalized path under assets/")
		}
	}
	switch strings.ToLower(path.Ext(value)) {
	case ".png", ".jpg", ".jpeg", ".webp", ".gif":
		return nil
	default:
		return errors.New("unsupported theme image format")
	}
}

func validateEffects(variant *Variant) error {
	particles := 0
	for slot, layers := range variant.Effects {
		_, known := themeSlots[slot]
		supported := slot == "conversation" || slot == "hero.new_session" || slot == "background.window" || slot == "background.sidebar" || slot == "background.content" || (known && strings.HasPrefix(slot, "banner."))
		if !supported || len(layers) == 0 || len(layers) > 4 {
			return fmt.Errorf("invalid theme effect target: %s", slot)
		}
		if (slot == "hero.new_session" || strings.HasPrefix(slot, "banner.")) && variant.Assets[slot] == nil {
			return fmt.Errorf("theme effect target requires artwork: %s", slot)
		}
		for _, effect := range layers {
			if effect.Count < 1 || effect.Count > 64 || !themeRange(effect.Opacity, 0, 1) || !themeRange(effect.Size, 0.5, 12) || !themeRange(effect.Duration, 3, 120) || ((effect.Kind == Pulse || effect.Kind == Scan) && effect.Count != 1) {
				return fmt.Errorf("invalid theme effect dimensions: %s", slot)
			}
			particles += effect.Count
		}
	}
	if particles > 192 {
		return errors.New("theme effects exceed particle limit")
	}
	return nil
}
