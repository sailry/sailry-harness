import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';

/// One session's captured team roster with the host's stored team catalog.
class SessionTeam {
  const SessionTeam(this.session, this.teams);

  final Map<String, dynamic> session;

  /// Stored teams only; the Default team is virtual and selects every role.
  final List<Map<String, dynamic>> teams;

  /// The latest admitted Lead configuration, including changes made in the picker.
  Map<String, dynamic> get config => object(session['config']);

  Map<String, dynamic> get _roles => object(session['roles']);

  /// The selected stored team, or null for the Default team. A team removed
  /// from the catalog keeps only its identity.
  Map<String, dynamic>? get team {
    final reference = _roles['team'];
    if (reference is! Map) return null;
    final id = reference['id'];
    return teams.where((team) => team['id'] == id).firstOrNull ?? {'id': id};
  }

  /// Captured roles in roster order.
  List<Map<String, dynamic>> get members => objects(_roles['profiles']);

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
}

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

  /// Selects a stored team, or the Default team for null.
  final Future<void> Function(Map<String, dynamic>? team) onTeam;

  /// Sets one member's model, or clears its override for null.
  final Future<void> Function(String role, Map<String, dynamic>? model)
  onMember;
}

/// `set_session_team` data; a null team selects the Default team.
Map<String, dynamic> selectCommand(
  String session,
  int revision,
  Map<String, dynamic>? team,
) => {
  'session': session,
  'expected_revision': revision,
  'team': team == null
      ? null
      : {'id': team['id'], 'revision': team['revision']},
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
        },
};

/// The lead configuration a team fixes, or null when it keeps the current one.
Map<String, dynamic>? leadConfig(
  Map<String, dynamic> config,
  Map<String, dynamic> team,
  List<Map<String, dynamic>> providers,
) {
  final model = team['model'];
  if (model is! Map) return null;
  final provider = providers
      .where((item) => item['id'] == model['provider'])
      .firstOrNull;
  final definition = objects(
    provider?['models'],
  ).where((item) => item['id'] == model['model']).firstOrNull;
  final next = {
    ...config,
    'provider': model['provider'],
    'credential': provider?['credential'],
    'model': model['model'],
    'effort': model['effort'] ?? definition?['default_effort'] ?? 'default',
  };
  final unchanged =
      next['provider'] == config['provider'] &&
      next['model'] == config['model'] &&
      next['effort'] == config['effort'];
  return unchanged ? null : next;
}

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
