import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';

void main() {
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
