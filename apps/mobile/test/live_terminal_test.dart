import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/terminal/appearance.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';

void main() {
  test('selection preserves cell widths and wrapped graphemes', () {
    final lines = <Map<String, dynamic>>[
      {
        'wrapped': true,
        'spans': [
          {'column': 0, 'columns': 2, 'text': 'ab'},
          {'column': 2, 'columns': 2, 'text': '中'},
        ],
      },
      {
        'wrapped': false,
        'spans': [
          {'column': 0, 'columns': 1, 'text': 'e\u0301'},
          {'column': 1, 'columns': 2, 'text': '😀'},
        ],
      },
      {
        'spans': [
          {'column': 0, 'columns': 3, 'text': 'end'},
        ],
      },
    ];
    expect(
      terminalSelection(lines, (row: 0, column: 1), (row: 2, column: 2)),
      'b中e\u0301😀\nend',
    );
    expect(
      terminalSelection(lines, (row: 1, column: 2), (row: 0, column: 3)),
      '中e\u0301😀',
    );
    expect(terminalViewport(const Size(415, 207), const Size(9, 21)), {
      'columns': 46,
      'rows': 9,
      'pixel_width': 414,
      'pixel_height': 189,
    });
    expect(terminalViewport(const Size(99999, 99999), const Size(9, 21)), {
      'columns': 500,
      'rows': 200,
      'pixel_width': 4500,
      'pixel_height': 4200,
    });
    for (final (glyph, columns) in [('➜', 1), ('中', 2), ('🙂', 2)]) {
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
    }
  });
}
