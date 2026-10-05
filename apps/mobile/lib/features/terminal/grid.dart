import 'dart:math' as math;
import 'package:flutter/material.dart';

import '../../runtime/json.dart';
import 'cursor.dart';

typedef CellPosition = ({int row, int column});

List<Map<String, dynamic>> terminalLines(Map<String, dynamic> screen) => [
  ...objects(screen['scrollback']),
  ...objects(screen['rows']),
];

String terminalSelection(
  List<Map<String, dynamic>> lines,
  CellPosition from,
  CellPosition to,
) {
  var start = from;
  var end = to;
  if (start.row > end.row ||
      start.row == end.row && start.column > end.column) {
    start = to;
    end = from;
  }
  final result = StringBuffer();
  for (var row = start.row; row <= end.row && row < lines.length; row++) {
    if (row < 0) continue;
    final low = row == start.row ? start.column : 0;
    final high = row == end.row ? end.column + 1 : 500;
    final line = StringBuffer();
    var previous = low;
    for (final span in objects(lines[row]['spans'])) {
      final column = number(span['column']).toInt();
      final count = number(span['columns']).toInt();
      if (column + count <= low || column >= high) continue;
      final begin = math.max(column, low);
      if (begin > previous) line.write(' ' * (begin - previous));
      final value = text(span['text']);
      // The Node only coalesces ASCII cells. A Unicode span is one complete
      // grapheme with its authoritative one- or two-cell width.
      if (value.codeUnits.every((unit) => unit < 128) &&
          value.length == count) {
        line.write(
          value.substring(
            begin - column,
            math.min(column + count, high) - column,
          ),
        );
      } else {
        line.write(value);
      }
      previous = math.min(column + count, high);
    }
    result.write(line.toString().trimRight());
    if (row < end.row && lines[row]['wrapped'] != true) result.write('\n');
  }
  return result.toString();
}

Color terminalColor(Object? value, Color fallback) {
  final color = object(value);
  if (color.isEmpty) return fallback;
  return Color.fromARGB(
    255,
    number(color['red']).toInt(),
    number(color['green']).toInt(),
    number(color['blue']).toInt(),
  );
}

/// Flutter has no VT grid widget. The execution Node supplies parsed spans;
/// this painter only positions cells, styles, cursor and the local selection.
class TerminalGridPainter extends CustomPainter {
  TerminalGridPainter({
    required this.screen,
    required this.cell,
    required this.fontSize,
    this.anchor,
    this.extent,
    required this.selectionColor,
    this.focused = false,
    this.blink,
  }) : super(repaint: blink);
  final Map<String, dynamic> screen;
  final Size cell;
  final double fontSize;
  final CellPosition? anchor;
  final CellPosition? extent;
  final Color selectionColor;
  final bool focused;
  final CursorBlink? blink;

  bool get cursorVisible =>
      object(screen['cursor']).isNotEmpty &&
      (!focused ||
          object(screen['cursor'])['blinking'] != true ||
          (blink?.value ?? true));

  @override
  void paint(Canvas canvas, Size size) {
    final lines = terminalLines(screen);
    final foreground = terminalColor(screen['foreground'], Colors.white);
    final background = terminalColor(screen['background'], Colors.black);
    canvas.drawRect(Offset.zero & size, Paint()..color = background);
    final clip = canvas.getLocalClipBounds();
    final first = (clip.top / cell.height).floor().clamp(0, lines.length);
    final last = (clip.bottom / cell.height).ceil().clamp(0, lines.length);
    for (var row = first; row < last; row++) {
      for (final span in objects(lines[row]['spans'])) {
        final style = object(span['style']);
        var ink = terminalColor(style['foreground'], foreground);
        var fill = terminalColor(style['background'], background);
        if (style['inverse'] == true) {
          final previous = ink;
          ink = fill;
          fill = previous;
        }
        final x = number(span['column']) * cell.width;
        final bounds = Rect.fromLTWH(
          x,
          row * cell.height,
          number(span['columns']) * cell.width,
          cell.height,
        );
        canvas.drawRect(bounds, Paint()..color = fill);
        if (style['invisible'] == true) continue;
        final painter = TextPainter(
          text: TextSpan(
            text: text(span['text']),
            style: TextStyle(
              fontFamily: 'monospace',
              fontSize: fontSize,
              height: 1,
              color: style['faint'] == true ? ink.withValues(alpha: .5) : ink,
              fontWeight: style['bold'] == true
                  ? FontWeight.bold
                  : FontWeight.normal,
              fontStyle: style['italic'] == true
                  ? FontStyle.italic
                  : FontStyle.normal,
              decoration: TextDecoration.combine([
                if (style['underline'] != null && style['underline'] != 'none')
                  TextDecoration.underline,
                if (style['strikethrough'] == true) TextDecoration.lineThrough,
                if (style['overline'] == true) TextDecoration.overline,
              ]),
              decorationColor: terminalColor(style['underline_color'], ink),
            ),
          ),
          textDirection: TextDirection.ltr,
        )..layout();
        canvas.save();
        canvas.clipRect(bounds);
        canvas.translate(
          bounds.left,
          bounds.top + (cell.height - painter.height) / 2,
        );
        // Fallback fonts can give a one-cell symbol a wider advance. Fit its
        // complete glyph into the Node's columns instead of clipping it in half.
        if (painter.width > bounds.width) {
          canvas.scale(bounds.width / painter.width, 1);
        }
        painter.paint(canvas, Offset.zero);
        canvas.restore();
        painter.dispose();
      }
    }
    var start = anchor;
    var end = extent;
    if (start != null && end != null) {
      if (start.row > end.row ||
          start.row == end.row && start.column > end.column) {
        final previous = start;
        start = end;
        end = previous;
      }
      for (
        var row = math.max(first, start.row);
        row <= end.row && row < last;
        row++
      ) {
        final left = row == start.row ? start.column * cell.width : 0.0;
        final right = row == end.row
            ? (end.column + 1) * cell.width
            : size.width;
        canvas.drawRect(
          Rect.fromLTRB(
            left,
            row * cell.height,
            right,
            (row + 1) * cell.height,
          ),
          Paint()..color = selectionColor,
        );
      }
    }
    final cursor = object(screen['cursor']);
    if (cursorVisible) {
      final row =
          objects(screen['scrollback']).length + number(cursor['row']).toInt();
      final column = number(cursor['column']).toInt();
      final style = text(cursor['style']);
      var bounds = Rect.fromLTWH(
        column * cell.width,
        row * cell.height,
        cell.width,
        cell.height,
      );
      if (focused && style == 'bar') {
        bounds = Rect.fromLTWH(bounds.left, bounds.top, 2, bounds.height);
      }
      if (focused && style == 'underline') {
        bounds = Rect.fromLTWH(bounds.left, bounds.bottom - 2, bounds.width, 2);
      }
      final paint = Paint()
        ..color = terminalColor(
          screen['cursor_color'],
          foreground,
        ).withValues(alpha: .55);
      if (!focused || style == 'block_hollow') {
        paint.style = PaintingStyle.stroke;
      }
      canvas.drawRect(bounds, paint);
    }
  }

  @override
  bool shouldRepaint(TerminalGridPainter old) =>
      old.screen != screen ||
      old.cell != cell ||
      old.fontSize != fontSize ||
      old.focused != focused ||
      old.blink != blink ||
      old.anchor != anchor ||
      old.extent != extent ||
      old.selectionColor != selectionColor;
}
