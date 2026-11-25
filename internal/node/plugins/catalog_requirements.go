package plugins

import (
	"encoding/json"
	"slices"

	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type catalogRuntimeRequirements struct {
	Products        []string `json:"products,omitempty"`
	RequiresModules []string `json:"requires_modules,omitempty"`
}

func readCatalogRequirements(data json.RawMessage) (map[string]catalogRuntimeRequirements, error) {
	if data == nil {
		return nil, nil
	}
	var records map[string]json.RawMessage
	if json.Unmarshal(data, &records) != nil || records == nil {
		return nil, packageFault("invalid catalog version requirements")
	}
	result := make(map[string]catalogRuntimeRequirements, len(records))
	for version, raw := range records {
		var requirements catalogRuntimeRequirements
		if !validPackageVersion(version) || protocol.DecodeObject(raw, &requirements, nil, true) != nil ||
			!validRuntimeRequirements(requirements.Products, requirements.RequiresModules) {
			return nil, packageFault("invalid catalog version requirements")
		}
		result[version] = requirements
	}
	return result, nil
}

func mergeCatalogRequirements(version *protocol.PluginCatalogVersion, requirements catalogRuntimeRequirements) error {
	if !validRuntimeRequirements(version.Products, version.RequiresModules) ||
		!validRuntimeRequirements(requirements.Products, requirements.RequiresModules) {
		return packageFault("invalid catalog version requirements")
	}
	if version.Products == nil {
		version.Products = slices.Clone(requirements.Products)
	} else if requirements.Products != nil {
		version.Products = slices.DeleteFunc(slices.Clone(version.Products), func(product string) bool {
			return !slices.Contains(requirements.Products, product)
		})
		if len(version.Products) == 0 {
			return packageFault("conflicting catalog product requirements")
		}
	}
	for _, module := range requirements.RequiresModules {
		if !slices.Contains(version.RequiresModules, module) {
			version.RequiresModules = append(version.RequiresModules, module)
		}
	}
	return nil
}

func applyCatalogRequirements(version *protocol.PluginCatalogVersion, requirements map[string]catalogRuntimeRequirements) error {
	if record, exists := requirements[version.Version]; exists {
		return mergeCatalogRequirements(version, record)
	}
	// Runtime-aware catalogs cover every release, including unrestricted history.
	// A legacy catalog cannot stand in for missing capability-era requirements.
	if requirements != nil || version.MinSailryVersion != nil && runtimeRequirementsMinimum(*version.MinSailryVersion) {
		return packageFault("missing catalog version requirements")
	}
	return nil
}

func runtimeRequirementsMinimum(minimum string) bool {
	version, err := semver.StrictNewVersion(minimum)
	return err == nil && version.Compare(semver.MustParse("0.2.14")) >= 0
}
