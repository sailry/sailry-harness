package live

import "os"

func openLocalAttachment(path string) (*os.File, error) { return os.Open(path) }
