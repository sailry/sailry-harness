package knowledge

import (
	"cmp"
	"slices"
)

// Ranked is one fused key with its reciprocal-rank score.
type Ranked struct {
	Key   string
	Score float32
}

// Fuse merges two ranked lists by reciprocal rank. A key counts once per
// list; equal scores order by key.
func Fuse(keyword, semantic []string) []Ranked {
	scores := map[string]float32{}
	for _, list := range [][]string{keyword, semantic} {
		seen := map[string]bool{}
		for rank, key := range list {
			if !seen[key] {
				seen[key] = true
				scores[key] += 1 / (1 + float32(rank) + 1)
			}
		}
	}
	ranked := make([]Ranked, 0, len(scores))
	for key, score := range scores {
		ranked = append(ranked, Ranked{key, score})
	}
	slices.SortFunc(ranked, func(left, right Ranked) int {
		return cmp.Or(cmp.Compare(right.Score, left.Score), cmp.Compare(left.Key, right.Key))
	})
	return ranked
}
