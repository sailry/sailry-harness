package icons

import (
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	ailogo "github.com/yldm-tech/ai-logo/mygo"
)

// Brand marks come from the AI Logo collection (github.com/yldm-tech/ai-logo),
// which embeds every vector variant and parses a mark when it is first asked
// for. Marks are trademarks of their owners.

// brandSlugs are the preset catalogue's identifiers (a brand, or a preset of
// one) whose AI Logo slug is another word. An identifier that is the slug
// needs no entry.
var brandSlugs = map[string]string{
	"chat-gpt":             "openai",
	"chat-gpt-oauth":       "openai",
	"compatible-responses": "openai",
	"compatible-chat":      "openai",
	"claude-oauth":         "claude",
	"compatible-anthropic": "anthropic",
	"compatible-gemini":    "gemini",
	"copilot":              "githubcopilot",
	"copilot-responses":    "githubcopilot",
	"azure-openai":         "azure",
	"azure-ai":             "azureai",
	"opencode-go":          "opencode",
	"opencode-zen":         "opencode",
}

// BrandSlug is the AI Logo slug of a catalogue identifier: the identifier
// without its source ("partner:", "package:<name>:"), lower-cased, or the
// slug brandSlugs gives it.
func BrandSlug(id string) string {
	id = strings.ToLower(id[strings.LastIndex(id, ":")+1:])
	if slug, mapped := brandSlugs[id]; mapped {
		return slug
	}
	return id
}

// logos are the marks asked for so far by archive name, nil for one the
// collection does not have, so a miss is searched for once.
var logos sync.Map

func logo(slug string, variant ailogo.Variant) *ui.SVG {
	key := slug + "/" + string(variant)
	if found, cached := logos.Load(key); cached {
		return found.(*ui.SVG)
	}
	mark, err := ailogo.Parse(slug, variant)
	if err != nil {
		mark = nil
	}
	logos.Store(key, mark)
	return mark
}

// declared are the marks packages declare for their own brands, by slug.
var declared sync.Map

// Declare gives a brand the mark its package declares. It shows only where
// the collection has no mark for that brand.
func Declare(id string, mark *ui.SVG) {
	if id != "" && mark != nil {
		declared.Store(BrandSlug(id), mark)
	}
}

// Brand is the mark of a catalogue identifier for ui.Icon, which draws it in
// the text color: the collection's, else the one its package declared; nil
// when the brand has neither.
func Brand(id string) *ui.SVG {
	if id == "" {
		return nil
	}
	if mark := logo(BrandSlug(id), ailogo.Mono); mark != nil {
		return mark
	}
	if mark, found := declared.Load(BrandSlug(id)); found {
		return mark.(*ui.SVG)
	}
	return nil
}

// BrandColor is the mark in the brand's own colors for ui.Image, which
// draws a mark without them in the text color; nil when the brand has no
// mark.
func BrandColor(id string) *ui.SVG {
	if id == "" {
		return nil
	}
	if mark := logo(BrandSlug(id), ailogo.Color); mark != nil {
		return mark
	}
	return Brand(id)
}

// logoVariants are the suffixes a plugin icon name takes after its slug,
// the longer first.
var logoVariants = []struct {
	suffix  string
	variant ailogo.Variant
}{
	{"-brand-color", ailogo.BrandColor}, {"-text-color", ailogo.TextColor}, {"-text-cn", ailogo.TextCn},
	{"-color", ailogo.Color}, {"-brand", ailogo.Brand}, {"-text", ailogo.Text},
}

// pluginLogo resolves the name after "ailogo:": a slug, or a slug with a
// variant suffix. colored reports a variant in the brand's own colors.
func pluginLogo(name string) (mark *ui.SVG, colored bool) {
	name = strings.ToLower(name)
	for _, v := range logoVariants {
		if slug, found := strings.CutSuffix(name, v.suffix); found {
			if mark := logo(slug, v.variant); mark != nil {
				return mark, strings.HasSuffix(v.suffix, "-color")
			}
		}
	}
	return logo(name, ailogo.Mono), false
}

// PluginBrand is the brand mark an "ailogo:" icon name shows on a package's
// tile: the variant it names, or for one without the brand's own colors
// the mark in them where the collection has it. colored reports a mark
// ui.Image draws in its colors; ui.Icon draws any other in the text color.
// It is nil for another kind of name and for a brand the collection lacks.
func PluginBrand(name string) (mark *ui.SVG, colored bool) {
	name, found := strings.CutPrefix(name, "ailogo:")
	if !found {
		return nil, false
	}
	if mark, colored = pluginLogo(name); mark != nil && !colored {
		if own := logo(strings.ToLower(name), ailogo.Color); own != nil {
			return own, true
		}
	}
	return mark, colored
}

// Colored reports whether a plugin icon name is a brand mark in its own
// colors, which ui.Image draws; ui.Icon would draw its silhouette.
func Colored(name string) bool {
	name, found := strings.CutPrefix(name, "ailogo:")
	if !found {
		return false
	}
	mark, colored := pluginLogo(name)
	return mark != nil && colored
}
