//go:build !unix

package telemetry

import "os"

const operationReadFlags = os.O_RDONLY
