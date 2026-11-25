package codeindex

import (
	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
	"github.com/sailry/sailry-harness/sdk/native/protocol"
)

type Watch = codeindexapi.Watch

type Service = codeindexapi.Service

const ReaderWaitMessage = codeindexapi.ReaderWaitMessage

func ReaderWaitExpired(f *protocol.Fault) bool { return codeindexapi.ReaderWaitExpired(f) }
