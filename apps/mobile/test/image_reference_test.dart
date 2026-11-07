import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/image_reference.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';

void main() {
  group('image reference', () {
    test('reads the tool call and original image position', () {
      expect(imageReference(Uri.parse('sailry-image:call_1/0')), (
        call: 'call_1',
        index: 0,
      ));
      expect(imageReference(Uri.parse('SAILRY-IMAGE:call%202/12')), (
        call: 'call 2',
        index: 12,
      ));
      expect(imageReference(Uri.parse('sailry-image:a%2Fb/3')), (
        call: 'a/b',
        index: 3,
      ));
    });

    test('ignores other image addresses', () {
      for (final value in [
        'images/call_1/0',
        'https://example.com/call_1/0',
        'sailry-image:call_1',
        'sailry-image:/0',
        'sailry-image:call_1/',
        'sailry-image:call_1/01',
        'sailry-image:call_1/-1',
        'sailry-image:%FF/0',
        'sailry-images:call_1/0',
        'sailry-image://host/call_1/0',
      ]) {
        expect(imageReference(Uri.parse(value)), isNull, reason: value);
      }
    });
  });

  test('transcript index returns Client-resolved images only', () {
    final image = {'entry': 'capture', 'part': 0, 'index': 0};
    final index = TranscriptIndex({
      'images': [
        {'call': 'call_1', 'index': 0, 'image': image},
      ],
    });
    expect(index.image((call: 'call_1', index: 0)), image);
    expect(index.image((call: 'call_1', index: 1)), isNull);
    expect(TranscriptIndex({}).image((call: 'call_1', index: 0)), isNull);
  });
}
