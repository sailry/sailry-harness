//go:build !unix

package theme

import "os"

const themeReadFlags = os.O_RDONLY
