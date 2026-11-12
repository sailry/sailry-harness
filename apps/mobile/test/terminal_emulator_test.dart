import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/terminal/emulator.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';

final _appearance = <String, dynamic>{
  'foreground': {'red': 10, 'green': 20, 'blue': 30},
  'background': {'red': 200, 'green': 210, 'blue': 220},
  'palette': [
    for (var index = 0; index < 16; index++)
      {'red': index, 'green': 100 + index, 'blue': 200 + index},
  ],
};

/// One delivery's screen, as the Client hands it to the page.
Map<String, dynamic> _delivery(
  List<int> bytes, {
  int columns = 10,
  int rows = 3,
}) => {
  'vt': {
    'viewport': {
      'columns': columns,
      'rows': rows,
      'pixel_width': 0,
      'pixel_height': 0,
    },
    'bytes': bytes,
  },
  'columns': columns,
  'scrollback': <dynamic>[],
  'rows': <dynamic>[],
  'cursor': null,
  'features': {'title': 'kept'},
};

Map<String, dynamic> _feed(
  TerminalEmulator emulator,
  String text, {
  bool reset = false,
  int columns = 10,
  int rows = 3,
}) {
  final screen = _delivery(utf8.encode(text), columns: columns, rows: rows);
  emulator.feed(screen['vt'] as Map<String, dynamic>, reset: reset);
  return emulator.screen(screen, _appearance);
}

List<Map<String, dynamic>> _lines(Map<String, dynamic> screen, String key) =>
    (screen[key] as List).cast<Map<String, dynamic>>();

List<Map<String, dynamic>> _spans(Map<String, dynamic> line) =>
    (line['spans'] as List).cast<Map<String, dynamic>>();

/// What each line shows, as a selection of the whole line copies it.
List<String> _text(Map<String, dynamic> screen, String key) => [
  for (final line in _lines(screen, key))
    terminalSelection([line], (row: 0, column: 0), (row: 0, column: 499)),
];

