import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/speech_sheet.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/speech.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'runtime_test.dart' show Preferences;
import 'sheets_test.dart' show mountSheet;

void main() {
  for (final brightness in Brightness.values) {
    for (final width in [320.0, 390.0]) {
      testWidgets('speech controls have room at $width in $brightness', (
        tester,
      ) async {
        final preferences = Preferences();
        final speech = SpeechController('unused', preferences: preferences);
        addTearDown(speech.dispose);
        await mountSheet(
          tester,
          width: width,
          brightness: brightness,
          child: SpeechSheet(speech: speech),
        );
        final toggle = tester.getRect(find.byType(SwitchListTile));
        final field = tester.getRect(find.byType(SelectField<String>));
        final download = tester.getRect(
          find.widgetWithText(FilledButton, tr('settingsSpeechDownload')),
        );
        expect(field.top - toggle.bottom, greaterThanOrEqualTo(16));
        expect(download.top - field.bottom, greaterThanOrEqualTo(16));
        await tester.tap(find.byType(SelectField<String>));
        await tester.pumpAndSettle();
        final menu = tester.getRect(find.byType(MenuItemButton).first);
        expect(menu.height, greaterThanOrEqualTo(48));
        expect(menu.left, greaterThanOrEqualTo(0));
        expect(menu.right, lessThanOrEqualTo(width));
        expect(tester.testTextInput.isVisible, isFalse);
        await tester.tap(
          find.widgetWithText(MenuItemButton, tr('settingsSpeechEnglish')),
        );
        await tester.pumpAndSettle();
        expect(speech.language, 'en');
        expect(preferences.language, 'en');
        expect(find.byType(MenuItemButton), findsNothing);
        expect(find.text(tr('settingsSpeechEnglish')), findsOneWidget);
        expect(tester.takeException(), isNull);
      });
    }
  }

  testWidgets('selection follows external values and translated labels', (
    tester,
  ) async {
    final selection = ValueNotifier('a');
    final enabled = ValueNotifier(true);
    final translated = ValueNotifier(false);
    addTearDown(selection.dispose);
    addTearDown(enabled.dispose);
    addTearDown(translated.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Scaffold(
          body: ListenableBuilder(
            listenable: Listenable.merge([selection, enabled, translated]),
            builder: (context, _) => SelectField<String>(
              label: 'Language',
              value: selection.value,
              options: [
                ('a', translated.value ? 'Translated' : 'First'),
                ('b', 'Second'),
              ],
              onChanged: enabled.value
                  ? (value) => selection.value = value
                  : null,
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.byType(SelectField<String>));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(MenuItemButton, 'Second'));
    await tester.pumpAndSettle();
    expect(selection.value, 'b');
    selection.value = 'a';
    translated.value = true;
    await tester.pumpAndSettle();
    expect(find.text('Translated'), findsOneWidget);
    enabled.value = false;
    await tester.pumpAndSettle();
    await tester.tap(find.byType(SelectField<String>));
    await tester.pumpAndSettle();
    expect(find.byType(MenuItemButton), findsNothing);
    expect(selection.value, 'a');
    expect(tester.takeException(), isNull);
  });

  testWidgets('keyboard opens, selects and dismisses without a text keyboard', (
    tester,
  ) async {
    var selected = 'a';
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: SelectField<String>(
            label: 'Language',
            value: selected,
            options: const [('a', 'First'), ('b', 'Second')],
            onChanged: (value) => selected = value,
          ),
        ),
      ),
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.byType(MenuItemButton), findsNWidgets(2));
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(selected, 'b');
    expect(find.byType(MenuItemButton), findsNothing);
    expect(tester.testTextInput.isVisible, isFalse);
    expect(tester.takeException(), isNull);
  });
}
