package node

import (
	"context"
	"errors"
	"strings"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/embedding"
	"github.com/sailry/sailry-harness/internal/localmodels/ocr"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// LocalModelCapabilities are the inference runtimes of this build for
// Options.LocalModels. Embedding and OCR run on the onnxruntime library
// packaged beside the executable and are unavailable without it.
func LocalModelCapabilities() localmodels.Capabilities {
	return localmodels.Capabilities{
		protocol.ModelsKindDecision: decisionCapability(),
		protocol.ModelsKindEmbedding: {
			Load: func(resource localmodels.Resource, _ bool) (localmodels.Engine, error) {
				engine, err := embedding.Load(resource.Definition, resource.Directory)
				if err != nil {
					return localmodels.Engine{}, err
				}
				return localmodels.Engine{Value: engine, Release: engine.Close}, nil
			},
			Available: onnx.Available,
			// An installation encodes one text with each prefix.
			Probe: func(_ context.Context, engine any) error {
				for _, input := range []embedding.Input{embedding.Query, embedding.Document} {
					if _, err := engine.(*embedding.Engine).Encode(input, []string{"Model compatibility check"}); err != nil {
						return err
					}
				}
				return nil
			},
		},
		protocol.ModelsKindOcr: {
			Load: func(resource localmodels.Resource, _ bool) (localmodels.Engine, error) {
				engine, err := ocr.Load(resource.Definition, resource.Directory)
				if err != nil {
					return localmodels.Engine{}, err
				}
				return localmodels.Engine{Value: engine, Release: engine.Close}, nil
			},
			Available: onnx.Available,
		},
	}
}

func ocrFailure(err error) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "OCR failed: " + err.Error()}
}

func ocrCancelled() error {
	return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "OCR operation was cancelled"}
}

func notConfigured(err error) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotConfigured
}

// ocrResource captures the OCR model the saved settings select: the chosen
// model, or with no choice the first installed model in catalog order and
// then any installed one. A chosen model that is not installed is not
// replaced by another. A model still being verified after startup is waited
// for rather than reported as not installed.
func (node *Node) ocrResource(ctx context.Context) (localmodels.Resource, error) {
	registry := node.localModels.registry
	settings := registry.Settings()
	choice := localmodels.Choice(&settings, protocol.ModelsKindOcr)
	if !choice.Enabled {
		return localmodels.Resource{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "OCR is disabled"}
	}
	if err := registry.Storage(); err != nil {
		return localmodels.Resource{}, err
	}
	var ids []string
	for _, model := range registry.Cached(protocol.ModelsKindOcr) {
		ids = append(ids, model.ID)
	}
	if err := registry.Settle(ctx, ids...); err != nil {
		return localmodels.Resource{}, err
	}
	installed := map[string]bool{}
	var first string
	for _, model := range registry.Cached(protocol.ModelsKindOcr) {
		if model.State.Kind == "installed" {
			installed[model.ID] = true
			if first == "" {
				first = model.ID
			}
		}
	}
	selected := first
	if choice.Model != nil {
		selected = ""
		if installed[*choice.Model] {
			selected = *choice.Model
		}
	} else {
		for _, definition := range localmodels.OCR() {
			if id := localmodels.ID(&definition); installed[id] {
				selected = id
				break
			}
		}
	}
	if selected == "" {
		return localmodels.Resource{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "an OCR model must be installed"}
	}
	return registry.Resource(protocol.ModelsKindOcr, selected, &choice)
}

// recognize reads the text of one image. Interactive work runs ahead of
// background inference. The scheduler loads the model on demand and
// releases it when idle.
func (node *Node) recognize(ctx context.Context, resource localmodels.Resource, image *ocr.RGB, lane localmodels.Lane) (string, error) {
	return localmodels.Execute(ctx, node.localModels.registry, resource, lane, func(ctx context.Context, engine any) (string, error) {
		lines, err := engine.(*ocr.Engine).Recognize(image, func() bool { return ctx.Err() != nil })
		if errors.Is(err, ocr.ErrCancelled) {
			return "", ocrCancelled()
		}
		if err != nil {
			return "", ocrFailure(err)
		}
		texts := make([]string, len(lines))
		for index, line := range lines {
			texts[index] = line.Text
		}
		return strings.Join(texts, "\n"), nil
	})
}

// imageParts reads an image file as one page of recognized text.
func (node *Node) imageParts(ctx context.Context, data []byte, lane localmodels.Lane) ([]office.Part, error) {
	image, err := ocr.Decode(data)
	if errors.Is(err, ocr.ErrImageSize) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: err.Error()}
	}
	if err != nil {
		return nil, ocrFailure(err)
	}
	resource, err := node.ocrResource(ctx)
	if err != nil {
		return nil, err
	}
	text, err := node.recognize(ctx, resource, image, lane)
	if err != nil {
		return nil, err
	}
	return []office.Part{{Name: "Image", Text: text, Position: protocol.KnowledgePosition{Kind: "page", Data: protocol.KnowledgePositionPagePayload{Page: 1}}}}, nil
}