void main() {
  test('plain text becomes rows of spans', () {
    final screen = _feed(
      TerminalEmulator(),
      r'$ ls'
      '\r\nfile  x',
      reset: true,
    );
    expect(_text(screen, 'rows'), [r'$ ls', 'file  x', '']);
    expect(screen['scrollback'], isEmpty);
    expect(_spans(_lines(screen, 'rows')[0]), hasLength(1));
    final span = _spans(_lines(screen, 'rows')[0]).single;
    expect(span['column'], 0);
    expect(span['columns'], 4);
    expect(span['text'], r'$ ls');
    expect((span['style'] as Map)['foreground'], isNull);
    expect((span['style'] as Map)['underline'], 'none');
    // What the delivery carried besides its content stays; the bytes do not.
    expect(screen['columns'], 10);
    expect(screen['features'], {'title': 'kept'});
    expect(screen.containsKey('vt'), isFalse);
    expect(screen['foreground'], _appearance['foreground']);
    expect(screen['background'], _appearance['background']);
    expect(screen['cursor_color'], _appearance['foreground']);
    expect(screen['alternate'], isFalse);
  });

  test('colors and styles are carried in the forms a Node writes', () {
    // A Mirror writes colors with colons and each cell run after a reset.
    final screen = _feed(
      TerminalEmulator(),
      '\x1b[0;38:2::255:128:0mA'
      '\x1b[0;38:5:1;48:5:12mB'
      '\x1b[0;31;44mC'
      '\x1b[0;38:5:196mD'
      '\x1b[0;38:5:244mE'
      '\x1b[0;1;2;3;5;7;8;9;53mF'
      '\x1b[0;4:3;58:2::1:2:3mG'
      '\x1b[0;4mH'
      '\x1b[0mI',
      reset: true,
    );
    final spans = _spans(_lines(screen, 'rows')[0]);
    expect(
      [for (final span in spans) span['text']],
      ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I'],
    );
    expect(
      [for (final span in spans) span['column']],
      [for (var column = 0; column < 9; column++) column],
    );
    Map<String, dynamic> style(int index) =>
        spans[index]['style'] as Map<String, dynamic>;
    expect(style(0)['foreground'], {'red': 255, 'green': 128, 'blue': 0});
    expect(style(0)['background'], isNull);
    // The first 16 colors are the page's own.
    expect(style(1)['foreground'], {'red': 1, 'green': 101, 'blue': 201});
    expect(style(1)['background'], {'red': 12, 'green': 112, 'blue': 212});
    expect(style(2)['foreground'], {'red': 1, 'green': 101, 'blue': 201});
    expect(style(2)['background'], {'red': 4, 'green': 104, 'blue': 204});
    // The cube and the grays are fixed.
    expect(style(3)['foreground'], {'red': 255, 'green': 0, 'blue': 0});
    expect(style(4)['foreground'], {'red': 128, 'green': 128, 'blue': 128});
    for (final flag in [
      'bold',
      'faint',
      'italic',
      'blink',
      'inverse',
      'invisible',
      'strikethrough',
      'overline',
    ]) {
      expect(style(5)[flag], isTrue, reason: flag);
      expect(style(8)[flag], isFalse, reason: flag);
    }
    expect(style(6)['underline'], 'curly');
    expect(style(6)['underline_color'], {'red': 1, 'green': 2, 'blue': 3});
    expect(style(6)['background'], isNull);
    expect(style(7)['underline'], 'single');
    expect(style(8)['underline'], 'none');
  });

  test('a filled cell without text is a blank span', () {
    final screen = _feed(
      TerminalEmulator(),
      'a\x1b[3C\x1b[0;48:2::9:8:7m \x1b[0m\x1b[2;1H\x1b[44m\x1b[K\x1b[0m',
      reset: true,
    );
    final first = _spans(_lines(screen, 'rows')[0]);
    // Cells nothing was written to are left out.
    expect(first, hasLength(2));
    expect(first[1]['column'], 4);
    expect(first[1]['text'], ' ');
    expect((first[1]['style'] as Map)['background'], {
      'red': 9,
      'green': 8,
      'blue': 7,
    });
    final erased = _spans(_lines(screen, 'rows')[1]).single;
    expect(erased['column'], 0);
    expect(erased['columns'], 10);
    expect(erased['text'], ' ' * 10);
  });

  test('a wide character takes two columns', () {
    final screen = _feed(TerminalEmulator(), 'a中b é\u{1F600}', reset: true);
    final spans = _spans(_lines(screen, 'rows')[0]);
    expect(
      [
        for (final span in spans)
          (span['column'], span['columns'], span['text']),
      ],
      [
        (0, 1, 'a'),
        (1, 2, '中'),
        (3, 2, 'b '),
        (5, 1, 'é'),
        (6, 2, '\u{1F600}'),
      ],
    );
    expect(_text(screen, 'rows').first, 'a中b é\u{1F600}');
    final lines = terminalLines(screen);
    expect(
      terminalSelection(lines, (row: 0, column: 1), (row: 0, column: 3)),
      '中b',
    );
    // A wide character that does not fit goes whole to the next row.
    final wrapped = _feed(TerminalEmulator(), '123456789中z', reset: true);
    expect(_text(wrapped, 'rows'), ['123456789', '中z', '']);
    expect(_spans(_lines(wrapped, 'rows')[1]).first['column'], 0);
  });

  test('the cursor follows position, visibility and shape', () {
    final emulator = TerminalEmulator();
    var screen = _feed(emulator, 'ab\r\ncd', reset: true);
    expect(screen['cursor'], {
      'column': 2,
      'row': 1,
      'at_wide_tail': false,
      'blinking': true,
      'style': 'block',
    });
    screen = _feed(emulator, '\x1b[1;2H\x1b[6 q');
    expect(screen['cursor'], {
      'column': 1,
      'row': 0,
      'at_wide_tail': false,
      'blinking': false,
      'style': 'bar',
    });
    screen = _feed(emulator, '\x1b[3 q');
    expect((screen['cursor'] as Map)['style'], 'underline');
    expect((screen['cursor'] as Map)['blinking'], isTrue);
    expect(_feed(emulator, '\x1b[?25l')['cursor'], isNull);
    screen = _feed(emulator, '\x1b[?25h\x1b[0 q\x1b[3;1H中\x1b[3;2H');
    expect(screen['cursor'], {
      'column': 1,
      'row': 2,
      'at_wide_tail': true,
      'blinking': true,
      'style': 'block',
    });
    // The row the page draws the cursor on counts the scrollback.
    screen = _feed(emulator, '\r\n\r\nx');
    expect(_lines(screen, 'scrollback'), hasLength(2));
    expect((screen['cursor'] as Map)['row'], 2);
  });

  test('rows that scroll off grow the scrollback up to its limit', () {
    final emulator = TerminalEmulator();
    var screen = _feed(emulator, '1\r\n2\r\n3', reset: true);
    expect(screen['scrollback'], isEmpty);
    screen = _feed(emulator, '\r\n4\r\n5');
    expect(_text(screen, 'scrollback'), ['1', '2']);
    expect(_text(screen, 'rows'), ['3', '4', '5']);
    expect(terminalLines(screen), hasLength(5));
    final kept = _lines(screen, 'scrollback').first;
    screen = _feed(emulator, '\r\n6');
    expect(_text(screen, 'scrollback'), ['1', '2', '3']);
    expect(identical(_lines(screen, 'scrollback').first, kept), isTrue);
    // Clearing the scrollback leaves the screen.
    screen = _feed(emulator, '\x1b[3J');
    expect(screen['scrollback'], isEmpty);
    expect(_text(screen, 'rows'), ['4', '5', '6']);
    screen = _feed(
      emulator,
      [for (var row = 0; row < 1200; row++) '\r\n$row'].join(),
    );
    expect(
      _lines(screen, 'scrollback'),
      hasLength(TerminalEmulator.scrollbackLimit),
    );
    expect(_text(screen, 'rows'), ['1197', '1198', '1199']);
    expect(_text(screen, 'scrollback').last, '1196');
  });

  test('a soft wrap joins its rows', () {
    final screen = _feed(
      TerminalEmulator(),
      'abcdefghijKLMNOPQRSTuv\r\nend',
      reset: true,
    );
    expect(_text(screen, 'scrollback'), ['abcdefghij']);
    expect(_text(screen, 'rows'), ['KLMNOPQRST', 'uv', 'end']);
    expect(
      [for (final line in terminalLines(screen)) line['wrapped']],
      [true, true, false, false],
    );
    expect(
      terminalSelection(
        terminalLines(screen),
        (row: 0, column: 0),
        (row: 3, column: 9),
      ),
      'abcdefghijKLMNOPQRSTuv\nend',
    );
  });

  test('bytes are written once and in order', () {
    final emulator = TerminalEmulator();
    _feed(emulator, 'one ', reset: true);
    var screen = _feed(emulator, 'two');
    expect(_text(screen, 'rows').first, 'one two');
    // A delivery without bytes changes nothing.
    screen = _feed(emulator, '');
    expect(_text(screen, 'rows').first, 'one two');
    // A character and a sequence may end in the next delivery.
    final bytes = utf8.encode('\r\n中\x1b[31mr');
    for (final part in [
      bytes.sublist(0, 3),
      bytes.sublist(3, 7),
      bytes.sublist(7),
    ]) {
      final delivery = _delivery(part);
      emulator.feed(delivery['vt'] as Map<String, dynamic>, reset: false);
      screen = emulator.screen(delivery, _appearance);
    }
    expect(_text(screen, 'rows'), ['one two', '中r', '']);
    expect(
      (_spans(_lines(screen, 'rows')[1]).last['style'] as Map)['foreground'],
      {'red': 1, 'green': 101, 'blue': 201},
    );
  });

  test('a starting frame and another viewport start over', () {
    final emulator = TerminalEmulator();
    _feed(
      emulator,
      '1\r\n2\r\n3\r\n4\x1b[?1049h\x1b[?2004h\x1b[4 q',
      reset: true,
    );
    var screen = _feed(emulator, 'fresh', reset: true);
    expect(screen['scrollback'], isEmpty);
    expect(_text(screen, 'rows'), ['fresh', '', '']);
    expect(screen['alternate'], isFalse);
    expect(screen['bracketed_paste'], isFalse);
    expect((screen['cursor'] as Map)['style'], 'block');
    expect((screen['cursor'] as Map)['column'], 5);

    // Frames of another size were drawn for a terminal that starts over.
    screen = _feed(emulator, 'wide', columns: 20, rows: 4);
    expect(screen['columns'], 20);
    expect(_text(screen, 'rows'), ['wide', '', '', '']);
    screen = _feed(emulator, ' and more', columns: 20, rows: 4);
    expect(_text(screen, 'rows').first, 'wide and more');
    expect(
      _text(
        _feed(
          emulator,
          '12345678901234567890',
          reset: true,
          columns: 20,
          rows: 4,
        ),
        'rows',
      ).first,
      '12345678901234567890',
    );
  });

  test('the alternate screen hides and returns the main one', () {
    final emulator = TerminalEmulator();
    _feed(emulator, '1\r\n2\r\n3\r\n4', reset: true);
    var screen = _feed(emulator, '\x1b[?1049h\x1b[H\x1b[2Jfull');
    expect(screen['alternate'], isTrue);
    expect(screen['scrollback'], isEmpty);
    expect(_text(screen, 'rows'), ['full', '', '']);
    screen = _feed(emulator, '\r\na\r\nb\r\nc');
    expect(screen['scrollback'], isEmpty);
    expect(_text(screen, 'rows'), ['a', 'b', 'c']);
    screen = _feed(emulator, '\x1b[?1049l');
    expect(screen['alternate'], isFalse);
    expect(_text(screen, 'scrollback'), ['1']);
    expect(_text(screen, 'rows'), ['2', '3', '4']);
    expect(screen['cursor'], containsPair('row', 2));
  });

  test('what a Node frame carries besides cells draws nothing', () {
    final emulator = TerminalEmulator();
    // A starting frame: both screens' images go, then the rows with their
    // prompt marks, a link, an image, the modes, colors, title and cursor.
    var screen = _feed(
      emulator,
      '\x1b[?1049h\x1b_Ga=d,d=A,q=2\x1b\\'
      '\x1b[?1049l\x1b_Ga=d,d=A,q=2\x1b\\'
      '\x1b[0m\x1b[H\x1b[2J\x1b[3J\x1b]133;C\x07'
      '\x1b[H'
      '\x1b[0m\x1b[2K\x1b]133;A\x07\x1b[0;1m\$\x1b[0m ls\x1b[0m\x1b[K\x1b]133;D\x07\r\n'
      '\x1b[0m\x1b[2K\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\\x1b[0m\x1b[K\r\n'
      '\x1b[0m\x1b[2K\u{10EEEE}\u{10EEEE}x\x1b[0m\x1b[K'
      '\x1b_Ga=t,f=32,o=z,s=1,v=1,i=7,q=2,m=1;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\'
      '\x1b_Ga=p,U=1,i=7,p=1,c=2,r=1,z=0,q=2\x1b\\'
      '\x1b[1;1H\x1b_Ga=p,i=7,p=2,X=0,Y=0,x=0,y=0,w=1,h=1,c=1,r=1,z=0,C=1,q=2\x1b\\'
      '\x1b[?1l\x1b[?66l\x1b[?67l\x1b[?1035l\x1b[?1036l\x1b[?1039l\x1b[?1004l'
      '\x1b[?1007l\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1005l\x1b[?1015l\x1b[?1016l'
      '\x1b[?5l\x1b[?2004h\x1b[?2027h\x1b[?1006h\x1b[?1003h'
      '\x1b[=0;1u\x1b[>4;1m\x1b]22;text\x1b\\'
      '\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\\x1b]104\x1b\\'
      '\x1b]2;title\x1b\\\x1b]52;c;aGk=\x1b\\\x1b]52;c;?\x1b\\'
      '\x1b[0m\x1b[1;5H\x1b[?25h\x1b[0 q',
      reset: true,
    );
    expect(_text(screen, 'rows'), [r'$ ls', 'link', '  x']);
    expect(screen['scrollback'], isEmpty);
    expect(screen['bracketed_paste'], isTrue);
    expect(screen['mouse_tracking'], isTrue);
    expect(screen['cursor'], containsPair('column', 4));
    expect(screen['cursor'], containsPair('row', 0));
    final prompt = _spans(_lines(screen, 'rows')[0]);
    expect((prompt.first['style'] as Map)['bold'], isTrue);
    // The keyboard modes are not text styles.
    expect((prompt.last['style'] as Map)['bold'], isFalse);
    expect((prompt.last['style'] as Map)['underline'], 'none');
    final link = _spans(_lines(screen, 'rows')[1]).single;
    expect(link['hyperlink'], 'https://example.com');
    expect(prompt.first['hyperlink'], isNull);
    // An image's placeholder cells are left empty.
    expect(_spans(_lines(screen, 'rows')[2]).single['column'], 2);

    // A later frame redraws a row where it is and sets program colors.
    screen = _feed(
      emulator,
      '\x1b[2H\x1b[0m\x1b[2K\x1b[0;38:5:1mred\x1b[0m\x1b[K'
      '\x1b]10;rgb:01/02/03\x1b\\\x1b]11;rgb:04/05/06\x1b\\\x1b]12;rgb:07/08/09\x1b\\'
      '\x1b]104\x1b\\\x1b]4;1;rgb:0a/0b/0c\x1b\\'
      '\x1b[0m\x1b[2;4H\x1b[?25h\x1b[0 q',
    );
    expect(_text(screen, 'rows'), [r'$ ls', 'red', '  x']);
    expect(screen['foreground'], {'red': 1, 'green': 2, 'blue': 3});
    expect(screen['background'], {'red': 4, 'green': 5, 'blue': 6});
    expect(screen['cursor_color'], {'red': 7, 'green': 8, 'blue': 9});
    expect(
      (_spans(_lines(screen, 'rows')[1]).single['style'] as Map)['foreground'],
      {'red': 10, 'green': 11, 'blue': 12},
    );
  });

  test('a screen with parsed rows is not the emulator\'s', () {
    final parsed = <String, dynamic>{
      'columns': 10,
      'scrollback': <dynamic>[],
      'rows': [
        {
          'spans': [
            {'column': 0, 'columns': 2, 'text': 'ok'},
          ],
        },
      ],
    };
    // An emulator that read nothing returns the screen as it came.
    expect(
      identical(TerminalEmulator().screen(parsed, _appearance), parsed),
      isTrue,
    );
  });

  test('the page\'s colors recolor the rows already read', () {
    final emulator = TerminalEmulator();
    final delivery = _delivery(utf8.encode('\x1b[32mg\x1b[0m\r\n\r\n\r\n'));
    emulator.feed(delivery['vt'] as Map<String, dynamic>, reset: true);
    final first = emulator.screen(delivery, _appearance);
    expect(identical(emulator.screen(delivery, _appearance), first), isTrue);
    final second = emulator.screen(delivery, const {});
    expect(second['foreground'], {'red': 255, 'green': 255, 'blue': 255});
    expect(second['background'], {'red': 0, 'green': 0, 'blue': 0});
    expect(
      (_spans(_lines(second, 'scrollback').first).single['style']
          as Map)['foreground'],
      {'red': 0, 'green': 205, 'blue': 0},
    );
    expect(
      (_spans(_lines(first, 'scrollback').first).single['style']
          as Map)['foreground'],
      {'red': 2, 'green': 102, 'blue': 202},
    );
  });
}
