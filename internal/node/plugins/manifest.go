package plugins

import (
	"encoding/json"
	"io/fs"
	"slices"
	"strings"
	"unicode"

	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const ManifestSchema = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
const MaxManifestBytes = 256 * 1024

type Manifest struct {
	Name        string
	Version     *string
	Description *string
	Homepage    *string
	Repository  *string
	License     *string
	Extension   *protocol.PluginExtension
	Issues      []protocol.PluginIssue
}

func packageFault(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func ValidateName(name string) bool {
	if len(name) == 0 || len(name) > 64 || strings.Contains(name, "--") || strings.Contains(name, "..") {
		return false
	}
	alphanumeric := func(value byte) bool {
		return value >= 'a' && value <= 'z' || value >= '0' && value <= '9'
	}
	if !alphanumeric(name[0]) || !alphanumeric(name[len(name)-1]) {
		return false
	}
	for _, value := range []byte(name) {
		if !alphanumeric(value) && value != '.' && value != '-' {
			return false
		}
	}
	return true
}

func resourcePath(name string) bool {
	return name != "." && fs.ValidPath(name) && !strings.ContainsAny(name, "\\:") && !strings.ContainsFunc(name, unicode.IsControl)
}

func namespacePath(name string) bool {
	return resourcePath(name) && strings.HasPrefix(name, "dev.sailry.platform/")
}

func ValidateManifest(data []byte) (Manifest, error) {
	var result Manifest
	if len(data) > MaxManifestBytes {
		return result, packageFault("plugin manifest exceeds its size limit")
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(data, &fields) != nil || fields == nil {
		return result, packageFault("plugin manifest must be a JSON object")
	}
	stringField := func(name string) (*string, error) {
		raw, exists := fields[name]
		if !exists {
			return nil, nil
		}
		var value string
		if string(raw) == "null" || json.Unmarshal(raw, &value) != nil {
			return nil, packageFault("plugin metadata must contain strings")
		}
		return &value, nil
	}
	schema, err := stringField("$schema")
	if err != nil || schema == nil || *schema != ManifestSchema {
		return result, packageFault("unsupported Agent Plugins manifest schema")
	}
	name, err := stringField("name")
	if err != nil || name == nil || !ValidateName(*name) {
		return result, packageFault("invalid plugin name")
	}
	result.Name = *name
	targets := map[string]**string{
		"version": &result.Version, "description": &result.Description,
		"homepage": &result.Homepage, "repository": &result.Repository, "license": &result.License,
	}
	for name, target := range targets {
		*target, err = stringField(name)
		if err != nil {
			return result, err
		}
	}
	if raw, exists := fields["author"]; exists {
		var author map[string]json.RawMessage
		if json.Unmarshal(raw, &author) != nil || author == nil {
			return result, packageFault("plugin author must be an object")
		}
		for name, raw := range author {
			var value string
			if !slices.Contains([]string{"name", "email", "url"}, name) || string(raw) == "null" || json.Unmarshal(raw, &value) != nil {
				return result, packageFault("invalid plugin author metadata")
			}
		}
	}
	if raw, exists := fields["keywords"]; exists {
		var values []string
		if string(raw) == "null" || json.Unmarshal(raw, &values) != nil {
			return result, packageFault("plugin keywords must contain strings")
		}
	}
	known := []string{"$schema", "name", "version", "description", "author", "homepage", "repository", "license", "keywords", "extensions"}
	for name := range fields {
		if !slices.Contains(known, name) {
			result.Issues = append(result.Issues, protocol.PluginIssue{Path: "plugin.json." + name, Kind: protocol.PluginIssueKindIgnoredManifestField})
		}
	}
	if raw, exists := fields["extensions"]; exists {
		var extensions map[string]json.RawMessage
		issuePath := "plugin.json.extensions"
		if json.Unmarshal(raw, &extensions) != nil || extensions == nil {
			result.Issues = append(result.Issues, protocol.PluginIssue{Path: issuePath, Kind: protocol.PluginIssueKindInvalidExtensions})
		} else if raw, exists := extensions["dev.sailry.platform"]; exists {
			issuePath += ".dev.sailry.platform"
			var extension protocol.PluginExtension
			if json.Unmarshal(raw, &extension) != nil {
				result.Issues = append(result.Issues, protocol.PluginIssue{Path: issuePath, Kind: protocol.PluginIssueKindInvalidExtensions})
			} else if extension.ApiVersion != "v1" {
				result.Issues = append(result.Issues, protocol.PluginIssue{Path: issuePath, Kind: protocol.PluginIssueKindUnsupportedExtension})
			} else if validateExtension(extension) != nil {
				result.Issues = append(result.Issues, protocol.PluginIssue{Path: issuePath, Kind: protocol.PluginIssueKindInvalidExtensions})
			} else {
				result.Extension = &extension
			}
		}
	}
	slices.SortFunc(result.Issues, func(left, right protocol.PluginIssue) int { return strings.Compare(left.Path, right.Path) })
	return result, nil
}

func validateExtension(extension protocol.PluginExtension) error {
	if len(extension.Actions) > 32 || len(extension.Connectors) > 8 || len(extension.Tools) > 256 {
		return packageFault("plugin extension exceeds declaration limits")
	}
	if extension.MinSailryVersion != nil {
		if _, err := semver.StrictNewVersion(*extension.MinSailryVersion); err != nil {
			return packageFault("invalid minimum Sailry version")
		}
	}
	if text := extension.Instructions; text != nil && (strings.TrimSpace(*text) == "" || len(*text) > 16*1024) {
		return packageFault("invalid plugin instructions")
	}
	actions := map[protocol.PluginAction]bool{}
	for _, action := range extension.Actions {
		if actions[action] {
			return packageFault("duplicate plugin action")
		}
		actions[action] = true
	}
	connectors := map[string]bool{}
	for _, connector := range extension.Connectors {
		if connector.ID == "" || connectors[connector.ID] {
			return packageFault("invalid plugin connector identity")
		}
		connectors[connector.ID] = true
	}
	if extension.SettingsSchema != nil && !namespacePath(*extension.SettingsSchema) {
		return packageFault("plugin settings must belong to the platform namespace")
	}
	if host := extension.Host; host != nil {
		if !namespacePath(host.Entry) || !slices.Contains(host.Resources, host.Entry) || len(host.Handlers) > 256 || len(host.Resources) > 256 {
			return packageFault("invalid plugin host resources")
		}
		seen := map[string]bool{}
		for _, handler := range host.Handlers {
			if handler == "" || len(handler) > 128 || seen[handler] || strings.ContainsRune(handler, 0) {
				return packageFault("invalid plugin host handler")
			}
			seen[handler] = true
		}
		for _, resource := range host.Resources {
			if !namespacePath(resource) {
				return packageFault("plugin host resources must belong to the platform namespace")
			}
		}
		for _, handler := range host.ReadOnly {
			if !seen[handler] {
				return packageFault("read-only handler is not declared")
			}
		}
	}
	if desktop := extension.Desktop; desktop != nil {
		for _, resource := range desktop.Resources {
			if !namespacePath(resource) {
				return packageFault("plugin desktop resources must belong to the platform namespace")
			}
		}
	}
	if err := validateDesktopExtension(extension); err != nil {
		return err
	}
	if err := validateStatusExtension(extension); err != nil {
		return err
	}
	tools := map[string]bool{}
	for _, tool := range extension.Tools {
		server := ""
		if tool.Server != nil {
			server = *tool.Server
		}
		identity := server + "\x00" + tool.Name
		if tool.Name == "" || len(tool.Name) > 128 || tools[identity] {
			return packageFault("invalid plugin tool identity")
		}
		tools[identity] = true
		if handler := tool.Handler; handler != nil {
			if extension.Host == nil || !slices.Contains(extension.Host.Handlers, handler.Name) {
				return packageFault("plugin tool handler is not declared")
			}
			if handler.Result != nil && !slices.Contains(extension.Host.Handlers, *handler.Result) {
				return packageFault("plugin result handler is not declared")
			}
		}
	}
	if err := ValidateCards(extension); err != nil {
		return err
	}
	if extension.Storage != nil {
		return ValidateStorage(*extension.Storage)
	}
	return nil
}

func Compatibility(required *string) protocol.PluginCompatibility {
	result := protocol.PluginCompatibility{Kind: "compatible"}
	if required == nil {
		return result
	}
	minimum, minimumErr := semver.StrictNewVersion(*required)
	current, currentErr := semver.StrictNewVersion(link.ProductVersion)
	if minimumErr != nil || currentErr != nil || current.Compare(minimum) < 0 {
		return protocol.PluginCompatibility{Kind: "requires_newer", Data: protocol.PluginCompatibilityRequiresNewerPayload{Required: *required}}
	}
	return result
}

func EnsureCompatible(compatibility protocol.PluginCompatibility) error {
	if compatibility.Kind == "compatible" {
		return nil
	}
	var requirement protocol.PluginCompatibilityRequiresNewerPayload
	data, err := json.Marshal(compatibility.Data)
	if err != nil || json.Unmarshal(data, &requirement) != nil {
		return packageFault("invalid plugin compatibility requirement")
	}
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin requires a newer Sailry Node",
		Details: &protocol.FaultDetails{Kind: "plugin_requires_newer", Data: protocol.FaultDetailsPluginRequiresNewerPayload{Required: requirement.Required}}}
}

func IsNewer(installed, candidate *string) bool {
	if installed == nil || candidate == nil {
		return false
	}
	before, beforeErr := semver.StrictNewVersion(*installed)
	after, afterErr := semver.StrictNewVersion(*candidate)
	return beforeErr == nil && afterErr == nil && after.Compare(before) > 0
}
