import 'dart:ui' show Tristate;

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
        final surface = find
            .ancestor(
              of: find.byType(MenuItemButton).first,
              matching: find.byWidgetPredicate(
                (widget) =>
                    widget is Surface && widget.kind == SurfaceKind.sheet,
              ),
            )
            .first;
        expect(surface, findsOneWidget);
        final fill =
            (tester
                        .widget<Ink>(
                          find
                              .descendant(
                                of: surface,
                                matching: find.byType(Ink),
                              )
                              .first,
                        )
                        .decoration!
                    as BoxDecoration)
                .gradient!;
        expect(
          fill,
          SailryTheme.glassFill(tester.element(surface), SurfaceKind.sheet),
        );
        expect(fill.colors.every((color) => color.a < .9), isTrue);
        expect(
          find.descendant(of: surface, matching: find.byType(BackdropFilter)),
          findsOneWidget,
        );
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
    await tester.tap(find.byType(SelectField<String>));
    await tester.pumpAndSettle();
    expect(find.byType(MenuItemButton), findsNWidgets(2));
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

  testWidgets('selection menu dismisses and restores field focus', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    try {
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.dark),
          home: Scaffold(
            body: Padding(
              padding: const EdgeInsets.all(20),
              child: SelectField<String>(
                label: 'Language',
                value: 'a',
                options: const [('a', 'First'), ('b', 'Second')],
                onChanged: (_) {},
              ),
            ),
          ),
        ),
      );
      final field = find.bySemanticsLabel('Language');
      expect(tester.getSemantics(field).flagsCollection.isButton, isTrue);
      expect(tester.getSemantics(field).value, 'First');
      await tester.tap(find.byType(SelectField<String>));
      await tester.pumpAndSettle();
      expect(
        tester.getSemantics(field).flagsCollection.isExpanded,
        Tristate.isTrue,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byType(MenuItemButton), findsNothing);
      expect(
        tester.widget<TextField>(find.byType(TextField)).focusNode!.hasFocus,
        isTrue,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
      await tester.pumpAndSettle();
      expect(find.byType(MenuItemButton), findsNWidgets(2));
      await tester.tapAt(const Offset(10, 450));
      await tester.pumpAndSettle();
      expect(find.byType(MenuItemButton), findsNothing);
      expect(
        tester.getSemantics(field).flagsCollection.isExpanded,
        Tristate.isFalse,
      );
      expect(tester.testTextInput.isVisible, isFalse);
      expect(tester.takeException(), isNull);
    } finally {
      semantics.dispose();
    }
  });

  testWidgets('long selection menus scroll within the glass surface', (
    tester,
  ) async {
    var chosen = -1;
    await mountSheet(
      tester,
      width: 320,
      child: SelectField<int>(
        label: 'Choose',
        value: 0,
        options: [
          for (var index = 0; index < 20; index++) (index, 'Option $index'),
        ],
        onChanged: (value) => chosen = value,
      ),
    );
    await tester.tap(find.byType(SelectField<int>));
    await tester.pumpAndSettle();
    final surface = find
        .ancestor(
          of: find.byType(MenuItemButton).first,
          matching: find.byType(Surface),
        )
        .first;
    expect(tester.getRect(surface).height, lessThanOrEqualTo(320));
    await tester.ensureVisible(
      find.widgetWithText(MenuItemButton, 'Option 19'),
    );
    await tester.pumpAndSettle();
    expect(find.text('Option 19').hitTestable(), findsOneWidget);
    await tester.tap(find.text('Option 19'));
    await tester.pumpAndSettle();
    expect(chosen, 19);
    expect(find.byType(MenuItemButton), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
