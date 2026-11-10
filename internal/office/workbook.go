package office

import (
	"bytes"
	"encoding/xml"
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/xuri/excelize/v2"
)

type workbookCell struct {
	name        string
	column, row int
}

func workbookParts(pkg documentPackage, data []byte) ([]Part, error) {
	rels, err := relationships(pkg, "xl/_rels/workbook.xml.rels")
	if err != nil {
		return nil, err
	}
	byID := map[string]string{}
	cells := map[string][]workbookCell{}
	for _, rel := range rels {
		if !strings.HasSuffix(rel.Type, "/worksheet") {
			continue
		}
		name, err := target("xl", rel.Target)
		if err != nil {
			return nil, err
		}
		byID[rel.ID] = name
		text, err := pkg.xml(name)
		if err != nil {
			return nil, err
		}
		row := 0
		seen := map[string]bool{}
		err = elements(text, func(start xml.StartElement) error {
			if start.Name.Local == "row" {
				value := ""
				for _, attr := range start.Attr {
					if attr.Name.Local == "r" {
						value = attr.Value
					}
				}
				number, err := strconv.Atoi(value)
				if err != nil || number <= row || number > 1048576 {
					return invalid("invalid worksheet row coordinate")
				}
				row = number
			}
			if start.Name.Local == "c" {
				ref := ""
				for _, attr := range start.Attr {
					if attr.Name.Local == "r" {
						ref = attr.Value
					}
				}
				column, r, err := excelize.CellNameToCoordinates(ref)
				if err != nil || column < 1 || column > 16384 || r != row || seen[ref] {
					return invalid("invalid worksheet cell coordinate")
				}
				seen[ref] = true
				cells[name] = append(cells[name], workbookCell{ref, column, r})
			}
			return nil
		})
		if err != nil {
			return nil, err
		}
		slices.SortFunc(cells[name], func(a, b workbookCell) int {
			if a.row != b.row {
				return a.row - b.row
			}
			return a.column - b.column
		})
	}
	file, err := excelize.OpenReader(bytes.NewReader(data), excelize.Options{RawCellValue: true, UnzipSizeLimit: 128 << 20, UnzipXMLSizeLimit: 128 << 20})
	if err != nil {
		return nil, invalid(err)
	}
	defer file.Close()
	workbook, err := pkg.xml("xl/workbook.xml")
	if err != nil {
		return nil, err
	}
	type sheet struct{ name, path string }
	var sheets []sheet
	err = elements(workbook, func(start xml.StartElement) error {
		if start.Name.Local != "sheet" {
			return nil
		}
		name, id := "", ""
		for _, attr := range start.Attr {
			if attr.Name.Local == "name" {
				name = attr.Value
			}
			if attr.Name.Local == "id" && strings.HasSuffix(attr.Name.Space, "/relationships") {
				id = attr.Value
			}
		}
		if path, ok := byID[id]; ok {
			sheets = append(sheets, sheet{name, path})
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	out := make([]Part, 0, len(sheets))
	for _, sheet := range sheets {
		var text strings.Builder
		left, top, right, bottom := 16385, 1048577, 0, 0
		for _, cell := range cells[sheet.path] {
			value, err := file.GetCellValue(sheet.name, cell.name, excelize.Options{RawCellValue: true})
			if err != nil {
				return nil, invalid(err)
			}
			formula, err := file.GetCellFormula(sheet.name, cell.name)
			if err != nil {
				return nil, invalid(err)
			}
			left = min(left, cell.column)
			right = max(right, cell.column)
			top = min(top, cell.row)
			bottom = max(bottom, cell.row)
			fmt.Fprintf(&text, "%s: %s", cell.name, value)
			if formula != "" {
				fmt.Fprintf(&text, " [=%s]", formula)
			}
			text.WriteByte('\n')
		}
		if right == 0 {
			left, top, right, bottom = 1, 1, 1, 1
		}
		first, _ := excelize.CoordinatesToCellName(left, top)
		last, _ := excelize.CoordinatesToCellName(right, bottom)
		out = append(out, Part{Name: sheet.name, Text: text.String(), Position: protocol.KnowledgePosition{Kind: "cells", Data: protocol.KnowledgePositionCellsPayload{Sheet: sheet.name, Start: first, End: last}}})
	}
	return out, nil
}
