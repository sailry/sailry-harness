import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// The debug-only test input client is not available in profile builds.
Future<void> edit(WidgetTester tester, Finder finder, String value) async {
  final editor = tester.state<EditableTextState>(
    find.descendant(
      of: finder,
      matching: find.byType(EditableText),
      matchRoot: true,
    ),
  );
  editor.requestKeyboard();
  await tester.pump();
  editor.updateEditingValue(
    TextEditingValue(
      text: value,
      selection: TextSelection.collapsed(offset: value.length),
    ),
  );
  await tester.pump();
}
