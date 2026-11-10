package protocol

import (
	"encoding/json"
	"reflect"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"
)

const PluginDesktopMaxFiles = 128
const PluginDesktopMaxBytes = 256 * 1024
const PluginDesktopMaxImageBytes = 8 * 1024 * 1024

func ValidatePluginDesktopBundle(bundle PluginDesktopBundle) error {
	if !desktopScriptPath(bundle.Entry) {
		return desktopInvalid("invalid plugin desktop entry")
	}
	if _, exists := bundle.Files[bundle.Entry]; !exists {
		return desktopInvalid("plugin desktop entry is unavailable")
	}
	names := make([]string, 0, len(bundle.Files)+len(bundle.Images))
	remaining := PluginDesktopMaxBytes
	for name, content := range bundle.Files {
		if strings.HasSuffix(name, ".png") || !utf8.ValidString(content) || len(content) > remaining {
			return desktopInvalid("invalid plugin desktop text resources")
		}
		remaining -= len(content)
		names = append(names, name)
	}
	remaining = PluginDesktopMaxImageBytes
	for name, content := range bundle.Images {
		if !strings.HasSuffix(name, ".png") || content == "" || len(content)%4 != 0 || len(content) > remaining {
			return desktopInvalid("invalid plugin desktop image resources")
		}
		for _, character := range []byte(content) {
			if !desktopAlphanumeric(character) && character != '+' && character != '/' && character != '=' {
				return desktopInvalid("invalid plugin desktop image encoding")
			}
		}
		remaining -= len(content)
		names = append(names, name)
	}
	return validateDesktopPaths(names)
}

func ValidatePluginDesktopManifest(manifest PluginDesktopManifest) error {
	for _, entry := range []*string{manifest.Entry, manifest.UiEntry} {
		if entry != nil && (!desktopScriptPath(*entry) || !slices.Contains(manifest.Resources, *entry)) {
			return desktopInvalid("plugin desktop entry is not declared")
		}
	}
	options := manifest.NavigationOptions
	if options.Surface.Kind != "workspace" && options.Surface.Kind != "settings" ||
		options.Surface.Kind == "settings" && (manifest.Navigation == nil || options.Target != PluginDesktopNavigationTargetNode || options.Details) ||
		(options.Details || options.Target != PluginDesktopNavigationTargetNode) && (manifest.Entry == nil || manifest.Navigation == nil) {
		return desktopInvalid("invalid plugin desktop navigation options")
	}
	if manifest.Navigation != nil && (!manifest.Navigation.Valid() || manifest.Navigation.Icon == nil) ||
		manifest.Panel != nil && !manifest.Panel.Valid() ||
		(manifest.UiOverlay || manifest.UiShared) && manifest.UiEntry == nil {
		return desktopInvalid("invalid plugin desktop navigation")
	}
	if len(manifest.Previews) > 32 {
		return desktopInvalid("too many plugin desktop preview types")
	}
	for index, mime := range manifest.Previews {
		kind, subtype, found := strings.Cut(mime, "/")
		if manifest.Entry == nil || len(mime) > 128 || !found || kind == "" || subtype == "" ||
			slices.Contains(manifest.Previews[:index], mime) {
			return desktopInvalid("invalid plugin desktop preview type")
		}
		for _, character := range []byte(mime) {
			if !(character >= 'a' && character <= 'z' || character >= '0' && character <= '9' || strings.ContainsRune("/.-+", rune(character))) {
				return desktopInvalid("invalid plugin desktop preview type")
			}
		}
	}
	for index, renderer := range manifest.Renderers {
		if manifest.Entry == nil || slices.ContainsFunc(manifest.Renderers[:index], func(previous PluginDesktopRenderer) bool {
			return previous.Resource == renderer.Resource
		}) {
			return desktopInvalid("invalid plugin desktop resource renderer")
		}
		if renderer.Shortcut != nil {
			value := *renderer.Shortcut
			if strings.TrimSpace(value) == "" || len(value) > 128 || len(strings.Fields(value)) > 2 ||
				strings.ContainsFunc(value, func(character rune) bool { return character < ' ' || character > '~' }) {
				return desktopInvalid("invalid plugin desktop renderer shortcut")
			}
		}
		if renderer.Resource == PluginDesktopResourceKindTerminal {
			if renderer.Create == nil || !desktopIdentifier(*renderer.Create, 128) {
				return desktopInvalid("invalid plugin desktop terminal creation action")
			}
		} else if renderer.Create != nil {
			return desktopInvalid("plugin desktop renderer cannot declare a creation action")
		}
	}
	if err := validateDesktopConversations(manifest.Conversations); err != nil {
		return err
	}
	if manifest.Entry == nil {
		if len(manifest.Resources) == 0 {
			return nil
		}
		if manifest.Navigation != nil || manifest.Panel != nil || len(manifest.Conversations) != 0 {
			return desktopInvalid("plugin desktop surface requires an entry")
		}
	}
	return validateDesktopPaths(manifest.Resources)
}

