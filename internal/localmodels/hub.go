package localmodels

import (
	"context"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Search lists Hub models compatible with one kind. Pages without a
// compatible model are skipped, at most three per request.
func (hubs *Hubs) Search(ctx context.Context, search *protocol.ModelsSearch, mirror *string) (protocol.ModelsPage, error) {
	page := protocol.ModelsPage{Items: []protocol.ModelsItem{}}
	if err := ValidateSearch(search); err != nil {
		return page, err
	}
	hub, err := hubs.open(search.Hub, mirror)
	if err != nil {
		return page, err
	}
	request := *search
	for range 3 {
		var found listing
		if request.Hub == protocol.ModelsHubHuggingFace {
			found, err = huggingfaceSearch(ctx, hub, &request)
		} else {
			found, err = modelscopeSearch(ctx, hub, &request)
		}
		if err != nil {
			return page, err
		}
		page.Partial = page.Partial || found.partial
		items := make([]protocol.ModelsItem, len(found.items))
		failures := make([]error, len(found.items))
		// Four candidates are inspected at a time, in listing order.
		slots := make(chan struct{}, 4)
		var inspecting sync.WaitGroup
		for index, entry := range found.items {
			slots <- struct{}{}
			inspecting.Go(func() {
				defer func() { <-slots }()
				items[index], failures[index] = inspect(ctx, hub, request.Kind, request.Hub, entry)
			})
		}
		inspecting.Wait()
		for index, failure := range failures {
			switch code(failure) {
			case "":
				if failure == nil {
					page.Items = append(page.Items, items[index])
				} else {
					page.Partial = true
				}
			case protocol.ErrorCodeCancelled:
				return page, failure
			case protocol.ErrorCodeInvalidRequest, protocol.ErrorCodeNotFound, protocol.ErrorCodePermissionDenied, protocol.ErrorCodeNotConfigured:
			default:
				page.Partial = true
			}
		}
		if found.next != nil && request.Cursor != nil && *found.next == *request.Cursor {
			return page, invalid("model pagination cursor did not advance")
		}
		if len(page.Items) > 0 || found.next == nil {
			page.NextCursor = found.next
			return page, nil
		}
		request.Cursor = found.next
	}
	page.NextCursor = request.Cursor
	return page, nil
}

// Versions lists the branches and tags of one repository.
func (hubs *Hubs) Versions(ctx context.Context, origin protocol.ModelsHub, name string, mirror *string) ([]protocol.ModelsVersion, error) {
	if err := ValidateSource(protocol.ModelsSource{Hub: origin, Repository: name, Revision: "main"}); err != nil {
		return nil, err
	}
	hub, err := hubs.open(origin, mirror)
	if err != nil {
		return nil, err
	}
	if origin == protocol.ModelsHubHuggingFace {
		return huggingfaceVersions(ctx, hub, name)
	}
	return modelscopeVersions(ctx, hub, name)
}

// Resolve pins a repository revision into an installable definition.
func (hubs *Hubs) Resolve(ctx context.Context, kind protocol.ModelsKind, source protocol.ModelsSource, mirror *string) (*protocol.ModelsDefinition, error) {
	if err := ValidateSource(source); err != nil {
		return nil, err
	}
	hub, err := hubs.open(source.Hub, mirror)
	if err != nil {
		return nil, err
	}
	return resolveDefinition(ctx, hub, kind, source)
}

// RefreshOCR pins the latest revision of an OCR definition's files.
func (hubs *Hubs) RefreshOCR(ctx context.Context, previous *protocol.ModelsDefinition, mirror *string) (*protocol.ModelsDefinition, error) {
	if err := ValidateDefinition(previous); err != nil {
		return nil, err
	}
	if Kind(previous.Format) != protocol.ModelsKindOcr {
		return nil, invalid("OCR model format required")
	}
	hub, err := hubs.open(previous.Source.Hub, mirror)
	if err != nil {
		return nil, err
	}
	return ocrRefresh(ctx, hub, previous)
}
