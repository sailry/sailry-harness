package store

import (
	"database/sql"
	"errors"
	"math"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPythonSettingsRevision(test *testing.T) {
	test.Parallel()
	_, _, storage := schemaFixture(test)
	ctx := test.Context()
	index := "https://pypi.org/simple"
	var output protocol.Output
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		current, err := ReadPythonSettings(ctx, db)
		if err != nil || current.Revision != 0 || current.Index != nil {
			return errors.New("Python settings did not start at the source defaults")
		}
		var changes []protocol.Event
		output, changes, err = SavePythonSettings(ctx, db, protocol.PythonSettings{Revision: 0, Index: &index})
		if err == nil && (len(changes) != 1 || changes[0].Kind != "python_settings_changed") {
			return errors.New("Python settings change was not published through the existing event path")
		}
		return err
	}); err != nil {
		test.Fatal(err)
	}
	info, ok := output.Data.(protocol.PythonInfo)
	if !ok || info.Version != python.Version || info.Settings.Revision != 1 || *info.Settings.Index != index {
		test.Fatal("Python settings output lost the bundled version or persisted revision")
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := SavePythonSettings(ctx, db, protocol.PythonSettings{Revision: 0})
		return err
	}); !pythonSettingsFault(err, protocol.ErrorCodeRevisionConflict) {
		test.Fatalf("stale Python settings save=%v", err)
	}
	invalidIndex := "https://user:password@pypi.org/simple"
	for _, invalid := range []protocol.PythonSettings{{Revision: math.MaxUint64}, {Revision: 1, Index: &invalidIndex}} {
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, _, err := SavePythonSettings(ctx, db, invalid)
			return err
		}); !pythonSettingsFault(err, protocol.ErrorCodeInvalidRequest) {
			test.Fatalf("invalid Python settings save=%v", err)
		}
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := ReadPythonSettings(ctx, db)
		if err == nil && (current.Revision != 1 || current.Index == nil || *current.Index != index) {
			return errors.New("rejected save changed committed Python settings")
		}
		return err
	}); err != nil {
		test.Fatal(err)
	}
}

func pythonSettingsFault(err error, code protocol.ErrorCode) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == code
}
