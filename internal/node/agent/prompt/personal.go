package prompt

import (
	"context"
	"errors"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const personalFile = "instructions.md"

// The Node owns its profile exclusively. A turn can capture preferences while
// a controller creates the file for the first time, before all bytes are written.
var personalFileMu sync.RWMutex

func readPersonalText(ctx context.Context, profile string) (*string, error) {
	personalFileMu.RLock()
	defer personalFileMu.RUnlock()
	files, err := core.OpenFiles(profile, core.RestrictedFiles)
	if err != nil {
		return nil, err
	}
	defer files.Close()
	return readInstruction(ctx, files, personalFile)
}

func ReadPersonalInstructions(ctx context.Context, profile string) (protocol.PersonalInstructions, error) {
	value, err := readPersonalText(ctx, profile)
	if err != nil || value == nil {
		return protocol.PersonalInstructions{}, err
	}
	rev := digest(*value)
	return protocol.PersonalInstructions{Text: *value, Revision: &rev}, nil
}

func SavePersonalInstructions(ctx context.Context, profile string, input protocol.CommandSavePersonalInstructionsPayload) (protocol.PersonalInstructions, error) {
	if len(input.Text) > protocol.MAX_FRAME_BYTES || !utf8.ValidString(input.Text) || strings.IndexByte(input.Text, 0) >= 0 {
		return protocol.PersonalInstructions{}, instructionFault("personal instructions must be UTF-8 text within the conversation history record capacity")
	}
	personalFileMu.Lock()
	defer personalFileMu.Unlock()
	files, err := core.OpenFiles(profile, core.RestrictedFiles)
	if err != nil {
		return protocol.PersonalInstructions{}, err
	}
	defer files.Close()
	target, err := files.PrepareFileTarget(ctx, personalFile, input.ExpectedRevision, protocol.MAX_FRAME_BYTES)
	if err != nil {
		var fault *protocol.Fault
		if input.ExpectedRevision != nil && errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
			return protocol.PersonalInstructions{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "personal instructions were removed since they were read"}
		}
		return protocol.PersonalInstructions{}, err
	}
	result, err := target.Write(strings.NewReader(input.Text), uint64(len(input.Text)), digest(input.Text))
	if err != nil {
		return protocol.PersonalInstructions{}, err
	}
	return protocol.PersonalInstructions{Text: input.Text, Revision: &result.Revision}, nil
}
