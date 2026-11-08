import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';

/// One session's captured team roster with the host's stored team catalog.
class SessionTeam {
  const SessionTeam(
    this.session,
    this.teams, {
    this.catalog = const [],
    this.providers = const [],
  });

  final Map<String, dynamic> session;

  /// Stored teams only; the Default team is virtual and selects every role.
  final List<Map<String, dynamic>> teams;

  /// Current role identities, used only for their display appearance.
  final List<Map<String, dynamic>> catalog;
  final List<Map<String, dynamic>> providers;

  /// The latest admitted Lead configuration, including changes made in the picker.
  Map<String, dynamic> get config => object(session['config']);

  Map<String, dynamic> get _roles => object(session['roles']);

  String get execution => text(_roles['execution'], 'team');

  /// The selected stored team, or null for the Default team. A team removed
  /// from the catalog keeps only its identity.
  Map<String, dynamic>? get team {
    if (execution == 'model') return null;
    final reference = _roles['team'];
    if (reference is! Map) return null;
    final id = reference['id'];
    return teams.where((team) => team['id'] == id).firstOrNull ?? {'id': id};
  }

  /// Captured roles in roster order.
  List<Map<String, dynamic>> get members =>
      execution == 'model' ? const [] : objects(_roles['profiles']);

  /// Match the current role by identity; removed roles retain their capture.
  Map<String, dynamic> appearance(Map<String, dynamic> member) => object(
    (catalog.where((role) => role['id'] == member['id']).firstOrNull ??
        member)['appearance'],
  );

  Map<String, dynamic> get _overrides => object(_roles['models']);

  bool overridden(String role) => _overrides[role] is Map;

  /// The override, else the captured role model; null follows the lead.
  Map<String, dynamic>? model(Map<String, dynamic> member) {
    final override = _overrides[member['id']];
    if (override is Map) return object(override);
    final own = member['model'];
    return own is Map ? object(own) : null;
  }

  /// A picker selection follows the latest Lead when the member has no model.
  Map<String, dynamic> selection(Map<String, dynamic>? member) =>
      (member == null ? null : model(member)) ?? config;

  Map<String, dynamic> features(Map<String, dynamic> member) {
    final selected = selection(member);
    return _retainFeatures(
      selected,
      object(selected['features'] ?? config['features']),
    );
  }

  List<Map<String, dynamic>> featureOptions(Map<String, dynamic> selected) {
    final provider = providers
        .where((provider) => provider['id'] == selected['provider'])
        .firstOrNull;
    final definition = objects(
      provider?['models'],
    ).where((model) => model['id'] == selected['model']).firstOrNull;
    final declared = objects(object(definition?['capabilities'])['features']);
    return declared
        .map((feature) {
          final options = objects(feature['options'])
              .where(
                (option) =>
                    _supported(provider?['api'], feature['id'], option['id']),
              )
              .toList();
          return {...feature, 'options': options};
        })
        .where((feature) => (feature['options'] as List).isNotEmpty)
        .toList();
  }

  Map<String, dynamic> _retainFeatures(
    Map<String, dynamic> selected,
    Map<String, dynamic> values,
  ) {
    final declared = featureOptions(selected);
    return {
      for (final entry in values.entries)
        if (declared.any(
          (feature) =>
              feature['id'] == entry.key &&
              objects(
                feature['options'],
              ).any((option) => option['id'] == entry.value),
        ))
          entry.key: entry.value,
    };
  }

  Map<String, dynamic> choose(
    Map<String, dynamic> member,
    Map<String, dynamic> choice,
  ) {
    final current = model(member);
    return {
      'provider': choice['provider'],
      'model': choice['model'],
      'effort': choice['effort'],
      if (current?['features'] != null)
        'features': _retainFeatures(choice, object(current!['features'])),
    };
  }

  Map<String, dynamic> feature(
    Map<String, dynamic> member,
    String id,
    String option,
  ) {
    final selected = selection(member);
    final chosen = features(member)..remove(id);
    if (option.isNotEmpty) chosen[id] = option;
    return {
      ...choose(member, selected),
      'features': _retainFeatures(selected, chosen),
    };
  }

  /// A member's current model with another effort; a member following the
  /// Lead fixes the Lead model.
  Map<String, dynamic> effort(Map<String, dynamic> member, Object? effort) =>
      choose(member, {...selection(member), 'effort': effort});
}

bool _supported(Object? api, Object? feature, Object? option) =>
    api == 'plugin' ||
    api == 'responses' &&
        feature == 'speed' &&
        (option == 'fast' || option == 'ultrafast');

/// Live team state and the owner's revision-checked team commands.
class TeamChoices {
  const TeamChoices({
    required this.listenable,
    required this.read,
    required this.onTeam,
    required this.onMember,
  });

  final Listenable listenable;
  final SessionTeam Function() read;

  /// Selects a stored team; null selects the Default team.
  final Future<void> Function(Map<String, dynamic>? team) onTeam;

  /// Sets one member's model, or clears its override for null.
  final Future<void> Function(String role, Map<String, dynamic>? model)
  onMember;
}

/// `set_session_team` data; team selection clears member overrides.
Map<String, dynamic> selectCommand(
  String session,
  int revision,
  Map<String, dynamic>? team,
) => {
  'session': session,
  'expected_revision': revision,
  'execution': 'team',
  'team': team == null
      ? null
      : {'id': team['id'], 'revision': team['revision']},
  'models': <String, dynamic>{},
};

/// `set_session_member_model` data; a null model clears the override.
Map<String, dynamic> memberCommand(
  String session,
  int revision,
  String role,
  Map<String, dynamic>? model,
) => {
  'session': session,
  'expected_revision': revision,
  'role': role,
  'model': model == null
      ? null
      : {
          'provider': model['provider'],
          'model': model['model'],
          'effort': model['effort'],
          if (model.containsKey('features')) 'features': model['features'],
        },
};

/// The model chip shows a non-default team's name instead of the lead model.
String chipLabel(
  SessionTeam? state,
  Map<String, dynamic> config,
  String Function(String key) tr,
) {
  final team = state?.team;
  if (team != null) return text(team['name'], tr('team'));
  final model = text(config['model']);
  return model.isEmpty ? tr('conversationNoModel') : model;
}
