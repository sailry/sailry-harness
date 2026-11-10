// Package decision runs CPU-only local decisions through the packaged, pinned
// llama.cpp C API. There is no fallback runtime, model or GPU path.
package decision

import (
	"errors"
	"io/fs"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Request bounds of the decision protocol.
const (
	MaxQuestions         = 16
	MaxOptions           = 16
	MaxStateBytes        = 16 * 1024
	MaxInstructionsBytes = 1024
	MaxOptionBytes       = 256
)

// Kind classifies an Error.
type Kind uint8

const (
	// Invalid reports a rejected model file or request.
	Invalid Kind = iota
	// Unavailable reports that the pinned runtime cannot run on this machine.
	Unavailable
	// Native reports a failure inside the native runtime.
	Native
	// Cancelled reports that the caller stopped the request.
	Cancelled
	// IO reports a file that could not be read.
	IO
	// Library reports a native image that could not be loaded.
	Library
)

// Error is every failure this package returns.
type Error struct {
	Kind Kind
	// Message is the reason without the kind's prefix.
	Message string
}

func (e *Error) Error() string {
	switch e.Kind {
	case Invalid:
		return "decision input is invalid: " + e.Message
	case Unavailable:
		return "decision runtime is unavailable: " + e.Message
	case Native:
		return "decision inference failed: " + e.Message
	case Cancelled:
		return "decision inference was cancelled"
	case IO:
		return "decision file could not be read: " + e.Message
	}
	return "decision runtime could not load: " + e.Message
}

func invalid(message string) error     { return &Error{Kind: Invalid, Message: message} }
func unavailable(message string) error { return &Error{Kind: Unavailable, Message: message} }
func native(message string) error      { return &Error{Kind: Native, Message: message} }
func cancelled() error                 { return &Error{Kind: Cancelled} }

// failed reports a file system failure by its cause, without the path.
func failed(err error) error {
	var path *fs.PathError
	if errors.As(err, &path) {
		err = path.Err
	}
	return &Error{Kind: IO, Message: err.Error()}
}

// Available checks the platform and the exact packaged runtime without
// loading a model.
func Available() error {
	_, err := prepared()
	return err
}

func bounded(value string, limit int) bool {
	return len(value) <= limit && !strings.ContainsRune(value, 0)
}

// content reads a question decoded as a pointer or built as a value.
func content(question protocol.DecisionQuestion) (instructions string, binary map[string]string, choices map[string]*string, levels []string, known bool) {
	switch data := question.Data.(type) {
	case *protocol.DecisionQuestionNoulPayload:
		return content(protocol.DecisionQuestion{Kind: question.Kind, Data: *data})
	case *protocol.DecisionQuestionChoicePayload:
		return content(protocol.DecisionQuestion{Kind: question.Kind, Data: *data})
	case *protocol.DecisionQuestionScorePayload:
		return content(protocol.DecisionQuestion{Kind: question.Kind, Data: *data})
	case protocol.DecisionQuestionNoulPayload:
		return data.Instructions, data.Criteria, nil, nil, question.Kind == "noul"
	case protocol.DecisionQuestionChoicePayload:
		return data.Instructions, nil, data.Criteria, nil, question.Kind == "choice"
	case protocol.DecisionQuestionScorePayload:
		return data.Instructions, nil, nil, data.Criteria, question.Kind == "score"
	}
	return "", nil, nil, nil, false
}

// ValidateRequest checks the protocol bounds of a request. The model
// identity belongs to the registry and is not checked here.
func ValidateRequest(request *protocol.DecisionRequest) error {
	fault := func(message string) error {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
	}
	if !bounded(request.State, MaxStateBytes) || len(request.Questions) == 0 || len(request.Questions) > MaxQuestions {
		return fault("invalid decision request")
	}
	for id, question := range request.Questions {
		if id == "" || !bounded(id, 128) {
			return fault("invalid decision question identity")
		}
		instructions, binary, choices, levels, known := content(question)
		switch {
		case !known:
			return fault("invalid decision request")
		case question.Kind == "noul":
			for key, value := range binary {
				if key != "false" && key != "true" || !bounded(value, MaxOptionBytes) {
					return fault("invalid binary decision criteria")
				}
			}
		case question.Kind == "choice":
			if len(choices) == 0 || len(choices) > MaxOptions {
				return fault("invalid decision choices")
			}
			for key, value := range choices {
				if key == "" || !bounded(key, MaxOptionBytes) || value != nil && !bounded(*value, MaxOptionBytes) {
					return fault("invalid decision choices")
				}
			}
		default:
			if len(levels) < 2 || len(levels) > 10 {
				return fault("invalid decision score criteria")
			}
			for _, value := range levels {
				if !bounded(value, MaxOptionBytes) {
					return fault("invalid decision score criteria")
				}
			}
		}
		if strings.TrimSpace(instructions) == "" || !bounded(instructions, MaxInstructionsBytes) {
			return fault("invalid decision instructions")
		}
	}
	return nil
}
