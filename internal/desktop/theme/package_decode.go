package theme

import (
	"encoding/json"
	"errors"
	"fmt"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func decodeThemeObject(data []byte, value any, required ...string) error {
	if !utf8.Valid(data) {
		return errors.New("theme manifest must be valid UTF-8")
	}
	return protocol.DecodeObject(data, value, required, true)
}

func (value *Package) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Version     int      `json:"version"`
		ID          string   `json:"id"`
		Name        string   `json:"name"`
		Description *string  `json:"description"`
		Release     *string  `json:"package_version"`
		Homepage    *string  `json:"homepage"`
		License     License  `json:"license"`
		Light       *Variant `json:"light"`
		Dark        *Variant `json:"dark"`
	}
	if err := decodeThemeObject(data, &decoded, "version", "id", "name", "license", "light", "dark"); err != nil {
		return err
	}
	for _, field := range []struct {
		value     *string
		limit     int
		multiline bool
		message   string
	}{
		{decoded.Description, 4096, true, "invalid theme package description"},
		{decoded.Release, 64, false, "invalid theme release version"},
	} {
		if field.value != nil && !themeText(*field.value, field.limit, field.multiline) {
			return errors.New(field.message)
		}
	}
	if decoded.Homepage != nil {
		if err := themeHomepage(*decoded.Homepage); err != nil {
			return err
		}
	}
	*value = Package{Version: decoded.Version, ID: decoded.ID, Name: decoded.Name, License: decoded.License, Light: decoded.Light, Dark: decoded.Dark}
	if decoded.Description != nil {
		value.Description = *decoded.Description
	}
	if decoded.Release != nil {
		value.Release = *decoded.Release
	}
	if decoded.Homepage != nil {
		value.Homepage = *decoded.Homepage
	}
	return nil
}

func (value *License) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Author string  `json:"author"`
		Name   *string `json:"name"`
		Text   string  `json:"text"`
	}
	if err := decodeThemeObject(data, &decoded, "author", "text"); err != nil {
		return err
	}
	if decoded.Name != nil && !themeText(*decoded.Name, 160, false) {
		return errors.New("invalid theme license name")
	}
	*value = License{Author: decoded.Author, Text: decoded.Text}
	if decoded.Name != nil {
		value.Name = *decoded.Name
	}
	return nil
}

func (value *Variant) UnmarshalJSON(data []byte) error {
	type plain Variant
	var decoded plain
	if err := decodeThemeObject(data, &decoded, "theme"); err != nil {
		return err
	}
	*value = Variant(decoded)
	return nil
}

func (value *Config) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Name           string                     `json:"name"`
		Mode           string                     `json:"mode"`
		IsDefault      bool                       `json:"is_default"`
		FontSize       *float32                   `json:"font.size"`
		FontFamily     *string                    `json:"font.family"`
		MonoFontSize   *float32                   `json:"mono_font.size"`
		MonoFontFamily *string                    `json:"mono_font.family"`
		Radius         *uint64                    `json:"radius"`
		RadiusLg       *uint64                    `json:"radius.lg"`
		Shadow         *bool                      `json:"shadow"`
		Highlight      json.RawMessage            `json:"highlight"`
		Colors         map[string]json.RawMessage `json:"colors"`
	}
	decoded.Mode = "light"
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	if fields == nil {
		return errors.New("theme must be an object")
	}
	for key, raw := range fields {
		switch key {
		case "name", "mode", "is_default", "font.size", "font.family", "mono_font.size", "mono_font.family", "radius", "radius.lg", "shadow", "colors":
		case "highlight":
			if string(raw) != "null" {
				return errors.New("unsupported theme field: highlight")
			}
		default:
			return fmt.Errorf("unsupported theme field: %s", key)
		}
	}
	if err := decodeThemeObject(data, &decoded); err != nil {
		return err
	}
	if decoded.Mode != "light" && decoded.Mode != "dark" {
		return fmt.Errorf("unknown theme mode: %s", decoded.Mode)
	}
	for key := range decoded.Colors {
		if _, known := themeColorKeys[key]; !known {
			return fmt.Errorf("unknown Kit color: %s", key)
		}
	}
	for _, size := range []*float32{decoded.FontSize, decoded.MonoFontSize} {
		if size != nil && !themeRange(*size, 8, 32) {
			return errors.New("invalid theme sizing")
		}
	}
	for _, radius := range []*uint64{decoded.Radius, decoded.RadiusLg} {
		if radius != nil && *radius > 32 {
			return errors.New("invalid theme sizing")
		}
	}
	var radius, radiusLg *float32
	if decoded.Radius != nil {
		value := float32(*decoded.Radius)
		radius = &value
	}
	if decoded.RadiusLg != nil {
		value := float32(*decoded.RadiusLg)
		radiusLg = &value
	}
	*value = Config{Name: decoded.Name, Mode: decoded.Mode, IsDefault: decoded.IsDefault, FontSize: decoded.FontSize, FontFamily: decoded.FontFamily, MonoFontSize: decoded.MonoFontSize, MonoFontFamily: decoded.MonoFontFamily, Radius: radius, RadiusLg: radiusLg, Shadow: decoded.Shadow, Highlight: decoded.Highlight, Colors: make(map[string]Color, len(decoded.Colors))}
	for key, raw := range decoded.Colors {
		var text *string
		if err := json.Unmarshal(raw, &text); err != nil {
			return err
		}
		if text == nil {
			continue
		}
		color, err := parseThemeBackground(*text)
		if err != nil {
			return err
		}
		value.Colors[key] = Color{V: color, OK: true}
	}
	return nil
}

