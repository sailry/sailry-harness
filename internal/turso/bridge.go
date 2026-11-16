package turso

import (
	"context"
	"fmt"
	"unsafe"

	"github.com/ebitengine/purego"
	sdk "turso.tech/database/tursogo"
)

var bridge struct {
	open      func(string, *uintptr, *sdk.TursoConnection, *unsafe.Pointer) sdk.TursoStatusCode
	close     func(uintptr)
	interrupt func(sdk.TursoConnection)
	free      func(unsafe.Pointer)
}

func initializeBridge() {
	purego.RegisterLibFunc(&bridge.open, engineLibrary, "sailry_turso_open")
	purego.RegisterLibFunc(&bridge.close, engineLibrary, "sailry_turso_database_close")
	purego.RegisterLibFunc(&bridge.interrupt, engineLibrary, "sailry_turso_connection_interrupt")
	purego.RegisterLibFunc(&bridge.free, engineLibrary, "turso_str_deinit")
}

func openConnection(ctx context.Context, path string) (*connection, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var database uintptr
	var message unsafe.Pointer
	var pointer sdk.TursoConnection
	status := bridge.open(path, &database, &pointer, &message)
	if status != sdk.TURSO_OK {
		return nil, openError(status, message)
	}
	// The public constructor delegates every statement, binding and row to the
	// official SDK. Only native ownership and context interruption live here.
	base := sdk.NewConnection(pointer, nil)
	opened := &connection{base: base, database: database, pointer: pointer, rows: make(map[*rows]struct{})}
	if err := base.SetBusyTimeout(0); err != nil {
		_ = opened.Close()
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		_ = opened.Close()
		return nil, err
	}
	return opened, nil
}

func openError(status sdk.TursoStatusCode, pointer unsafe.Pointer) error {
	message := "native open failed"
	if pointer != nil {
		length := uintptr(0)
		for *(*byte)(unsafe.Add(pointer, length)) != 0 {
			length++
		}
		message = string(unsafe.Slice((*byte)(pointer), length))
		bridge.free(pointer)
	}
	kind := sdk.ErrTursoGeneric
	switch status {
	case sdk.TURSO_BUSY, 6:
		kind = sdk.ErrTursoBusy
	case sdk.TURSO_INTERRUPT:
		kind = sdk.ErrTursoInterrupt
	case sdk.TURSO_MISUSE:
		kind = sdk.ErrTursoMisuse
	case sdk.TURSO_CONSTRAINT:
		kind = sdk.ErrTursoConstraint
	case sdk.TURSO_READONLY:
		kind = sdk.ErrTursoReadOnly
	case sdk.TURSO_DATABASE_FULL:
		kind = sdk.ErrTursoDatabaseFull
	case sdk.TURSO_NOTADB:
		kind = sdk.ErrTursoNotADb
	case sdk.TURSO_CORRUPT:
		kind = sdk.ErrTursoCorrupt
	}
	return fmt.Errorf("%w: %s", kind, message)
}
