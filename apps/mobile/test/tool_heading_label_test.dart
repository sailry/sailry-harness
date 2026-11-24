import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/tool_heading.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  testWidgets('built-in tools show the application label, not their name', (
    tester,
  ) async {
    for (final name in [
      'read_tool_output',
      'tool_search',
      'present_files',
      'set_session_title',
    ]) {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ToolHeading(call: {'name': name, 'state': 'returned'}),
          ),
        ),
      );
      expect(find.text(tr('tool_$name')), findsOneWidget);
      expect(find.text(name), findsNothing);
    }
  });
}
