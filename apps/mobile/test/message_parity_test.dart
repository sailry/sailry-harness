import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/diff.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';

void main() {
  test('only rewind exposes the recovery copy returned by Node', () {
    const backup = {'id': 'backup', 'title': 'Original conversation'};
    expect(
      historyBackup('rewind_conversation', {
        'data': {'backup': backup},
      }),
      backup,
    );
    expect(historyBackup('rewind_conversation', {'data': {}}), isNull);
    expect(
      historyBackup('replace_turn', {
        'data': {
          'history': {'backup': backup},
        },
      }),
      isNull,
    );
    expect(historyBackup('fork_conversation', {'data': backup}), isNull);
  });

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