func (value *Composer) UnmarshalJSON(data []byte) error {
	var decoded struct {
		BorderWidth uint8   `json:"border_width"`
		Send        *Action `json:"send"`
		Stop        *Action `json:"stop"`
	}
	if err := decodeThemeObject(data, &decoded); err != nil {
		return err
	}
	*value = Composer{BorderWidth: float32(decoded.BorderWidth), Send: decoded.Send, Stop: decoded.Stop}
	return nil
}
func (value *Action) UnmarshalJSON(data []byte) error {
	type plain Action
	var decoded plain
	if err := decodeThemeObject(data, &decoded, "background", "foreground", "hover", "active"); err != nil {
		return err
	}
	if !decoded.Background.OK || !decoded.Foreground.OK || !decoded.Hover.OK || !decoded.Active.OK {
		return errors.New("theme action colors must be strings")
	}
	*value = Action(decoded)
	return nil
}
func (value *Card) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Background  *Color `json:"background"`
		Foreground  *Color `json:"foreground"`
		Detail      *Color `json:"detail"`
		Icon        *Color `json:"icon"`
		Hover       *Color `json:"hover"`
		Active      *Color `json:"active"`
		Border      *Color `json:"border"`
		BorderWidth uint8  `json:"border_width"`
		Radius      *uint8 `json:"radius"`
	}
	if err := decodeThemeObject(data, &decoded); err != nil {
		return err
	}
	*value = Card{}
	if decoded.Background != nil {
		value.Background = *decoded.Background
	}
	if decoded.Foreground != nil {
		value.Foreground = *decoded.Foreground
	}
	if decoded.Detail != nil {
		value.Detail = *decoded.Detail
	}
	if decoded.Icon != nil {
		value.Icon = *decoded.Icon
	}
	if decoded.Hover != nil {
		value.Hover = *decoded.Hover
	}
	if decoded.Active != nil {
		value.Active = *decoded.Active
	}
	if decoded.Border != nil {
		value.Border = *decoded.Border
	}
	value.BorderWidth = float32(decoded.BorderWidth)
	if decoded.Radius != nil {
		radius := float32(*decoded.Radius)
		value.Radius = &radius
	}
	return nil
}

func (value *Message) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Background  *Color `json:"background"`
		Foreground  *Color `json:"foreground"`
		Border      *Color `json:"border"`
		BorderWidth *uint8 `json:"border_width"`
		Radius      *uint8 `json:"radius"`
	}
	if err := decodeThemeObject(data, &decoded); err != nil {
		return err
	}
	*value = Message{}
	if decoded.Background != nil {
		value.Background = *decoded.Background
	}
	if decoded.Foreground != nil {
		value.Foreground = *decoded.Foreground
	}
	if decoded.Border != nil {
		value.Border = *decoded.Border
	}
	if decoded.BorderWidth != nil {
		value.BorderWidth = float32(*decoded.BorderWidth)
	}
	if decoded.Radius != nil {
		radius := float32(*decoded.Radius)
		value.Radius = &radius
	}
	return nil
}

func (value *Wordmark) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Foreground *Color `json:"foreground"`
		Glow       *Color `json:"glow"`
	}
	if err := decodeThemeObject(data, &decoded); err != nil {
		return err
	}
	*value = Wordmark{}
	if decoded.Foreground != nil {
		value.Foreground = *decoded.Foreground
	}
	if decoded.Glow != nil {
		value.Glow = *decoded.Glow
	}
	return nil
}

func (value *Effect) UnmarshalJSON(data []byte) error {
	type plain Effect
	var decoded plain
	if err := decodeThemeObject(data, &decoded, "kind", "color", "count", "opacity", "size", "duration"); err != nil {
		return err
	}
	switch decoded.Kind {
	case Embers, Dust, Twinkle, Pulse, Petals, Scan:
	default:
		return fmt.Errorf("unknown theme effect kind: %s", decoded.Kind)
	}
	if !decoded.Color.OK {
		return errors.New("theme effect color must be a string")
	}
	*value = Effect(decoded)
	return nil
}
