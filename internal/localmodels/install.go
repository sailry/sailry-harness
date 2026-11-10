package localmodels

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Report receives the real phases of an installation. Progress accompanies
// only actual downloads.
type Report func(protocol.InstallationPhase, *protocol.InstallationProgress)

func reportAssets(report Report, progress assets.Progress) {
	if report == nil {
		return
	}
	switch progress.Phase {
	case assets.Inspecting:
		report(protocol.InstallationPhaseInspecting, nil)
	case assets.Validating:
		report(protocol.InstallationPhaseValidating, nil)
	case assets.Installing:
		report(protocol.InstallationPhaseInstalling, nil)
	case assets.Downloading:
		speed := uint64(progress.BytesPerSecond)
		report(protocol.InstallationPhaseDownloading, &protocol.InstallationProgress{Kind: "percent",
			Data: protocol.InstallationProgressPercentPayload{Percent: uint8(progress.Percent), BytesPerSecond: &speed}})
	}
}

func assetError(err error) error {
	switch {
	case errors.Is(err, assets.ErrCancelled):
		return Cancelled()
	case errors.Is(err, assets.ErrInvalidAsset):
		return invalid("local model asset validation failed")
	}
	return Failure(err)
}

// Install downloads, verifies and validates one registered model. An
// installed model returns at once. Cancelling ctx, Cancel, a changed asset
// source or Shutdown stop the transfer and keep its resumable checkpoint.
func (registry *Registry) Install(ctx context.Context, id string, report Report) error {
	if ctx.Err() != nil || registry.stop.Err() != nil {
		return Cancelled()
	}
	definition, err := registry.Definition(id)
	if err != nil {
		return err
	}
	kind := Kind(definition.Format)
	if err := registry.capabilities.Ensure(kind); err != nil {
		return err
	}
	root, err := registry.Root()
	if err != nil {
		return err
	}
	path, err := directory(root, id, definition)
	if err != nil {
		return err
	}
	if err := registry.verify(func(candidate string, _ *protocol.ModelsDefinition) bool { return candidate == id }); err != nil {
		return err
	}
	registry.mu.Lock()
	if registry.verified[id] != nil {
		registry.mu.Unlock()
		return nil
	}
	stop, cancel := context.WithCancel(registry.stop)
	registry.transfers[id] = cancel
	registry.publish()
	mirror := registry.settings.Mirror
	registry.mu.Unlock()
	// Publishes from the validated stamp; it never inspects files again.
	defer func() {
		registry.mu.Lock()
		delete(registry.transfers, id)
		cancel()
		registry.publish()
		registry.mu.Unlock()
	}()
	release := context.AfterFunc(ctx, cancel)
	defer release()
	err = registry.transfer(stop, id, definition, path, mirror, report)
	if err != nil && ctx.Err() != nil {
		return Cancelled()
	}
	return err
}

func (registry *Registry) transfer(stop context.Context, id string, definition *protocol.ModelsDefinition, path string, mirror *string, report Report) error {
	downloads := make([]assets.Download, 0, len(definition.Assets))
	for _, asset := range definition.Assets {
		address, err := assets.URL(asset, mirror)
		if err != nil {
			return assetError(err)
		}
		downloads = append(downloads, assets.Download{File: assets.File{Name: asset.Name, Size: asset.Bytes, SHA256: asset.Sha256}, URL: address})
	}
	if err := assets.Prepare(stop, registry.client, path, downloads, func(progress assets.Progress) { reportAssets(report, progress) }); err != nil {
		return assetError(err)
	}
	if report != nil {
		report(protocol.InstallationPhaseValidating, nil)
	}
	candidate := assets.Candidate(path, files(definition))
	if candidate == nil {
		return invalid("model assets changed before validation")
	}
	resource := Resource{ID: id, Definition: definition, Directory: path, Stamp: candidate, Scope: stop}
	check := func(bool) error { return registry.installing(resource) }
	probe := registry.capabilities[Kind(definition.Format)].Probe
	marked, err := registry.scheduler.request(stop, resource, Background, check, true, func(ctx context.Context, engine any) (any, error) {
		if ctx.Err() != nil {
			return nil, Cancelled()
		}
		if probe != nil {
			if err := probe(ctx, engine); err != nil {
				return nil, fault(err)
			}
			if ctx.Err() != nil {
				return nil, Cancelled()
			}
		}
		return mark(path, definition, candidate)
	})
	if err != nil {
		return err
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if stop.Err() != nil || registry.removing[id] {
		return Cancelled()
	}
	registry.verified[id] = marked.(*assets.Stamp)
	return nil
}

// installing rejects a validation whose installation ended or whose model
// changed.
func (registry *Registry) installing(resource Resource) error {
	if registry.stop.Err() != nil || resource.Scope.Err() != nil {
		return Cancelled()
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if registry.transfers[resource.ID] == nil || registry.removing[resource.ID] || registry.definitions[resource.ID] != resource.Definition {
		return conflict()
	}
	return nil
}

// Remove unloads one model and deletes its files.
func (registry *Registry) Remove(id string) error {
	definition, err := registry.Definition(id)
	if err != nil {
		return err
	}
	registry.mu.Lock()
	registry.removing[id] = true
	// Clearing the stamp invalidates queued captures before files are deleted.
	delete(registry.verified, id)
	registry.publish()
	registry.mu.Unlock()
	removed := false
	defer func() {
		registry.mu.Lock()
		delete(registry.removing, id)
		registry.publish()
		registry.mu.Unlock()
		if !removed {
			// Files may remain after a failed removal; reinspect them off the caller.
			registry.spawn(func() { _, _ = registry.List("") })
		}
	}()
	if err := registry.scheduler.unload(id); err != nil {
		return err
	}
	root, err := registry.Root()
	if err != nil {
		return err
	}
	if err := remove(root, id, definition); err != nil {
		return err
	}
	removed = true
	return nil
}

// fault keeps protocol faults and reports anything else as a runtime failure.
func fault(err error) error {
	if _, ok := err.(*protocol.Fault); ok || err == nil {
		return err
	}
	return Failure(err)
}
