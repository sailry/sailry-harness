import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/session_links.dart';

void main() {
  const child = '0b7c3e1a-5d2f-4c6b-9a8e-1f2d3c4b5a69';
  const other = '6f1e2d3c-4b5a-4968-8776-5a4b3c2d1e0f';
  final reference = {'index': 0, 'target': 'captured-target'};
  final view = {
    'children': {
      'sessions': {child: reference},
    },
  };
  String? parse(String link) => switch (link) {
    'sailry://session/child' => child,
    'sailry://session/other' => other,
    _ => null,
  };

  test('opens Client child references', () {
    final linked = linkedSession('sailry://session/child', view, parse: parse);
    expect(linked?.id, child);
    expect(linked?.child, reference);
  });

  test('leaves other sessions to the Node snapshot', () {
    final linked = linkedSession('sailry://session/other', view, parse: parse);
    expect(linked?.id, other);
    expect(linked?.child, isNull);
    final empty = linkedSession('sailry://session/child', {}, parse: parse);
    expect(empty?.id, child);
    expect(empty?.child, isNull);
  });

  test('ignores links Client does not parse', () {
    expect(linkedSession('https://example.com', view, parse: parse), isNull);
  });
}
