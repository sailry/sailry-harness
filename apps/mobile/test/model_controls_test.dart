import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/model_controls.dart';
import 'package:sailry_mobile/features/conversations/live/team.dart';

void main() {
  const saved = <String, dynamic>{
    'provider': 'provider',
    'credential': 'old-credential',
    'model': 'old-model',
    'effort': 'high',
    'mode': 'flagship',
    'permission': 'ask',
    'features': {'speed': 'normal'},
  };
  final unsaved = <String, dynamic>{
    ...saved,
    'mode': 'plan',
    'permission': 'allow',
    'features': {'speed': 'fast'},
  };
  const providers = <Map<String, dynamic>>[
    {
      'id': 'provider',
      'credential': 'new-credential',
      'name': 'Fixture provider',
      'enabled': true,
      'models': [
        {'id': 'old-model', 'display_name': 'Old model'},
        {
          'id': 'new-model',
          'display_name': 'Chosen model',
          'default_effort': 'medium',
        },
      ],
    },
  ];

  for (final withTeam in [false, true]) {
    for (final updateWhileOpen in [false, true]) {
      testWidgets('Lead choice preserves the current draft '
          '(team=$withTeam, updateWhileOpen=$updateWhileOpen)', (tester) async {
        // A failed save leaves the form draft ahead of the admitted team.
        final draft = ValueNotifier<Map<String, dynamic>>(unsaved);
        addTearDown(draft.dispose);
        final changed = <Map<String, dynamic>>[];
        final committed = <Map<String, dynamic>>[];
        final team = withTeam
            ? TeamChoices(
                listenable: draft,
                read: () => const SessionTeam({'config': saved}, []),
                onTeam: (_) async => fail('Lead choice changed the team'),
                onMember: (_, _) async => fail('Lead choice changed a member'),
              )
            : null;
        await tester.pumpWidget(
          MaterialApp(
            home: Scaffold(
              body: ValueListenableBuilder<Map<String, dynamic>>(
                valueListenable: draft,
                builder: (context, config, _) => ModelControls(
                  providers: providers,
                  config: config,
                  team: team,
                  onChanged: (value) {
                    changed.add(value);
                    draft.value = value;
                  },
                  onCommitted: committed.add,
                ),
              ),
            ),
          ),
        );
        await tester.tap(find.byKey(const ValueKey('choose-model')));
        await tester.pumpAndSettle();
        expect(find.byType(ModelChoices), findsOneWidget);

        if (updateWhileOpen) {
          draft.value = {
            ...unsaved,
            'permission': 'ask',
            'features': {'speed': 'ultrafast'},
          };
          await tester.pump();
        }
        final before = draft.value;
        expect(changed, isEmpty);
        expect(committed, isEmpty);
        await tester.tap(find.text('Chosen model'));
        await tester.pumpAndSettle();

        final expected = {
          ...before,
          'provider': 'provider',
          'credential': 'new-credential',
          'model': 'new-model',
          'effort': 'medium',
        };
        expect(changed, [expected]);
        expect(committed, [expected]);
        expect(draft.value, expected);
        expect(before['model'], 'old-model');
        expect(saved['mode'], 'flagship');
        expect(team?.read().config, withTeam ? saved : null);
        expect(find.byType(ModelChoices), findsNothing);
      });
    }
  }
}
