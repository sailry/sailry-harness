import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';

class GridCanvas implements Canvas {
  final Rect viewport;
  GridCanvas(this.viewport);

  Offset _offset = Offset.zero;
  Size _scale = const Size(1, 1);
  final _saved = <(Offset, Size)>[];
  final paragraphs = <Rect>[];
  final clips = <Rect>[];
  final fills = <Rect>[];
  final scales = <Size>[];

  @override
  Rect getLocalClipBounds() => viewport;

  @override
  void save() => _saved.add((_offset, _scale));

  @override
  void restore() {
    final state = _saved.removeLast();
    _offset = state.$1;
    _scale = state.$2;
  }

  @override
  void translate(double dx, double dy) {
    _offset += Offset(dx * _scale.width, dy * _scale.height);
  }

  @override
  void scale(double sx, [double? sy]) {
    scales.add(Size(sx, sy ?? sx));
    _scale = Size(_scale.width * sx, _scale.height * (sy ?? sx));
  }

  @override
  void clipRect(
    Rect rect, {
    ui.ClipOp clipOp = ui.ClipOp.intersect,
    bool doAntiAlias = true,
  }) => clips.add(rect);

  @override
  void drawRect(Rect rect, Paint paint) => fills.add(rect);

  @override
  void drawParagraph(ui.Paragraph paragraph, Offset offset) {
    paragraphs.add(
      Rect.fromLTWH(
        _offset.dx + offset.dx * _scale.width,
        _offset.dy + offset.dy * _scale.height,
        paragraph.longestLine * _scale.width,
        paragraph.height * _scale.height,
      ),
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void gridTests() {
  group('glyph fitting', () {
    for (final (label, glyph, columns) in [
      ('arrow', '➜', 1),
      ('wide text', '中', 2),
      ('emoji', '🙂', 2),
    ]) {
      test('fits $label without moving adjacent text', () {
        const cell = Size(4, 24);
        const size = Size(80, 24);
        final canvas = GridCanvas(Offset.zero & size);
        TerminalGridPainter(
          screen: {
            'rows': [
              {
                'spans': [
                  {'column': 1, 'columns': columns, 'text': glyph},
                  {'column': 1 + columns, 'columns': 3, 'text': '933'},
                ],
              },
            ],
          },
          cell: cell,
          fontSize: 16,
          selectionColor: Colors.transparent,
        ).paint(canvas, size);

        expect(canvas.paragraphs, hasLength(2));
        final symbol = canvas.paragraphs.first;
        expect(symbol.left, cell.width);
        expect(symbol.width, closeTo(columns * cell.width, .001));
        expect(symbol.right, closeTo(canvas.clips.first.right, .001));
        expect(canvas.scales.first.width, lessThan(1));
        expect(canvas.scales.every((scale) => scale.height == 1), isTrue);
        expect(canvas.paragraphs.last.left, (1 + columns) * cell.width);
        expect(canvas.fills[1], canvas.clips.first);
        expect(canvas.fills[2], canvas.clips.last);
        expect(canvas._saved, isEmpty);
        expect(
          terminalSelection(
            terminalLines({
              'rows': [
                {
                  'spans': [
                    {'column': 0, 'columns': columns, 'text': glyph},
                  ],
                },
              ],
            }),
            (row: 0, column: 0),
            (row: 0, column: columns - 1),
          ),
          glyph,
        );
      });
    }

    test('preserves text that already fits', () {
      const size = Size(300, 24);
      final canvas = GridCanvas(Offset.zero & size);
      TerminalGridPainter(
        screen: {
          'rows': [
            {
              'spans': [
                {'column': 2, 'columns': 3, 'text': '933'},
              ],
            },
          ],
        },
        cell: const Size(24, 24),
        fontSize: 16,
        selectionColor: Colors.transparent,
      ).paint(canvas, size);
      expect(canvas.scales, isEmpty);
      expect(canvas.paragraphs.single.left, 48);
      expect(canvas.paragraphs.single.width, lessThan(72));
    });
  });
}
