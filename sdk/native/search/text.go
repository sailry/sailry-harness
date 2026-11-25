package search

import "github.com/sailry/sailry-harness/internal/node/knowledge"

func Field(text string) string { return knowledge.Field(text) }

func Index(text string) string { return knowledge.Index(text) }

func Query(text string) string { return knowledge.Query(text) }

func Phrase(text string) string { return knowledge.Phrase(text) }
