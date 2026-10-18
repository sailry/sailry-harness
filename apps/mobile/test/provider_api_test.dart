import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/provider_api.dart' as api;

void main() {
  test('opaque provider APIs retain their identity', () {
    expect(api.choices('plugin').where((value) => value == 'plugin'), [
      'plugin',
    ]);
    expect(api.editable('plugin'), isFalse);
    expect(api.choices('future_transport').last, 'future_transport');
    expect(api.editable('future_transport'), isFalse);
    expect(api.choices('chat_completions'), api.values);
    expect(api.editable('chat_completions'), isTrue);
    expect(api.values, isNot(contains('plugin')));
  });
}