func validateDesktopPaths(paths []string) error {
	if len(paths) == 0 || len(paths) > PluginDesktopMaxFiles {
		return desktopInvalid("invalid plugin desktop resource count")
	}
	names := make(map[string]bool, len(paths))
	for _, name := range paths {
		folded := strings.ToLower(name)
		if len(name) > 512 || strings.ContainsFunc(name, unicode.IsControl) ||
			strings.ContainsAny(name, "\\:<>\"|?*") || names[folded] {
			return desktopInvalid("invalid plugin desktop resource path")
		}
		names[folded] = true
		parts := strings.Split(name, "/")
		if len(parts) > 32 {
			return desktopInvalid("plugin desktop resource path is too deep")
		}
		for _, part := range parts {
			if part == "" || strings.HasSuffix(part, ".") || strings.HasSuffix(part, " ") {
				return desktopInvalid("invalid plugin desktop resource segment")
			}
			stem, _, _ := strings.Cut(part, ".")
			stem = strings.ToUpper(strings.TrimRight(stem, " "))
			if slices.Contains([]string{"CON", "PRN", "AUX", "NUL"}, stem) {
				return desktopInvalid("plugin desktop resource uses a reserved name")
			}
			for _, prefix := range []string{"COM", "LPT"} {
				suffix, found := strings.CutPrefix(stem, prefix)
				if found && slices.Contains([]string{"1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"}, suffix) {
					return desktopInvalid("plugin desktop resource uses a reserved name")
				}
			}
		}
	}
	return nil
}

func validateDesktopConversations(declarations []PluginConversationDeclaration) error {
	if len(declarations) > 16 {
		return desktopInvalid("too many plugin assistant declarations")
	}
	for index, declaration := range declarations {
		if !desktopIdentifier(declaration.ID, 64) || len(declaration.Context) > 16*1024 ||
			strings.ContainsRune(declaration.Context, 0) || len(declaration.Tools) > 128 ||
			slices.ContainsFunc(declarations[:index], func(previous PluginConversationDeclaration) bool { return previous.ID == declaration.ID }) {
			return desktopInvalid("invalid plugin assistant declaration")
		}
		for position, tool := range declaration.Tools {
			if slices.ContainsFunc(declaration.Tools[:position], func(previous PluginConversationTool) bool { return reflect.DeepEqual(previous, tool) }) {
				return desktopInvalid("duplicate plugin assistant tool")
			}
			var fields struct {
				Name    string  `json:"name"`
				Package string  `json:"package"`
				Server  *string `json:"server"`
			}
			body, err := json.Marshal(tool.Data)
			if err != nil || json.Unmarshal(body, &fields) != nil || !desktopToolName(fields.Name) ||
				fields.Server != nil && !desktopToolName(*fields.Server) {
				return desktopInvalid("invalid plugin assistant tool")
			}
			if tool.Kind == "package" && !desktopPackageName(fields.Package) {
				return desktopInvalid("invalid plugin assistant tool package")
			}
		}
	}
	return nil
}

func desktopScriptPath(value string) bool {
	return strings.HasSuffix(value, ".js") || strings.HasSuffix(value, ".mjs")
}

func desktopIdentifier(value string, limit int) bool {
	if value == "" || len(value) > limit {
		return false
	}
	for _, character := range []byte(value) {
		if !desktopAlphanumeric(character) && !strings.ContainsRune("._-", rune(character)) {
			return false
		}
	}
	return true
}

func desktopAlphanumeric(character byte) bool {
	return character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9'
}

func desktopPackageName(value string) bool {
	if value == "" || len(value) > 64 || strings.Contains(value, "--") || strings.Contains(value, "..") ||
		!desktopAlphanumeric(value[0]) || !desktopAlphanumeric(value[len(value)-1]) {
		return false
	}
	for _, character := range []byte(value) {
		if !(character >= 'a' && character <= 'z' || character >= '0' && character <= '9' || character == '-' || character == '.') {
			return false
		}
	}
	return true
}

func desktopToolName(value string) bool {
	return strings.TrimSpace(value) != "" && len(value) <= 256 && !strings.ContainsFunc(value, unicode.IsControl)
}

func desktopInvalid(message string) error {
	return &Fault{Code: ErrorCodeInvalidRequest, Message: message}
}
