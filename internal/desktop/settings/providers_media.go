package settings

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// imageGeneration keeps native image models on the images endpoint. A
// conversation model generates images through the Responses hosted tool;
// enabling its Image capability must not send it to the images endpoint.
func (d *modelDraft) imageGeneration(api string) string {
	if d.catalog != nil && slices.Contains(d.catalog.Generates, viewmodel.GenerateImage) {
		return viewmodel.GenerateImage
	}
	if d.hosted || slices.Contains(d.model.Generates, viewmodel.GenerateImageTool) || api == "responses" && d.model.Tools {
		return viewmodel.GenerateImageTool
	}
	return viewmodel.GenerateImage
}

// refreshImageGeneration updates enabled image capabilities when the protocol
// changes. A remembered hosted tool stays explicit on unsupported protocols,
// rather than becoming a request to an unrelated images endpoint.
func (e *providerEditor) refreshImageGeneration() {
	api := e.current().API
	for i := range e.models {
		d := &e.models[i]
		if !slices.Contains(d.model.Generates, viewmodel.GenerateImage) && !slices.Contains(d.model.Generates, viewmodel.GenerateImageTool) {
			continue
		}
		generation := d.imageGeneration(api)
		d.hosted = generation == viewmodel.GenerateImageTool
		d.model.Generates = slices.DeleteFunc(d.model.Generates, func(value string) bool {
			return value == viewmodel.GenerateImage || value == viewmodel.GenerateImageTool
		})
		d.model.Generates = append(d.model.Generates, generation)
	}
}
