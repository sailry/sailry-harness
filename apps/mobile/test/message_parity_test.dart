import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/diff.dart';

void main() {
  test('added diff preserves original text and coordinates', () {
    final lines = parseLines('Original 中文 🙂\nsecond\n', added: true);
    expect(
      lines.map((line) => [line.text, line.before, line.after, line.kind]),
      [
        ['Original 中文 🙂', null, 1, 'added'],
        ['second', null, 2, 'added'],
      ],
    );
    expect(parseLines('', added: true), isEmpty);
  });
}
