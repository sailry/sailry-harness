package process

import "github.com/sailry/sailry-harness/internal/node/process"

type Environment = process.Environment

func NewEnvironment(values []string) Environment { return process.NewEnvironment(values) }
