import 'dart:convert';
import 'dart:typed_data';

import 'package:xterm2_core/xterm2_core.dart';

import '../../runtime/json.dart';

/// Shows a terminal whose Node sends its screen as VT frames.
///
/// A frame's bytes bring a terminal that read every frame before to the
/// Node's screen, so they are written once and in order to an emulator kept
/// here; a frame that starts over, and one of another viewport size, begin a
/// new emulator. [screen] then turns the emulator's buffer into the
/// scrollback and row spans the grid paints and selects from, the shape a
/// Node that parses its own screen sends.
///
/// The Node leaves default colors to the terminal that shows the frames, so
/// they come from the page's appearance. Images are not drawn.
class TerminalEmulator {
  /// Rows kept above the viewport, the bound of a parsed screen's scrollback.
  static const scrollbackLimit = 1000;

  Terminal? _terminal;
  Sink<List<int>>? _input;
  int _columns = 0;
  int _rows = 0;
  Map<String, dynamic>? _screen;
  String? _colors;
  // A row above the viewport no longer changes, so its spans are kept.
  Expando<Map<String, dynamic>> _kept = Expando();

  /// Reads one delivery's `vt` member. Each delivery is fed once.
  void feed(Map<String, dynamic> vt, {required bool reset}) {
    final viewport = object(vt['viewport']);
    final columns = number(viewport['columns']).toInt().clamp(1, 500);
    final rows = number(viewport['rows']).toInt().clamp(1, 200);
    if (reset || _terminal == null || columns != _columns || rows != _rows) {
      final terminal = Terminal(maxLines: scrollbackLimit + rows)
        ..resize(columns, rows);
      _terminal = terminal;
      _columns = columns;
      _rows = rows;
      _kept = Expando();
      // A character's bytes may end in the next delivery.
      _input = const Utf8Decoder(
        allowMalformed: true,
      ).startChunkedConversion(_Writer(terminal));
    }
    final bytes = vt['bytes'];
    if (bytes is List && bytes.isNotEmpty) {
      _input!.add(Uint8List.fromList(bytes.cast<int>()));
    }
    _screen = null;
  }

  /// The delivered [source] screen with the emulator's rows, cursor and
  /// colors. [appearance] is what the page sends its Node: `foreground`,
  /// `background` and the 16-color `palette`.
  Map<String, dynamic> screen(
    Map<String, dynamic> source,
    Map<String, dynamic> appearance,
  ) {
    final terminal = _terminal;
    if (terminal == null) return source;
    final colors = '${terminal.colorRevision} ${jsonEncode(appearance)}';
    if (colors != _colors) {
      _colors = colors;
      _kept = Expando();
      _screen = null;
    }
    return _screen ??= _project(terminal, source, appearance);
  }

  Map<String, dynamic> _project(
    Terminal terminal,
    Map<String, dynamic> source,
    Map<String, dynamic> appearance,
  ) {
    final palette = _Palette(terminal, appearance);
    final buffer = terminal.buffer;
    final above = buffer.scrollBack;
    final height = buffer.height;
    Map<String, dynamic> line(int y) => {
      'spans': _spans(terminal, buffer.lines[y], y, palette),
      // The emulator marks the row a soft wrap continues onto.
      'wrapped': y + 1 < height && buffer.lines[y + 1].isWrapped,
    };
    final x = buffer.cursorX;
    final cursorLine = buffer.lines[above + buffer.cursorY];
    final type = terminal.applicationCursorType;
    return {
      for (final entry in source.entries)
        if (entry.key != 'vt') entry.key: entry.value,
      'columns': _columns,
      'scrollback': [
        for (var y = 0; y < above; y++) _kept[buffer.lines[y]] ??= line(y),
      ],
      'rows': [for (var y = above; y < height; y++) line(y)],
      'cursor': terminal.cursorVisibleMode
          ? {
              'column': x,
              'row': buffer.cursorY,
              'at_wide_tail': x > 0 && cursorLine.getWidth(x - 1) == 2,
              // A program that set no cursor leaves the terminal's own.
              'blinking': type == null || terminal.cursorBlinkMode,
              'style': switch (type) {
                TerminalCursorType.underline => 'underline',
                TerminalCursorType.verticalBar => 'bar',
                _ => 'block',
              },
            }
          : null,
      'foreground': palette.foreground,
      'background': palette.background,
      'cursor_color': palette.cursor,
      'bracketed_paste': terminal.bracketedPasteMode,
      'mouse_tracking': terminal.mouseMode != MouseMode.none,
      'alternate': terminal.isUsingAltBuffer,
    };
  }

