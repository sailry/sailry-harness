import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';

void main() {
  test('large previews keep a bounded Unicode prefix across text parts', () {
    final prefix = List.filled(510, '🙂').join();
    final tail = List.filled(100000, 'x').join();
    final reply = [
      {
        'parts': [
          {'kind': 'text', 'data': '  $prefix'},
          {'kind': 'text', 'data': '\n tail$tail'},
        ],
      },
    ];
    final preview = messagePreview(reply);
    expect(preview.runes.length, 512);
    expect(preview, '$prefix t');
    expect(
      messagePreview([
        {
          'parts': [
            {'kind': 'text', 'data': 'Part'},
            {'kind': 'text', 'data': 'one \n'},
            {'kind': 'text', 'data': '\t two  '},
          ],
        },
      ]),
      'Partone two',
    );
  });

  test('message text excludes tools and thinking', () {
    expect(
      messagePreview([
        {
          'parts': [
            {'kind': 'text', 'data': 'Previous'},
          ],
        },
        {
          'parts': [
            {'kind': 'text', 'data': 'Latest\nmessage 🙂'},
          ],
        },
        {
          'parts': [
            {'kind': 'thinking', 'data': 'Reasoning'},
          ],
        },
        {
          'parts': [
            {
              'kind': 'tool_result',
              'data': {'result': 'output'},
            },
          ],
        },
      ]),
      'Latest message 🙂',
    );
    expect(
      messagePreview([
        {
          'parts': [
            {
              'kind': 'attachment',
              'data': {
                'spec': {'name': 'photo.png'},
              },
            },
          ],
        },
      ]),
      'photo.png',
    );
  });
}
