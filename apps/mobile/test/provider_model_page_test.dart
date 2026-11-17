import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/providers/controller.dart';
import 'package:sailry_mobile/features/settings/providers/model_page.dart';
import 'package:sailry_mobile/features/settings/providers/parts.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

Map<String, dynamic> _provider(String authentication) => {
  'id': 'fixture',
  'revision': 1,
  'name': 'Fixture',
  'api': 'responses',
  'authentication': authentication,
  'default_model': 'fixture-model',
  'models': [
    {
      'id': 'fixture-model',
      'context': 128000,
      'output': 16384,
      'tools': true,
      'generates': ['image_tool'],
    },
  ],
};

Future<ProviderController> _showModel(
  WidgetTester tester, {
  String authentication = 'chat_gpt_oauth',
}) async {
  tester.view.physicalSize = const Size(900, 1400);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  final host = HostConnection.test(
    id: 'fixture',
    label: 'Fixture',
    command: (kind, data) async {
      expect(kind, 'save_provider');
      return {
        'data': {
          ...data!['provider'] as Map<String, dynamic>,
          'revision': (data['expected_revision'] as int) + 1,
        },
      };
    },
  );
  final controller = ProviderController(host, _provider(authentication));
  addTearDown(host.dispose);
  addTearDown(controller.dispose);
  await tester.pumpWidget(
    MaterialApp(
      locale: const Locale('en'),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      theme: SailryTheme.of(Brightness.light),
      home: ModelPage(controller: controller, id: 'fixture-model'),
    ),
  );
  await tester.pumpAndSettle();
  return controller;
}

void main() {
  testWidgets('context editing bounds an output limit the page does not show', (
    tester,
  ) async {
    final controller = await _showModel(tester);
    await tester.tap(find.text('Context length'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), '8192');
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();
    final saved = (controller.provider['models'] as List).single as Map;
    expect(saved['context'], 8192);
    expect(saved['output'], 8192);
    expect(find.byType(TextField), findsNothing);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  for (final authentication in ['api_key', 'chat_gpt_oauth']) {
    testWidgets(
      'image route survives Tools and Image toggles ($authentication)',
      (tester) async {
        final controller = await _showModel(
          tester,
          authentication: authentication,
        );
        Finder toggle(String label) => find.descendant(
          of: find
              .byWidgetPredicate(
                (widget) => widget is SwitchRow && widget.label == label,
              )
              .last,
          matching: find.byType(AppSwitch),
        );
        for (final label in ['Tools', 'Image', 'Image']) {
          await tester.ensureVisible(toggle(label));
          await tester.tap(toggle(label));
          await tester.pumpAndSettle();
        }
        final saved = (controller.provider['models'] as List).single as Map;
        expect(saved['tools'], isFalse);
        expect(saved['generates'], ['image_tool']);
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }
}