  /// A row's cells as spans: ASCII cells of one style run together, any other
  /// character is one span of its own one or two columns, and cells nothing
  /// was written to are left out unless they carry a background.
  List<Map<String, dynamic>> _spans(
    Terminal terminal,
    BufferLine line,
    int y,
    _Palette palette,
  ) {
    final spans = <Map<String, dynamic>>[];
    final run = StringBuffer();
    var runColumn = 0;
    var runStyle = const _Style(0, 0, 0, 0, 0);
    void close() {
      if (run.isEmpty) return;
      spans.add(
        _span(
          terminal,
          y,
          runColumn,
          run.length,
          run.toString(),
          runStyle,
          palette,
        ),
      );
      run.clear();
    }

    final columns = _columns < line.length ? _columns : line.length;
    for (var x = 0; x < columns;) {
      var code = line.getCodePoint(x);
      // The placeholder of an image is not a character to draw.
      if (code == 0x10EEEE) code = 0;
      final style = code == 0
          ? _Style(0, line.getBackground(x), 0, 0, 0)
          : _Style(
              line.getForeground(x),
              line.getBackground(x),
              line.getUnderlineColor(x),
              line.getAttributes(x) & _Style.attributes,
              line.getHyperlinkId(x),
            );
      if (code == 0 && style.background == 0) {
        close();
        x++;
        continue;
      }
      final combining = code == 0 ? null : line.getCombiningCharacters(x);
      final wide = line.getWidth(x) == 2 && x + 1 < columns;
      if (code < 128 && combining == null && !wide) {
        if (run.isNotEmpty && style != runStyle) close();
        if (run.isEmpty) {
          runColumn = x;
          runStyle = style;
        }
        run.writeCharCode(code == 0 ? 0x20 : code);
        x++;
        continue;
      }
      close();
      spans.add(
        _span(
          terminal,
          y,
          x,
          wide ? 2 : 1,
          String.fromCharCode(code) + (combining ?? ''),
          style,
          palette,
        ),
      );
      x += wide ? 2 : 1;
    }
    close();
    return spans;
  }

  Map<String, dynamic> _span(
    Terminal terminal,
    int y,
    int column,
    int columns,
    String text,
    _Style style,
    _Palette palette,
  ) {
    final flags = style.flags;
    return {
      'column': column,
      'columns': columns,
      'text': text,
      'style': {
        'foreground': palette.resolve(style.foreground),
        'background': palette.resolve(style.background),
        'underline_color': palette.resolve(style.underline),
        'bold': flags & CellAttr.bold != 0,
        'faint': flags & CellAttr.faint != 0,
        'italic': flags & CellAttr.italic != 0,
        'blink': flags & CellAttr.blink != 0,
        'underline': flags & CellAttr.underline != 0
            ? 'single'
            : flags & CellAttr.doubleUnderline != 0
            ? 'double'
            : flags & CellAttr.undercurl != 0
            ? 'curly'
            : flags & CellAttr.dottedUnderline != 0
            ? 'dotted'
            : flags & CellAttr.dashedUnderline != 0
            ? 'dashed'
            : 'none',
        'inverse': flags & CellAttr.inverse != 0,
        'invisible': flags & CellAttr.invisible != 0,
        'strikethrough': flags & CellAttr.strikethrough != 0,
        'overline': flags & CellAttr.overline != 0,
      },
      'hyperlink': style.link == 0
          ? null
          : terminal.hyperlinkAt(CellOffset(column, y)),
    };
  }
}

class _Writer implements Sink<String> {
  _Writer(this.terminal);
  final Terminal terminal;
  @override
  void add(String data) => terminal.write(data);
  @override
  void close() {}
}

/// What makes two cells look alike.
class _Style {
  const _Style(
    this.foreground,
    this.background,
    this.underline,
    this.flags,
    this.link,
  );

  /// The attributes a span's style carries.
  static const attributes =
      CellAttr.bold |
      CellAttr.faint |
      CellAttr.italic |
      CellAttr.blink |
      CellAttr.inverse |
      CellAttr.invisible |
      CellAttr.strikethrough |
      CellAttr.overline |
      CellAttr.underlineMask;

  final int foreground;
  final int background;
  final int underline;
  final int flags;
  final int link;

  @override
  bool operator ==(Object other) =>
      other is _Style &&
      other.foreground == foreground &&
      other.background == background &&
      other.underline == underline &&
      other.flags == flags &&
      other.link == link;

  @override
  int get hashCode =>
      Object.hash(foreground, background, underline, flags, link);
}

/// The colors of a cell: those a program set (OSC 4, 10, 11, 12) over the
/// page's own, and the fixed cube and grays past the first 16.
class _Palette {
  _Palette(Terminal terminal, Map<String, dynamic> appearance)
    : _named = objects(appearance['palette']),
      _set = Map.fromEntries(terminal.indexedColorOverrides) {
    foreground =
        _rgb(terminal.foregroundColorOverride) ??
        _given(appearance['foreground']) ??
        _rgb(0xffffff)!;
    background =
        _rgb(terminal.backgroundColorOverride) ??
        _given(appearance['background']) ??
        _rgb(0)!;
    cursor = _rgb(terminal.cursorColorOverride) ?? foreground;
  }

  // xterm's 16 colors, for an appearance that names none.
  static const _standard = [
    0x000000,
    0xcd0000,
    0x00cd00,
    0xcdcd00,
    0x0000ee,
    0xcd00cd,
    0x00cdcd,
    0xe5e5e5,
    0x7f7f7f,
    0xff0000,
    0x00ff00,
    0xffff00,
    0x5c5cff,
    0xff00ff,
    0x00ffff,
    0xffffff,
  ];
  static const _levels = [0, 95, 135, 175, 215, 255];

  final List<Map<String, dynamic>> _named;
  final Map<int, int> _set;
  late final Map<String, dynamic> foreground;
  late final Map<String, dynamic> background;
  late final Map<String, dynamic> cursor;

  static Map<String, dynamic>? _rgb(int? value) => value == null
      ? null
      : {
          'red': value >> 16 & 0xff,
          'green': value >> 8 & 0xff,
          'blue': value & 0xff,
        };

  static Map<String, dynamic>? _given(Object? value) {
    final color = object(value);
    return color.isEmpty ? null : color;
  }

  /// A cell's color, null for the default one.
  Map<String, dynamic>? resolve(int color) {
    final value = color & CellColor.valueMask;
    switch (color & CellColor.typeMask) {
      case CellColor.rgb:
        return _rgb(value);
      case CellColor.named || CellColor.palette:
        final index = value & 0xff;
        final set = _set[index];
        if (set != null) return _rgb(set);
        if (index < 16) {
          return index < _named.length ? _named[index] : _rgb(_standard[index]);
        }
        if (index < 232) {
          final cube = index - 16;
          return {
            'red': _levels[cube ~/ 36],
            'green': _levels[cube ~/ 6 % 6],
            'blue': _levels[cube % 6],
          };
        }
        final gray = 8 + 10 * (index - 232);
        return {'red': gray, 'green': gray, 'blue': gray};
    }
    return null;
  }
}
